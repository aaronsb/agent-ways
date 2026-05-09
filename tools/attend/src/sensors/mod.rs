//! Sensor module — re-exports from sensor crates and local ScriptSensor.
//!
//! Sensor crates are wired in via feature flags. Each sensor is compiled only
//! when its feature is enabled (default: all). Config controls runtime
//! activation/deactivation independent of compilation.

mod script;

// Re-export from sensor-trait (always available)
pub use sensor_trait::{Focus, Sensor, SensorSlot};

// Re-export from sensor crates (feature-gated)
#[cfg(feature = "sensor-git")]
pub use sensor_git::GitSensor;

#[cfg(feature = "sensor-context")]
pub use sensor_context::ContextSensor;

#[cfg(feature = "sensor-peers")]
pub use sensor_peers::{PeerSensor, find_own_session_id};

#[cfg(feature = "sensor-processes")]
pub use sensor_processes::ProcessSensor;

#[cfg(feature = "sensor-disclosure")]
pub use sensor_disclosure::DisclosureSensor;

// ScriptSensor stays in attend — it's part of the orchestrator, not a sensor impl
pub use script::ScriptSensor;

// ── Sensor registration ─────────────────────────────────────────

use crate::config::Config;
use crate::groups::Groups;
#[allow(unused_imports)]
use std::time::Duration;

/// Register all enabled sensors based on config and feature flags.
///
/// To add a new sensor crate:
/// 1. Create the crate implementing sensor_trait::Sensor
/// 2. Add it as an optional dep + feature in attend/Cargo.toml
/// 3. Add a #[cfg(feature)] block below with register_builtin!
///
/// Config controls runtime: `-sensorname` in attend.yaml disables it.
/// Feature flags control compilation: `--no-default-features` excludes it.
#[allow(unused_variables)]
pub fn register_sensors(
    cfg: &Config,
    focus: &Focus,
    catchup: bool,
    groups: &Groups,
) -> (Vec<SensorSlot>, Vec<String>) {
    let mut slots: Vec<SensorSlot> = Vec::new();
    let mut enabled_names: Vec<String> = Vec::new();

    // Macro to reduce boilerplate for built-in sensor registration.
    // Checks: feature compiled + config enabled → instantiate with config overrides.
    #[allow(unused_macros)]
    macro_rules! register_builtin {
        ($name:expr, $sensor:expr, $default_interval:expr, $default_min:expr, $default_decay:expr) => {
            if cfg.sensors.get($name).map(|s| s.enabled).unwrap_or(true) {
                let sc = cfg.sensors.get($name);
                slots.push(SensorSlot::new_with_config(
                    Box::new($sensor),
                    sc.map(|s| s.interval).unwrap_or(Duration::from_secs($default_interval)),
                    sc.map(|s| s.min_interval).unwrap_or(Duration::from_secs($default_min)),
                    sc.map(|s| s.decay_threshold).unwrap_or($default_decay),
                ));
                enabled_names.push($name.to_string());
            }
        };
    }

    // ── Built-in crate sensors (feature-gated) ──────────────────

    #[cfg(feature = "sensor-context")]
    register_builtin!("context", ContextSensor::new(), 60, 20, 3);

    #[cfg(feature = "sensor-processes")]
    {
        // Resolve the watch list once at startup: if config provides one,
        // it replaces the defaults verbatim (explicit-replace contract).
        // Otherwise the sensor's `new()` uses `DEFAULT_WATCH`.
        let processes_sensor = match cfg.sensors.get("processes").and_then(|s| s.watch.clone()) {
            Some(list) => ProcessSensor::with_watch(list),
            None => ProcessSensor::new(),
        };
        register_builtin!("processes", processes_sensor, 30, 5, 5);
    }

    #[cfg(feature = "sensor-git")]
    register_builtin!("git", GitSensor::new(), 30, 10, 4);

    #[cfg(feature = "sensor-disclosure")]
    register_builtin!("disclosure", DisclosureSensor::new(), 60, 20, 3);

    #[cfg(feature = "sensor-peers")]
    {
        if cfg.sensors.get("peers").map(|s| s.enabled).unwrap_or(true) {
            let mut peer_sensor = PeerSensor::new();
            // Register a provider that re-reads group membership on every
            // poll, so mid-session focus-group join/leave is reflected
            // without restarting the sensor loop (ADR-118 + issue #15).
            let groups_for_scan = groups.clone();
            peer_sensor.set_extra_scan_dirs_provider(std::sync::Arc::new(move || {
                groups_for_scan
                    .joined_group_names()
                    .into_iter()
                    .map(|name| groups_for_scan.group_dir(&name))
                    .collect()
            }));
            // Align per-peer engagement window with global engagement config.
            peer_sensor.set_peer_activity_window(cfg.engagement.peer_activity_window);
            // ADR-121 outward gate: apply the `signals:` block params so
            // sensor-peers suppresses aged backlog and resets on `re:`
            // replies. Defaults inside PeerSensor match SignalsConfig
            // defaults, so this call is a no-op when the user's config
            // does not override them.
            //
            // The salience gate replaces the legacy `mark_existing_as_seen`
            // startup blast prevention. On a fresh session start, every
            // pre-existing signal file flows through `read_signals` and is
            // filtered by age against its on-disk mtime — the backlog-filter
            // behavior ADR-121 designed. `catchup` still skips the pre-seed
            // path; the gate takes over in both modes and is the sole
            // mechanism deciding what surfaces.
            peer_sensor.set_salience_params(
                cfg.signals.half_life_seconds,
                cfg.signals.presentation_floor,
            );
            let sc = cfg.sensors.get("peers");
            slots.push(SensorSlot::new_with_config(
                Box::new(peer_sensor),
                sc.map(|s| s.interval).unwrap_or(Duration::from_secs(30)),
                sc.map(|s| s.min_interval).unwrap_or(Duration::from_secs(10)),
                sc.map(|s| s.decay_threshold).unwrap_or(5),
            ));
            enabled_names.push("peers".to_string());
        }
    }

    // ── Script sensors (config-driven, always available) ────────

    for (name, sc) in &cfg.sensors {
        if let Some(ref script_path) = sc.script {
            if sc.enabled {
                let sensor = ScriptSensor::new(
                    name.clone(),
                    script_path.clone(),
                    focus.working_dir.clone(),
                    sc.interval,
                    sc.min_interval,
                    sc.decay_threshold,
                    sc.threshold,
                );
                slots.push(SensorSlot::new_with_config(
                    Box::new(sensor),
                    sc.interval,
                    sc.min_interval,
                    sc.decay_threshold,
                ));
                enabled_names.push(name.clone());
            }
        }
    }

    (slots, enabled_names)
}

