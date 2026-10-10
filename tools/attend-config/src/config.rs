//! The typed settings attend runs on, resolved through the layers the
//! schema checked: defaults, then the user file, then the project's. Every
//! value here has passed the schema, so a conversion only falls back to the
//! default when a key is absent.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::time::Duration;

use agent_settings::load::{bindings, resolve, Layer};
use serde_yaml::Value;

use crate::schema::{sensor_defaults, BUILTINS, SCHEMA};

/// Top-level configuration.
#[derive(Debug, Clone)]
pub struct Config {
    pub governor: GovernorConfig,
    pub engagement: EngagementConfig,
    pub cleanup: CleanupConfig,
    pub sensors: HashMap<String, SensorConfig>,
    pub chat: ChatConfig,
}

/// attend-chat's own keys: how its tabs are reached, and the mouse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatConfig {
    /// `auto`, `ctrl`, `alt`, `both` or `none`.
    pub jump: String,
    pub menu_on_repeat: bool,
    /// `both`, `f2`, `ctrl-t` or `none`.
    pub focus_key: String,
    pub mouse: bool,
}

/// Background signal-file cleanup inside `attend run`. Reaping is by
/// project liveness (ADR-136), not by age.
#[derive(Debug, Clone)]
pub struct CleanupConfig {
    pub enabled: bool,
    pub interval: Duration,
}

/// Disclosure governor parameters.
#[derive(Debug, Clone)]
pub struct GovernorConfig {
    pub base_cooldown: Duration,
    pub max_per_window: u32,
    pub rate_window: Duration,
}

/// Action potential engagement parameters (ADR-123 `Curve::ActionPotential`).
#[derive(Debug, Clone)]
pub struct EngagementConfig {
    pub burst_threshold: usize,
    pub step_multiplier: f64,
    pub absolute_refractory: Duration,
    pub decay_per_minute: f64,
    pub peer_activity_window: Duration,
}

/// Per-sensor configuration.
#[derive(Debug, Clone)]
pub struct SensorConfig {
    pub enabled: bool,
    pub interval: Duration,
    pub min_interval: Duration,
    pub threshold: f64,
    pub decay_threshold: u32,
    /// A sensor of your own: its script, with `$HOME`, `~` and `$XDG_*`
    /// expanded.
    pub script: Option<String>,
    /// Permission requirements (ADR-116).
    pub requires: Vec<String>,
    /// The processes sensor's watch list. `None` keeps the sensor's
    /// built-in list; a list, even empty, replaces it.
    pub watch: Option<Vec<String>>,
}

impl Default for Config {
    fn default() -> Self {
        Config::from_layers(&[])
    }
}

/// Resolve the fixed key `name` through `layers`.
fn get(name: &str, layers: &[Layer]) -> Option<Value> {
    let spec = SCHEMA.keys.iter().find(|k| k.name == name)?;
    resolve(spec, &[], layers).value
}

/// Resolve the sensor key `attend.sensors.*.<field>` for `sensor`.
fn sensor(field: &str, sensor: &str, layers: &[Layer]) -> Option<Value> {
    let name = format!("attend.sensors.*.{field}");
    let spec = SCHEMA.keys.iter().find(|k| k.name == name)?;
    resolve(spec, &[sensor.to_string()], layers).value
}

fn secs(v: Option<Value>, default: u64) -> Duration {
    Duration::from_secs(v.and_then(|v| v.as_u64()).unwrap_or(default))
}

fn float(v: Option<Value>, default: f64) -> f64 {
    v.and_then(|v| v.as_f64()).unwrap_or(default)
}

fn list(v: Option<Value>) -> Option<Vec<String>> {
    Some(v?.as_sequence()?.iter().filter_map(|i| i.as_str().map(str::to_string)).collect())
}

