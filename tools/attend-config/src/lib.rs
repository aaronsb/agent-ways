//! attend's settings (ADR-503 §13): the schema in a library crate, so ways
//! composes it into `ways settings` without depending on the attend binary,
//! and the typed configuration attend runs on, loaded through it.
//!
//! The files and their paths are attend's own: `~/.config/attend/config.yaml`
//! (under `$XDG_CONFIG_HOME` when set) and a project's `.claude/attend.yaml`,
//! layered on top. `agent-settings` parses and checks them, and every write
//! goes through its writer: locked, atomic, and changing only the keys it
//! sets, so comments and order stay.

mod config;
pub mod schema;
pub mod theme;

pub use config::{expand_path, CleanupConfig, Config, EngagementConfig, GovernorConfig, SensorConfig};
pub use schema::{BUILTINS, FILE, SCHEMA};

use agent_settings::schema::LayerScope;
use agent_settings::writer::{self, WriteError};
use agent_settings::Layer;
use serde_yaml::Value;
use std::path::{Path, PathBuf};

/// The first line of an attend file a write creates, from `ways settings`
/// or `attend tune --apply` alike.
pub const HEADER: &str = "# attend settings; `ways settings help attend` describes each key\n";

/// `$XDG_CONFIG_HOME/attend/config.yaml`, or `~/.config/attend/config.yaml`.
/// An empty or relative `XDG_CONFIG_HOME` is ignored, as the XDG spec says.
pub fn user_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| claude_sessions::home_dir().join(".config"));
    base.join("attend").join("config.yaml")
}

/// A project's overlay: `<dir>/.claude/attend.yaml`.
pub fn project_path(dir: &Path) -> PathBuf {
    dir.join(".claude").join("attend.yaml")
}

/// The layers attend's keys resolve through, lowest first: the user file,
/// then the project's at `dir`.
pub fn layers(dir: &Path) -> Vec<Layer> {
    vec![user_layer(), Layer::read(&SCHEMA, "project", FILE, LayerScope::Project, &project_path(dir))]
}

/// The user file's layer alone.
pub fn user_layer() -> Layer {
    Layer::read(&SCHEMA, "user", FILE, LayerScope::User, &user_path())
}

/// Create `path` with [`default_yaml`] when no file is there. Returns false,
/// writing nothing, when one exists.
pub fn init(path: &Path) -> Result<bool, WriteError> {
    writer::create_new(path, &default_yaml())
}

/// Write `values`, each a key's dotted name and its value, into the attend
/// file at `path` in one locked, atomic edit that changes only those keys.
/// A missing or empty file starts with [`HEADER`]. Each value is
/// checked against the schema first; a value it refuses writes nothing.
/// Returns whether the file changed.
pub fn write(path: &Path, values: &[(&str, Value)]) -> Result<bool, String> {
    let mut edits = Vec::new();
    for (name, v) in values {
        let spec = SCHEMA.keys.iter().find(|k| k.name == *name && !k.is_pattern()).ok_or_else(|| format!("{name} is not an attend key"))?;
        spec.check_value(v).map_err(|m| format!("{name}: {m}; nothing written"))?;
        edits.push((spec.path.iter().map(|s| s.to_string()).collect::<Vec<_>>(), v.clone()));
    }
    writer::edit_file(path, Some(HEADER), |d| {
        for (k, v) in &edits {
            d.set(k, v)?;
        }
        Ok(())
    })
    .map(|((), changed)| changed)
    .map_err(|e| format!("{e}; nothing written"))
}

/// The file `attend config init` writes: every section at its defaults,
/// commented. It lints clean against the schema.
pub fn default_yaml() -> String {
    r#"# attend configuration
# User scope: ~/.config/attend/config.yaml
# Project scope: {project}/.claude/attend.yaml (layered on top)
# `ways settings help attend` describes every key; `ways settings` edits them.

governor:
  base_cooldown: 15
  max_per_window: 3
  rate_window: 120

# Action potential engagement model (ADR-123).
# Run `attend tune` to derive these from real session history.
engagement:
  burst_threshold: 3         # disclosures before refractory kicks in
  step_multiplier: 1.25      # per-burst threshold elevation
  absolute_refractory: 60    # seconds of complete suppression after burst
  decay_per_minute: 0.1      # relative refractory decay rate
  peer_activity_window: 900  # per-peer engagement window

# Background signal-file cleanup inside `attend run`. Reaping is by project
# liveness (ADR-136), not by age. `attend cleanup` runs it by hand.
cleanup:
  enabled: true
  interval: 600              # seconds between sweeps (10 min)

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

  # Example sensor of your own, switched off. A sensor with a `script` is
  # one of your own.
  #
  # To enable it:
  #   1. Copy the example script from the agent-ways repo to a path where
  #      you keep trusted user scripts. The XDG convention:
  #        mkdir -p $XDG_DATA_HOME/attend/sensors
  #        cp tools/attend/examples/xdg-downloads.sh \
  #           $XDG_DATA_HOME/attend/sensors/
  #   2. Read the script. A sensor runs arbitrary shell as you.
  #   3. Set `enabled: true` below.
  #
  # A script can live anywhere: under $XDG_DATA_HOME, in a project's
  # .claude/sensors/, or at any absolute path. attend needs only that the
  # path resolves and the script keeps the subprocess contract in
  # docs/attend-and-monitor/authoring-sensors.md.
  #
  # This example watches XDG Downloads for new files: one new file fires
  # at 2.0, a batch at 3.0.
  xdg-downloads:
    script: $XDG_DATA_HOME/attend/sensors/xdg-downloads.sh
    enabled: false
    interval: 120
    min_interval: 30
    threshold: 2.0
    decay_threshold: 3

  # A second shipped example, gh-notifications, emits one line per new
  # GitHub notification, its magnitude tiered by reason. It needs gh and
  # jq on PATH and a gh login. Watching a PR's CI is a Monitor concern,
  # not an ambient sensor (ADR-137).
  #gh-notifications:
  #  script: $XDG_DATA_HOME/attend/sensors/gh-notifications.sh
  #  enabled: false
  #  interval: 180
  #  min_interval: 60
  #  threshold: 2.0

# Project-scope example (in .claude/attend.yaml):
#
# sensors:
#   disk-pressure:
#     script: .claude/sensors/check-disk.sh
#     interval: 120
#     threshold: 3.0
#   processes:
#     enabled: false
"#
    .to_string()
}

#[cfg(test)]
mod tests;
