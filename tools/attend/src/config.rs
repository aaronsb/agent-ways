//! Configuration loader for attend.
//!
//! Two-layer config mirrors ways scoping:
//!   ~/.config/attend/config.yaml          # user scope — always loaded
//!   {project}/.claude/attend.yaml         # project scope — layered on top
//!
//! Project config uses +/- to modify the sensor set.
//!
//! Minimal YAML subset parser — no serde dependency.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Top-level configuration.
#[derive(Debug, Clone)]
pub struct Config {
    pub governor: GovernorConfig,
    pub engagement: EngagementConfig,
    pub cleanup: CleanupConfig,
    pub sensors: HashMap<String, SensorConfig>,
}

/// Background signal-file cleanup. Runs inside `attend run` so the signals
/// base doesn't accumulate indefinitely. Defaults keep peer discussion
/// readable for ~30 days, which is long enough to pick up a conversation
/// the next day or the next week and short enough that nothing lives forever.
#[derive(Debug, Clone)]
pub struct CleanupConfig {
    /// Master switch. Disable to skip auto-cleanup entirely.
    pub enabled: bool,
    /// How often the sensor loop runs a cleanup sweep.
    pub interval: Duration,
    /// Signal files older than this are removed by auto-cleanup.
    pub retention: Duration,
}

impl Default for CleanupConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            // Every 10 minutes — directory scan is O(files) and cheap.
            interval: Duration::from_secs(600),
            // 30 days — conservative enough that peer discussion threads
            // stay readable across multi-day gaps.
            retention: Duration::from_secs(30 * 86400),
        }
    }
}

/// Disclosure governor parameters.
#[derive(Debug, Clone)]
pub struct GovernorConfig {
    pub base_cooldown: Duration,
    pub max_per_window: u32,
    pub rate_window: Duration,
}

/// Action potential engagement parameters (ADR-119).
///
/// Governs per-sensor refractory behavior: after a burst of disclosures,
/// the sensor enters a refractory period where only high-magnitude events
/// break through. This provides natural disengagement from conversations
/// that have plateaued in value.
///
/// Sane defaults are sized for Claude's actual turn cadence. Use
/// `attend tune` to auto-derive values from real session history.
#[derive(Debug, Clone)]
pub struct EngagementConfig {
    /// Window for counting recent disclosures toward a burst.
    pub burst_window: Duration,
    /// Disclosures within burst_window needed to trigger refractory.
    pub burst_threshold: usize,
    /// Multiplier added per disclosure past the burst threshold.
    pub step_multiplier: f64,
    /// Absolute refractory: no disclosures at all for this long after burst.
    pub absolute_refractory: Duration,
    /// Relative refractory multiplier decays by this amount per minute.
    pub decay_per_minute: f64,
    /// Per-peer engagement window (used by sensor-peers for per-peer
    /// magnitude boosting).
    pub peer_activity_window: Duration,
}

impl Default for EngagementConfig {
    fn default() -> Self {
        Self {
            // 15-minute window — sized so typical multi-turn conversations
            // (with Claude turns averaging 60–120s) stay within it.
            burst_window: Duration::from_secs(900),
            burst_threshold: 3,
            step_multiplier: 1.25,
            // One Claude turn (~60s median) of complete silence after burst.
            absolute_refractory: Duration::from_secs(60),
            // Decay to rest over ~12 min (from peak 2.25).
            decay_per_minute: 0.1,
            // Match burst window.
            peer_activity_window: Duration::from_secs(900),
        }
    }
}

/// Per-sensor configuration.
#[derive(Debug, Clone)]
pub struct SensorConfig {
    pub enabled: bool,
    pub interval: Duration,
    pub min_interval: Duration,
    pub threshold: f64,
    pub decay_threshold: u32,
    /// For script sensors: path to the script (relative to project root)
    pub script: Option<String>,
    /// Permission requirements (ADR-116) — tool permissions this sensor needs.
    pub requires: Vec<String>,
}