impl Config {
    /// The configuration `layers` resolve to; no layers is the defaults.
    pub fn from_layers(layers: &[Layer]) -> Config {
        let governor = GovernorConfig {
            base_cooldown: secs(get("attend.governor.base_cooldown", layers), 15),
            max_per_window: get("attend.governor.max_per_window", layers).and_then(|v| v.as_u64()).unwrap_or(3) as u32,
            rate_window: secs(get("attend.governor.rate_window", layers), 120),
        };
        let engagement = EngagementConfig {
            burst_threshold: get("attend.engagement.burst_threshold", layers).and_then(|v| v.as_u64()).unwrap_or(3) as usize,
            step_multiplier: float(get("attend.engagement.step_multiplier", layers), 1.25),
            absolute_refractory: secs(get("attend.engagement.absolute_refractory", layers), 60),
            decay_per_minute: float(get("attend.engagement.decay_per_minute", layers), 0.1),
            peer_activity_window: secs(get("attend.engagement.peer_activity_window", layers), 900),
        };
        let cleanup = CleanupConfig {
            enabled: get("attend.cleanup.enabled", layers).and_then(|v| v.as_bool()).unwrap_or(true),
            interval: secs(get("attend.cleanup.interval", layers), 600),
        };
        let sensors = sensor_names(layers)
            .into_iter()
            .map(|n| {
                let d = sensor_defaults(&n);
                let c = SensorConfig {
                    enabled: sensor("enabled", &n, layers).and_then(|v| v.as_bool()).unwrap_or(true),
                    interval: secs(sensor("interval", &n, layers), d.interval),
                    min_interval: secs(sensor("min_interval", &n, layers), d.min_interval),
                    threshold: float(sensor("threshold", &n, layers), d.threshold),
                    decay_threshold: sensor("decay_threshold", &n, layers).and_then(|v| v.as_u64()).unwrap_or(d.decay_threshold) as u32,
                    script: sensor("script", &n, layers).and_then(|v| v.as_str().map(expand_path)),
                    requires: list(sensor("requires", &n, layers)).unwrap_or_else(|| d.requires.iter().map(|s| s.to_string()).collect()),
                    watch: list(sensor("watch", &n, layers)),
                };
                (n, c)
            })
            .collect();
        let text = |name: &str, default: &str| get(name, layers).and_then(|v| v.as_str().map(str::to_string)).unwrap_or_else(|| default.to_string());
        let chat = ChatConfig {
            jump: text("attend.chat.tabs.jump", "auto"),
            menu_on_repeat: get("attend.chat.tabs.menu_on_repeat", layers).and_then(|v| v.as_bool()).unwrap_or(true),
            focus_key: text("attend.chat.tabs.focus_key", "both"),
            mouse: get("attend.chat.mouse", layers).and_then(|v| v.as_bool()).unwrap_or(false),
        };
        Config { governor, engagement, cleanup, sensors, chat }
    }

    /// Load the user file, then the project's at `working_dir`. Each finding
    /// is one line on stderr naming the file, line and section; a section or
    /// sensor that fails falls through to the layers beneath, and a switch
    /// in it reads off (ADR-503 §4). Loading never rewrites a file.
    pub fn load(working_dir: &str) -> Self {
        let layers = crate::layers(Path::new(working_dir));
        for l in layers.iter().filter(|l| l.present) {
            if let Some(p) = &l.path {
                eprintln!("[attend] config: loaded {}", p.display());
            }
            for f in &l.findings {
                eprintln!("{}", f.diagnostic("attend"));
            }
        }
        Config::from_layers(&layers)
    }
}

/// The built-in sensors and every sensor a layer names, sorted.
fn sensor_names(layers: &[Layer]) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = BUILTINS.iter().map(|s| s.to_string()).collect();
    for spec in SCHEMA.keys.iter().filter(|k| k.section == "attend.sensors") {
        out.extend(bindings(spec, layers).into_iter().filter_map(|b| b.into_iter().next()));
    }
    out
}

/// Expand `~/`, `$HOME` and `$XDG_*` in a script path, so a config is
/// portable across installs.
pub fn expand_path(value: &str) -> String {
    let mut out = value.to_string();
    if let Some(home) = claude_sessions::env_home_dir() {
        let home = home.display().to_string();
        if let Some(rest) = out.strip_prefix("~/") {
            out = format!("{home}/{rest}");
        }
        out = out.replace("$HOME", &home);
    }
    for var in ["XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME"] {
        if let Ok(val) = std::env::var(var) {
            out = out.replace(&format!("${var}"), &val);
        }
    }
    out
}
