// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! A sensor's store file: the record it holds, where the stores live, and
//! reading, writing, sweeping and moving them. The module doc of
//! [`super`] describes the store.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use sensor_spec::bounds;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{fresh_nonce, state, Key, Memo, STORE_FORMAT};

/// The declared state as the store keeps it, with the fingerprint of the
/// declaration that wrote it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoredState {
    pub decl: String,
    pub fields: state::Fields,
}

/// One sensor's store file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Record {
    /// The store format. A file without it does not read as a record.
    pub store: u32,
    pub sensor: String,
    pub project: String,
    #[serde(default)]
    pub session: Option<String>,
    #[serde(default)]
    pub poll: Option<Poll>,
    /// What each trigger with `memory: store` last saw, by trigger name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub memory: BTreeMap<String, Memo>,
    /// The declared state, for a sensor that declares one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<StoredState>,
    /// A value new with each write, so a reader can tell two writes apart
    /// when the file's mtime cannot.
    #[serde(default)]
    pub nonce: u64,
}

impl Record {
    pub fn new(key: &Key, poll: Option<Poll>, memory: BTreeMap<String, Memo>) -> Record {
        Record { store: STORE_FORMAT, sensor: key.sensor.clone(), project: key.project.clone(), session: key.session.clone(), poll, memory, state: None, nonce: fresh_nonce() }
    }

    fn key(&self) -> Key {
        Key::new(&self.sensor, &self.project, self.session.as_deref())
    }
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

/// A store file of this format, whatever its key.
pub(super) fn parse(path: &Path) -> Option<Record> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<Record>(&text).ok().filter(|r| r.store == STORE_FORMAT)
}

/// The store at `path`, when it is of this format and its key is `key`.
pub fn read_at(path: &Path, key: &Key) -> Option<Record> {
    parse(path).filter(|r| r.key() == *key)
}

/// Every store of this format under `dir`.
pub(super) fn records_in(dir: &Path) -> impl Iterator<Item = (PathBuf, Record)> {
    let entries = std::fs::read_dir(dir).into_iter().flatten().flatten();
    entries.map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).filter_map(|p| parse(&p).map(|r| (p, r)))
}

/// Remove every file of the store directory not written within the
/// retention limit, and every `.json` file of an older format ([`outdated`]),
/// such as one written before stores were keyed by session. A store of a
/// newer format, which a newer attend on this machine writes, stays.
/// Returns how many it removed.
pub fn sweep() -> usize {
    sweep_in(&store_dir(), SystemTime::now())
}

pub(super) fn sweep_in(dir: &Path, now: SystemTime) -> usize {
    let limit = Duration::from_secs(bounds::RETENTION_DAYS * 86_400);
    let mut removed = 0;
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let expired = meta.modified().ok().and_then(|m| now.duration_since(m).ok()).is_some_and(|age| age > limit);
        let older = path.extension().is_some_and(|x| x == "json") && outdated(&path);
        if (expired || older) && std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Whether the file at `path` is a store of an older format: it is not
/// valid JSON, or it is an object whose `store` is missing or below
/// [`STORE_FORMAT`]. A file that cannot be read, a newer format and any
/// other JSON value count as not outdated.
pub(super) fn outdated(path: &Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else { return false };
    match serde_json::from_slice::<Value>(&bytes) {
        Err(_) => true,
        Ok(Value::Object(m)) => m.get("store").and_then(Value::as_u64).is_none_or(|n| n < u64::from(STORE_FORMAT)),
        Ok(_) => false,
    }
}

/// The last poll of each sensor in `project`, by sensor name: the
/// session's own store when a session is given, else the newest poll of
/// any session's store in the project.
pub fn last_polls(project: &str, session: Option<&str>) -> BTreeMap<String, Poll> {
    last_polls_in(&store_dir(), project, session)
}

pub(super) fn last_polls_in(dir: &Path, project: &str, session: Option<&str>) -> BTreeMap<String, Poll> {
    let mut out: BTreeMap<String, Poll> = BTreeMap::new();
    let session = session.filter(|s| !s.is_empty());
    for (_, r) in records_in(dir).filter(|(_, r)| r.project == project && (session.is_none() || r.session.as_deref() == session)) {
        let Some(poll) = r.poll else { continue };
        if out.get(&r.sensor).is_none_or(|p| p.at < poll.at) {
            out.insert(r.sensor, poll);
        }
    }
    out
}

/// Move every store of the session `from` to the session `to`, as a
/// session's move after `/clear` carries its state, and return how many
/// moved. A store `to` already has is kept. `from`'s store is removed once
/// its copy is written, or when `to` already has one; a failed write leaves
/// it in place.
pub fn move_stores(from: &str, to: &str) -> usize {
    move_stores_in(&store_dir(), from, to)
}

pub(super) fn move_stores_in(dir: &Path, from: &str, to: &str) -> usize {
    if from.is_empty() || to.is_empty() || from == to {
        return 0;
    }
    let mut moved = 0;
    for (path, r) in records_in(dir).filter(|(_, r)| r.session.as_deref() == Some(from)) {
        let key = Key::new(&r.sensor, &r.project, Some(to));
        let target = key.path_in(dir);
        let written = read_at(&target, &key).is_none() && write(&target, &Record { session: key.session.clone(), ..r });
        if written || read_at(&target, &key).is_some() {
            std::fs::remove_file(&path).ok();
        }
        moved += usize::from(written);
    }
    moved
}

/// Write `rec` to `path` whole: a temporary file renamed over it. Returns
/// whether the file now holds it.
pub(super) fn write(path: &Path, rec: &Record) -> bool {
    let Some(dir) = path.parent() else { return false };
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let tmp = path.with_extension(format!("json.{}", std::process::id()));
    let Ok(text) = serde_json::to_string(rec) else { return false };
    if std::fs::write(&tmp, text).is_err() {
        return false;
    }
    if std::fs::rename(&tmp, path).is_err() {
        std::fs::remove_file(&tmp).ok();
        return false;
    }
    true
}
