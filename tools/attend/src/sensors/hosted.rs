//! A sensor as its file configures it (ADR-403 §1): the compiled poll under
//! the file's name, cadence and threshold, with each poll's outcome
//! recorded where `attend sensor list` reads it.
//!
//! The record is the sensor's store file under
//! `$XDG_STATE_HOME/agent-ways/attend/sensor-state/`, named by a hash of the
//! sensor's name and the project, so no part of the path comes from a file
//! (§3b). It holds the last poll's outcome; the declared state and trigger
//! memory join it in the same file when command and mcp sensors run.

use std::path::{Path, PathBuf};
use std::time::Duration;

use sensor_spec::Cadence;
use sensor_trait::{Focus, Sensor};
use serde::{Deserialize, Serialize};

/// A compiled sensor under its file's settings.
pub struct FileSensor {
    name: String,
    cadence: Cadence,
    inner: Box<dyn Sensor>,
    /// Where each poll's outcome is written; none for a test poll.
    record: Option<PathBuf>,
    project: String,
}

impl FileSensor {
    pub fn new(name: String, cadence: Cadence, inner: Box<dyn Sensor>, project: &str, record: bool) -> Self {
        let path = record.then(|| store_path(&name, project));
        FileSensor { name, cadence, inner, record: path, project: project.to_string() }
    }
}

impl Sensor for FileSensor {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn source(&self) -> String {
        self.inner.source()
    }

    fn poll(&mut self, focus: &Focus) -> Vec<(f64, String)> {
        let out = self.inner.poll(focus);
        let failure = self.inner.take_failure();
        if let Some(path) = &self.record {
            let rec = Record {
                sensor: self.name.clone(),
                project: self.project.clone(),
                poll: Some(Poll { at: sensor_trait::epoch_secs(), ok: failure.is_none(), detail: failure, observations: out.len() }),
            };
            write(path, &rec);
        }
        out
    }

    fn emission_threshold(&self) -> f64 {
        self.cadence.threshold
    }

    fn base_interval(&self) -> Duration {
        Duration::from_secs(self.cadence.interval)
    }

    fn min_interval(&self) -> Duration {
        Duration::from_secs(self.cadence.min_interval)
    }

    fn decay_threshold(&self) -> u32 {
        self.cadence.decay_threshold
    }

    fn export_state(&self) -> Vec<(String, String)> {
        self.inner.export_state()
    }

    fn import_state(&mut self, state: &[(String, String)]) {
        self.inner.import_state(state)
    }

    fn take_failure(&mut self) -> Option<String> {
        None
    }
}

/// One sensor's store file.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Record {
    pub sensor: String,
    pub project: String,
    #[serde(default)]
    pub poll: Option<Poll>,
}

/// The last poll's outcome.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Poll {
    /// Seconds since the epoch.
    pub at: u64,
    pub ok: bool,
    /// Why it failed.
    #[serde(default)]
    pub detail: Option<String>,
    /// How many observations it returned.
    #[serde(default)]
    pub observations: usize,
}

/// The directory of every sensor's store.
pub fn store_dir() -> PathBuf {
    ways_core::paths::state_root().join("attend").join("sensor-state")
}

/// The store of sensor `name` in `project`: FNV-1a over both, so the name
/// is stable across builds and carries nothing a file wrote.
pub fn store_path(name: &str, project: &str) -> PathBuf {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.bytes().chain([0]).chain(project.bytes()) {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    store_dir().join(format!("{h:016x}.json"))
}

/// The store of `name` in `project`, if one was written.
pub fn read(name: &str, project: &str) -> Option<Record> {
    let text = std::fs::read_to_string(store_path(name, project)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Write `rec` to `path` whole: a temporary file renamed over it.
fn write(path: &Path, rec: &Record) {
    let Some(dir) = path.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let tmp = path.with_extension(format!("json.{}", std::process::id()));
    let Ok(text) = serde_json::to_string(rec) else { return };
    if std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, path).is_err() {
        std::fs::remove_file(&tmp).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_store_is_named_by_sensor_and_project_alone() {
        let a = store_path("git", "/p");
        assert_eq!(a, store_path("git", "/p"));
        assert_ne!(a, store_path("git", "/q"));
        assert_ne!(a, store_path("gi", "t/p"), "the separator keeps the two apart");
        let stem = a.file_stem().unwrap().to_str().unwrap();
        assert!(stem.len() == 16 && stem.chars().all(|c| c.is_ascii_hexdigit()), "{stem}");
    }
}