// ── Sensor enumeration (for `attend sensors`) ───────────────────

/// Lightweight metadata for one sensor — used by `attend sensors` to
/// report what's compiled, configured, and active.
pub struct SensorEntry {
    pub name: String,
    pub kind: SensorKind,
    pub state: SensorState,
    pub description: String,
    pub source: String,
    pub interval: Duration,
    pub min_interval: Duration,
}

#[derive(Clone, Copy)]
pub enum SensorKind {
    Builtin,
    Script,
}

impl SensorKind {
    pub fn label(self) -> &'static str {
        match self {
            SensorKind::Builtin => "builtin",
            SensorKind::Script => "script",
        }
    }
}

#[derive(Clone, Copy)]
pub enum SensorState {
    /// Compiled in (or script file present) and enabled in config.
    Active,
    /// Compiled in (or script file present) but disabled in config.
    Off,
    /// Built-in sensor whose feature flag was excluded at compile time.
    /// Only constructible when a `sensor-*` feature is off, so default
    /// builds correctly flag it as unused.
    #[allow(dead_code)]
    NotCompiled,
    /// Script sensor whose `script:` path does not point at a file.
    Missing,
}

impl SensorState {
    pub fn label(self) -> &'static str {
        match self {
            SensorState::Active => "active",
            SensorState::Off => "off",
            SensorState::NotCompiled => "not compiled",
            SensorState::Missing => "missing",
        }
    }
}

/// Push one builtin sensor's metadata into `entries`. Two `#[cfg]`-gated
/// branches per invocation: the feature-on branch instantiates the sensor
/// to read its trait-supplied `description()` / `source()` / `name()`; the
/// feature-off branch emits a `NotCompiled` placeholder so the slot is
/// still visible in the table.
///
/// `$sensor` is only parsed lexically as an expression — when its feature
/// is off, the entire block (and the substituted expression with it) is
/// stripped before name resolution, so it's fine if the type doesn't exist
/// in that build.
macro_rules! enumerate_builtin {
    ($entries:ident, $cfg:expr, $feature:literal, $name:literal, $base:expr, $min:expr, $sensor:expr) => {{
        #[cfg(feature = $feature)]
        {
            let s = $sensor;
            let (state, interval, min_interval) = builtin_state_for($cfg, $name, $base, $min);
            $entries.push(SensorEntry {
                name: s.name().to_string(),
                kind: SensorKind::Builtin,
                state,
                description: s.description().to_string(),
                source: s.source(),
                interval,
                min_interval,
            });
        }
        #[cfg(not(feature = $feature))]
        {
            let (_, interval, min_interval) = builtin_state_for($cfg, $name, $base, $min);
            $entries.push(SensorEntry {
                name: $name.to_string(),
                kind: SensorKind::Builtin,
                state: SensorState::NotCompiled,
                description: String::new(),
                source: String::new(),
                interval,
                min_interval,
            });
        }
    }};
}

