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
            if !catchup {
                peer_sensor.mark_existing_as_seen(focus);
            }
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