impl Default for GovernorConfig {
    fn default() -> Self {
        Self {
            base_cooldown: Duration::from_secs(15),
            max_per_window: 3,
            rate_window: Duration::from_secs(120),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        let mut sensors = HashMap::new();
        sensors.insert("context".to_string(), SensorConfig {
            enabled: true,
            interval: Duration::from_secs(60),
            min_interval: Duration::from_secs(20),
            threshold: 1.5,
            decay_threshold: 3,
            script: None,
            requires: vec!["Read".to_string()],
        });
        sensors.insert("git".to_string(), SensorConfig {
            enabled: true,
            interval: Duration::from_secs(30),
            min_interval: Duration::from_secs(10),
            threshold: 2.0,
            decay_threshold: 4,
            script: None,
            requires: vec!["Bash(git:*)".to_string()],
        });
        sensors.insert("peers".to_string(), SensorConfig {
            enabled: true,
            interval: Duration::from_secs(30),
            min_interval: Duration::from_secs(10),
            threshold: 2.0,
            decay_threshold: 5,
            script: None,
            requires: vec!["Read".to_string()],
        });
        sensors.insert("processes".to_string(), SensorConfig {
            enabled: true,
            interval: Duration::from_secs(30),
            min_interval: Duration::from_secs(5),
            threshold: 2.0,
            decay_threshold: 5,
            script: None,
            requires: vec!["Bash(ps:*)".to_string()],
        });
        Self {
            governor: GovernorConfig::default(),
            engagement: EngagementConfig::default(),
            cleanup: CleanupConfig::default(),
            sensors,
        }
    }
}

impl Config {
    /// Load config from user scope, then layer project scope on top.
    pub fn load(working_dir: &str) -> Self {
        let mut config = Config::default();

        // User scope: ~/.config/attend/config.yaml
        let user_path = user_config_path();
        if user_path.exists() {
            if let Ok(content) = fs::read_to_string(&user_path) {
                apply_config(&mut config, &content);
                eprintln!("[attend] config: loaded {}", user_path.display());
            }
        }

        // Project scope: {cwd}/.claude/attend.yaml
        let project_path = Path::new(working_dir).join(".claude").join("attend.yaml");
        if project_path.exists() {
            if let Ok(content) = fs::read_to_string(&project_path) {
                apply_config(&mut config, &content);
                eprintln!("[attend] config: loaded {}", project_path.display());
            }
        }

        config
    }

    /// Write default config to user scope path. Creates dirs if needed.
    pub fn init_user_config() -> PathBuf {
        let path = user_config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        fs::write(&path, Self::default_yaml()).ok();
        path
    }