/// Enumerate every sensor known to this build — both compiled-in
/// built-ins and config-defined script sensors. Pure metadata, no
/// side effects; safe to call from any subcommand.
pub fn enumerate_sensors(cfg: &Config, focus: &Focus) -> Vec<SensorEntry> {
    let mut entries: Vec<SensorEntry> = Vec::new();

    // Built-in sensors. Defaults here mirror the macro defaults in
    // `register_sensors` and the seeds in `Config::default()` — keep them
    // in sync if you change either source.
    enumerate_builtin!(entries, cfg, "sensor-context", "context",
        Duration::from_secs(60), Duration::from_secs(20),
        ContextSensor::new());
    enumerate_builtin!(entries, cfg, "sensor-git", "git",
        Duration::from_secs(30), Duration::from_secs(10),
        GitSensor::new());
    enumerate_builtin!(entries, cfg, "sensor-peers", "peers",
        Duration::from_secs(30), Duration::from_secs(10),
        PeerSensor::new());
    enumerate_builtin!(entries, cfg, "sensor-processes", "processes",
        Duration::from_secs(30), Duration::from_secs(5),
        match cfg.sensors.get("processes").and_then(|sc| sc.watch.clone()) {
            Some(list) => ProcessSensor::with_watch(list),
            None => ProcessSensor::new(),
        });
    enumerate_builtin!(entries, cfg, "sensor-disclosure", "disclosure",
        Duration::from_secs(60), Duration::from_secs(20),
        DisclosureSensor::new());

    // Script sensors — anything in config with `script:` set, regardless
    // of whether the file currently resolves.
    for (name, sc) in &cfg.sensors {
        let Some(script_path) = sc.script.clone() else { continue };
        let probe = ScriptSensor::new(
            name.clone(),
            script_path.clone(),
            focus.working_dir.clone(),
            sc.interval,
            sc.min_interval,
            sc.decay_threshold,
            sc.threshold,
        );
        let resolved = std::path::Path::new(&script_path);
        let resolved = if resolved.is_absolute() {
            resolved.to_path_buf()
        } else {
            std::path::Path::new(&focus.working_dir).join(resolved)
        };
        let state = if !resolved.is_file() {
            SensorState::Missing
        } else if sc.enabled {
            SensorState::Active
        } else {
            SensorState::Off
        };
        entries.push(SensorEntry {
            name: probe.name().to_string(),
            kind: SensorKind::Script,
            state,
            description: probe.description().to_string(),
            source: probe.source(),
            interval: sc.interval,
            min_interval: sc.min_interval,
        });
    }

    entries
}

fn builtin_state_for(
    cfg: &Config,
    name: &str,
    default_base: Duration,
    default_min: Duration,
) -> (SensorState, Duration, Duration) {
    let entry = cfg.sensors.get(name);
    let enabled = entry.map(|s| s.enabled).unwrap_or(true);
    let state = if enabled { SensorState::Active } else { SensorState::Off };
    (
        state,
        entry.map(|s| s.interval).unwrap_or(default_base),
        entry.map(|s| s.min_interval).unwrap_or(default_min),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every compiled-in built-in sensor must expose a non-empty description
    /// and a `crate@version`-shaped source string. Insurance for future
    /// sensor authors who forget to invoke `sensor_trait::sensor_metadata!()`
    /// in their `impl Sensor` block — without this test, a missing override
    /// just renders as a blank cell in `attend sensors` rather than failing
    /// the build.
    #[test]
    fn builtin_sensors_expose_metadata() {
        let cfg = Config::default();
        let focus = Focus::default_focus();
        let entries = enumerate_sensors(&cfg, &focus);

        let mut compiled_builtins = 0;
        for e in &entries {
            if !matches!(e.kind, SensorKind::Builtin) {
                continue;
            }
            if matches!(e.state, SensorState::NotCompiled) {
                continue;
            }
            compiled_builtins += 1;

            assert!(
                !e.description.is_empty(),
                "{}: description is empty — invoke sensor_trait::sensor_metadata!() in its impl Sensor block",
                e.name,
            );

            let src = &e.source;
            let parts: Vec<&str> = src.split('@').collect();
            assert_eq!(
                parts.len(),
                2,
                "{}: source `{src}` is not crate@version shaped",
                e.name,
            );
            assert!(
                !parts[0].is_empty() && !parts[1].is_empty(),
                "{}: source `{src}` has empty crate or version segment",
                e.name,
            );
            // Cheap semver-shape check: at least two dots in the version.
            assert!(
                parts[1].matches('.').count() >= 2,
                "{}: source `{src}` version `{}` is not in major.minor.patch shape",
                e.name,
                parts[1],
            );
        }

        // The default feature set compiles all five built-ins; if a future
        // refactor changes that, this assertion catches it before the
        // metadata test silently passes with zero assertions.
        assert!(
            compiled_builtins > 0,
            "no built-in sensors were compiled in — adjust this test if defaults changed",
        );
    }
}
