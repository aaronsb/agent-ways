//! attend's settings (ADR-503 §13): the schema in a library crate, so ways
//! composes it into `ways settings` without depending on the attend binary,
//! and the typed configuration attend runs on, loaded through it.
//!
//! The files and their paths are attend's own: `~/.config/attend/config.yaml`
//! (under `$XDG_CONFIG_HOME` when set) and a project's `.claude/attend.yaml`,
//! layered on top. `agent-settings` parses and checks them, and every write
//! goes through its writer: locked, atomic, and changing only the keys it
//! sets, so comments and order stay.
//!
//! Each sensor is a file of its own (ADR-403); [`sensors`] finds and layers
//! them.

mod config;
pub mod schema;
pub mod sensors;
pub mod theme;

pub use config::{expand_path, ChatConfig, CleanupConfig, Config, EngagementConfig, GovernorConfig};
pub use schema::{FILE, SCHEMA};

use agent_settings::schema::LayerScope;
use agent_settings::writer::{self, WriteError};
use agent_settings::Layer;
use serde_yaml::Value;
use std::path::{Path, PathBuf};

/// The first line of an attend file a write creates, from `ways settings`
/// or `attend config tune --apply` alike.
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
/// See [`update`].
pub fn write(path: &Path, values: &[(&str, Value)]) -> Result<bool, String> {
    write_within(path, None, values)
}

/// [`write`], waiting at most `wait` for another writer's lock when given:
/// a writer on a screen's own thread must not hang on a holder that does.
/// A lock not had in time writes nothing and says so.
pub fn write_within(path: &Path, wait: Option<std::time::Duration>, values: &[(&str, Value)]) -> Result<bool, String> {
    update_within(path, wait, |_| values.iter().map(|(n, v)| (n.to_string(), v.clone())).collect())
}

/// Read the attend file at `path` and write what `f` makes of it, in one
/// locked, atomic edit that changes only the keys `f` names. `f` sees the
/// file as it is under the lock, read as a user file over the defaults, so a
/// value derived from another key never comes from a stale read. A missing
/// or empty file starts with [`HEADER`]. Each value is checked against the
/// schema; a value it refuses, or a key it does not name, writes nothing.
/// Returns whether the file changed.
pub fn update(path: &Path, f: impl FnOnce(&Config) -> Vec<(String, Value)>) -> Result<bool, String> {
    update_within(path, None, f)
}

/// [`update`], waiting at most `wait` for the lock when given.
pub fn update_within(path: &Path, wait: Option<std::time::Duration>, f: impl FnOnce(&Config) -> Vec<(String, Value)>) -> Result<bool, String> {
    let (outcome, changed) = writer::edit_file_within(path, Some(HEADER), wait, |d| {
        let held = Layer::from_text(&SCHEMA, "user", FILE, LayerScope::User, Some(path), &d.text());
        for (name, v) in f(&Config::from_layers(&[held])) {
            let Some(spec) = SCHEMA.keys.iter().find(|k| k.name == name && !k.is_pattern()) else {
                return Ok(Err(format!("{name} is not an attend key; nothing written")));
            };
            if let Err(m) = spec.check_value(&v) {
                return Ok(Err(format!("{name}: {m}; nothing written")));
            }
            d.set(&spec.path.iter().map(|s| s.to_string()).collect::<Vec<_>>(), &v)?;
        }
        Ok(Ok(()))
    })
    .map_err(|e| format!("{e}; nothing written"))?;
    outcome.map(|()| changed)
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
# Run `attend config tune` to derive these from real session history.
engagement:
  burst_threshold: 3         # disclosures before refractory kicks in
  step_multiplier: 1.25      # per-burst threshold elevation
  absolute_refractory: 60    # seconds of complete suppression after burst
  decay_per_minute: 0.1      # relative refractory decay rate
  peer_activity_window: 900  # per-peer engagement window

# Background signal-file cleanup inside `attend run`. Reaping is by project
# liveness (ADR-136), not by age. `attend message cleanup` runs it by hand.
cleanup:
  enabled: true
  interval: 600              # seconds between sweeps (10 min)

# Projects whose own command and mcp sensor files run (ADR-403). Each
# sensor is a file under attend/sensors/; `attend sensor list` shows them.
# project_sensors:
#   - ~/Projects/example
"#
    .to_string()
}

#[cfg(test)]
mod tests;