    /// Generate a default config file as a string.
    pub fn default_yaml() -> String {
        r#"# attend configuration
# User scope: ~/.config/attend/config.yaml
# Project scope: {project}/.claude/attend.yaml (layered on top)

governor:
  base_cooldown: 15
  max_per_window: 3
  rate_window: 120

# Action potential engagement model (ADR-119).
# Run `attend tune` to auto-derive these from real session history.
engagement:
  burst_window: 900          # seconds — burst counting window
  burst_threshold: 3         # disclosures before refractory kicks in
  step_multiplier: 1.25      # per-burst threshold elevation
  absolute_refractory: 60    # seconds of complete suppression after burst
  decay_per_minute: 0.1      # relative refractory decay rate
  peer_activity_window: 900  # per-peer engagement window

# Background signal-file cleanup. Runs inside `attend run` so the signals
# base doesn't accumulate indefinitely. `attend cleanup` is the manual
# escape hatch with --older-than / --all / --dry-run.
cleanup:
  enabled: true
  interval: 600              # seconds — how often the sweep runs (10 min)
  retention: 2592000         # seconds — age cutoff (30 days)

sensors:
  context:
    interval: 60
    min_interval: 20
    threshold: 1.5
  git:
    interval: 30
    min_interval: 10
    threshold: 2.0
  peers:
    interval: 30
    min_interval: 10
    threshold: 2.0
  processes:
    interval: 30
    min_interval: 5
    threshold: 2.0

  # Example external sensor — disabled by default.
  #
  # To enable:
  #   1. Copy the example script from the agent-ways repo to a path
  #      where you keep trusted user scripts. XDG-convention default:
  #        mkdir -p $XDG_DATA_HOME/attend/sensors
  #        cp tools/attend/examples/xdg-downloads.sh \
  #           $XDG_DATA_HOME/attend/sensors/
  #      (Or put it anywhere you trust — ~/bin, .claude/sensors/ in a
  #      project dir, a team-shared tools repo, wherever.)
  #   2. Review the script. External sensors run arbitrary shell under
  #      your user. You should read any sensor before enabling it.
  #   3. Flip `enabled: true` below.
  #
  # Sensor sources are deliberately unconstrained — they can be
  # user-global (like this example), project-scoped in .claude/sensors/,
  # or absolute paths to anywhere on disk. attend only cares that the
  # path resolves and the script respects the subprocess contract
  # documented in docs/attend-and-monitor/authoring-sensors.md.
  #
  # This particular example watches XDG Downloads for new files and
  # demonstrates the magnitude-as-design-lever pattern — a single new
  # file fires at 2.0, a batch fires at 3.0.
  +xdg-downloads:
    script: $XDG_DATA_HOME/attend/sensors/xdg-downloads.sh
    enabled: false
    interval: 120
    min_interval: 30
    threshold: 2.0
    decay_threshold: 3

  # Second shipped example: gh-pr-checks. Watches `gh pr checks` for
  # the current branch's PR and emits on CI state transitions
  # (pass / fail). Requires git, gh, and jq on PATH. Silent on
  # main/master and on branches with no PR. Enable the same way as
  # xdg-downloads above: copy the script, review, flip enabled.
  #+gh-pr-checks:
  #  script: $XDG_DATA_HOME/attend/sensors/gh-pr-checks.sh
  #  enabled: false
  #  interval: 120
  #  min_interval: 30
  #  threshold: 2.0

# Project-scope example (in .claude/attend.yaml):
#
# sensors:
#   +disk-pressure:
#     script: .claude/sensors/check-disk.sh
#     interval: 120
#     threshold: 3.0
#   -processes:
"#.to_string()
    }
}

fn user_config_path() -> PathBuf {
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".config")
        });
    config_dir.join("attend").join("config.yaml")
}

/// Apply a YAML config string to an existing Config, overriding values.
/// Handles the +/- sensor syntax for project-scope overlays.
fn apply_config(config: &mut Config, content: &str) {
    let mut current_section = String::new();
    let mut current_sensor = String::new();

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip comments and empty lines
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let indent = line.len() - line.trim_start().len();

        // Top-level section
        if indent == 0 && trimmed.ends_with(':') {
            current_section = trimmed.trim_end_matches(':').to_string();
            current_sensor.clear();
            continue;
        }

        // Second-level: sensor name or governor key
        if indent == 2 {
            if current_section == "governor" {
                if let Some((key, value)) = parse_kv(trimmed) {
                    match key {
                        "base_cooldown" => {
                            if let Ok(v) = value.parse::<u64>() {
                                config.governor.base_cooldown = Duration::from_secs(v);
                            }
                        }
                        "max_per_window" => {
                            if let Ok(v) = value.parse::<u32>() {
                                config.governor.max_per_window = v;
                            }
                        }
                        "rate_window" => {
                            if let Ok(v) = value.parse::<u64>() {
                                config.governor.rate_window = Duration::from_secs(v);
                            }
                        }
                        _ => {}
                    }
                }
            } else if current_section == "cleanup" {
                if let Some((key, value)) = parse_kv(trimmed) {
                    match key {
                        "enabled" => {
                            config.cleanup.enabled = value == "true";
                        }
                        "interval" => {
                            if let Ok(v) = value.parse::<u64>() {
                                config.cleanup.interval = Duration::from_secs(v);
                            }
                        }
                        "retention" => {
                            if let Ok(v) = value.parse::<u64>() {
                                config.cleanup.retention = Duration::from_secs(v);
                            }
                        }
                        _ => {}
                    }
                }
            } else if current_section == "engagement" {
                if let Some((key, value)) = parse_kv(trimmed) {
                    match key {
                        "burst_window" => {
                            if let Ok(v) = value.parse::<u64>() {
                                config.engagement.burst_window = Duration::from_secs(v);
                            }
                        }
                        "burst_threshold" => {
                            if let Ok(v) = value.parse::<usize>() {
                                config.engagement.burst_threshold = v;
                            }
                        }
                        "step_multiplier" => {
                            if let Ok(v) = value.parse::<f64>() {
                                config.engagement.step_multiplier = v;
                            }
                        }
                        "absolute_refractory" => {
                            if let Ok(v) = value.parse::<u64>() {
                                config.engagement.absolute_refractory = Duration::from_secs(v);
                            }
                        }
                        "decay_per_minute" => {
                            if let Ok(v) = value.parse::<f64>() {
                                config.engagement.decay_per_minute = v;
                            }
                        }
                        "peer_activity_window" => {
                            if let Ok(v) = value.parse::<u64>() {
                                config.engagement.peer_activity_window = Duration::from_secs(v);
                            }
                        }
                        _ => {}
                    }
                }
            } else if current_section == "sensors" {
                let sensor_line = trimmed.trim_end_matches(':');
                if let Some(stripped) = sensor_line.strip_prefix('-') {
                    // Remove sensor: -processes
                    let name = stripped.trim();
                    if let Some(s) = config.sensors.get_mut(name) {
                        s.enabled = false;
                        eprintln!("[attend] config: disabled sensor '{}'", name);
                    }
                    current_sensor.clear();
                } else if let Some(stripped) = sensor_line.strip_prefix('+') {
                    // Add script sensor: +disk-pressure
                    let name = stripped.trim().to_string();
                    config.sensors.entry(name.clone()).or_insert(SensorConfig {
                        enabled: true,
                        interval: Duration::from_secs(60),
                        min_interval: Duration::from_secs(15),
                        threshold: 2.0,
                        decay_threshold: 4,
                        script: None,
                        requires: Vec::new(),
                    });
                    current_sensor = name;
                } else {
                    // Existing sensor override
                    current_sensor = sensor_line.trim().to_string();
                }
            }
            continue;
        }

        // Third-level: sensor properties
        if indent == 4 && !current_sensor.is_empty() {
            if let Some((key, value)) = parse_kv(trimmed) {
                if let Some(sensor) = config.sensors.get_mut(&current_sensor) {
                    match key {
                        "interval" => {
                            if let Ok(v) = value.parse::<u64>() {
                                sensor.interval = Duration::from_secs(v);
                            }
                        }
                        "min_interval" => {
                            if let Ok(v) = value.parse::<u64>() {
                                sensor.min_interval = Duration::from_secs(v);
                            }
                        }
                        "threshold" => {
                            if let Ok(v) = value.parse::<f64>() {
                                sensor.threshold = v;
                            }
                        }
                        "decay_threshold" => {
                            if let Ok(v) = value.parse::<u32>() {
                                sensor.decay_threshold = v;
                            }
                        }
                        "script" => {
                            sensor.script = Some(expand_path(value));
                        }
                        "enabled" => {
                            sensor.enabled = value == "true";
                        }
                        "requires" => {
                            // Inline array: requires: [Bash(gh:*), Read]
                            if value.starts_with('[') && value.ends_with(']') {
                                let inner = &value[1..value.len() - 1];
                                sensor.requires = inner
                                    .split(',')
                                    .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
                                    .filter(|s| !s.is_empty())
                                    .collect();
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

/// Expand `$HOME`, `~`, `$XDG_CONFIG_HOME`, `$XDG_DATA_HOME`, and
/// `$XDG_STATE_HOME` inside a config value. Keeps script paths portable
/// across installs without depending on the caller's cwd.
fn expand_path(value: &str) -> String {
    let mut out = value.to_string();
    if let Ok(home) = std::env::var("HOME") {
        if out.starts_with("~/") {
            out = format!("{home}{}", &out[1..]);
        }
        out = out.replace("$HOME", &home);
    }
    for var in &["XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME"] {
        if let Ok(val) = std::env::var(var) {
            out = out.replace(&format!("${var}"), &val);
        }
    }
    out
}

/// Parse "key: value" from a trimmed line.
fn parse_kv(line: &str) -> Option<(&str, &str)> {
    let colon = line.find(':')?;
    let key = line[..colon].trim();
    let value = line[colon + 1..].trim();
    if key.is_empty() || value.is_empty() {
        return None;
    }
    Some((key, value))
}
