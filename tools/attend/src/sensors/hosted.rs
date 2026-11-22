// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! A sensor as its file configures it: the compiled poll under the file's
//! name, cadence and threshold, with each poll's outcome and each trigger's
//! memory recorded in the sensor's store.
//!
//! A store is one file under `$XDG_STATE_HOME/agent-ways/attend/sensor-state/`
//! for each sensor, project and session. Its name is an FNV-1a hash of the
//! three, so no part of the path comes from a sensor file. Each report goes
//! to one session, so what a sensor last saw is what that session was told,
//! and two sessions in one project keep separate stores. A sensor keeps a
//! store only for a resolved session id, so each store has one writer: a
//! caller whose session did not resolve, and a test poll, keep none.
//!
//! The file carries its format. A file of another format, such as one
//! written before stores were keyed by session, or one whose sensor,
//! project and session are not the key asked for, reads as no store, and
//! the sensor starts from a baseline. When `attend run` registers its
//! sensors, [`sweep`] removes the files of an older format and every store
//! past the retention limit. It leaves a newer format alone, so a newer
//! attend on the same machine keeps its stores.
//!
//! A trigger's memory is kept with a fingerprint of what the trigger tests,
//! taken from the trigger's fields as the file states them. Memory under
//! another fingerprint belongs to an earlier definition and reads as none.
//! Memory a trigger keeps for the session (`memory: session`) lives in the
//! process and travels in the session's checkpoint through
//! [`Sensor::export_state`], so a re-executed run keeps it and a trigger
//! definition fires at most once in the session.
//!
//! References:
//! - ADR-403 §1 and §3b: the store, where it lives, its retention, and the
//!   fresh baseline a discarded store or changed declaration gives.
//! - ADR-416 items 6 and 8: `memory: session` firing at most once per
//!   session, and stores keyed by session and carried by a session's move.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use sensor_spec::{bounds, Cadence, Memory, On, Test, Trigger};
use sensor_trait::{Focus, Sensor};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The store format this build reads and writes.
pub const STORE_FORMAT: u32 = 2;

/// The `export_state` key that carries a sensor's session memory.
pub const SESSION_MEMORY: &str = "session_memory";

/// What one trigger saw on one poll: each item's key, `$` for a trigger
/// with no key, mapped to the value it compares. For `enters` and `leaves`
/// that is whether `where` held, as a boolean; for `changed` it is the
/// item; for `added` and `removed` any value.
pub type Seen = BTreeMap<String, Value>;

/// What a trigger last saw, under the fingerprint of the definition that
/// saw it ([`Trigger::fingerprint`]).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Memo {
    pub test: String,
    pub seen: Seen,
}

/// The memory of a sensor's triggers with `memory: session`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
struct SessionMemory {
    /// What each trigger last saw, by trigger name.
    #[serde(default)]
    held: BTreeMap<String, Memo>,
    /// The fingerprints of the trigger definitions that have fired. Each
    /// fires at most once in the session, under whatever name it carries.
    #[serde(default)]
    spent: BTreeSet<String>,
}

/// Whose store a file is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    pub sensor: String,
    pub project: String,
    /// The session the reports go to; none for a caller without an id.
    pub session: Option<String>,
}

impl Key {
    /// The key of `sensor` in `project` for `session`. An empty id is no
    /// session.
    pub fn new(sensor: &str, project: &str, session: Option<&str>) -> Key {
        Key { sensor: sensor.to_string(), project: project.to_string(), session: session.filter(|s| !s.is_empty()).map(str::to_string) }
    }

    /// The store's path under `dir`: FNV-1a over the sensor, the project
    /// and the session, each followed by a NUL, so the name is stable
    /// across builds and differs from every name of the format keyed by
    /// sensor and project alone.
    pub fn path_in(&self, dir: &Path) -> PathBuf {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let parts = [self.sensor.as_str(), self.project.as_str(), self.session.as_deref().unwrap_or_default()];
        for b in parts.iter().flat_map(|p| p.bytes().chain([0])) {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        dir.join(format!("{h:016x}.json"))
    }
}

/// A compiled sensor under its file's settings.
pub struct FileSensor {
    name: String,
    cadence: Cadence,
    inner: Box<dyn Sensor>,
    key: Key,
    /// Where each poll's outcome and the store memory are written; none
    /// without a session id, and for a test poll.
    store: Option<PathBuf>,
    /// The last poll's outcome.
    last: Option<Poll>,
    /// What each trigger with `memory: store` last saw, as the store holds it.
    stored: BTreeMap<String, Memo>,
    session: SessionMemory,
    /// The last poll's failure, for a caller that reads it.
    failure: Option<String>,
}

impl FileSensor {
    /// The sensor `name` of `project` for `session`. With `record` and a
    /// session id, it reads and writes its store; otherwise it keeps none.
    pub fn new(name: String, cadence: Cadence, inner: Box<dyn Sensor>, project: &str, session: Option<&str>, record: bool) -> Self {
        let key = Key::new(&name, project, session);
        let store = (record && key.session.is_some()).then(|| key.path_in(&store_dir()));
        FileSensor::open(key, cadence, inner, store)
    }

    /// The sensor under `key`, its store at `store`, starting from what the
    /// store holds for that key.
    fn open(key: Key, cadence: Cadence, inner: Box<dyn Sensor>, store: Option<PathBuf>) -> Self {
        let rec = store.as_deref().and_then(|p| read_at(p, &key));
        let (last, stored) = rec.map(|r| (r.poll, r.memory)).unwrap_or_default();
        FileSensor { name: key.sensor.clone(), cadence, inner, key, store, last, stored, session: SessionMemory::default(), failure: None }
    }

    /// Record what the item trigger `trigger` sees on this poll, and return
    /// the keys whose change fires it; none for a state trigger, which is
    /// not evaluated here. Memory kept under another definition of the
    /// trigger counts as none.
    ///
    /// Store memory is written to the store at once, and a fresh store's
    /// first poll sets a baseline. Session memory starts from nothing seen,
    /// and a trigger definition with it fires at most once in the session,
    /// whatever it compares on: once it has fired, a later change, such as
    /// a condition that leaves and enters again, fires nothing. The mark is
    /// kept by fingerprint, so a renamed trigger stays spent and an edited
    /// one fires once more.
    // Nothing in attend runs triggers yet. The caller is the sensor host
    // that runs command and mcp triggers.
    #[allow(dead_code)]
    pub fn observe(&mut self, trigger: &Trigger, now: Seen) -> Option<Vec<String>> {
        let Test::Items { on, .. } = &trigger.test else { return None };
        let test = trigger.fingerprint();
        let mem = match trigger.memory {
            Memory::Store => &mut self.stored,
            Memory::Session => &mut self.session.held,
        };
        let prev = mem.get(&trigger.name).filter(|m| m.test == test).map(|m| &m.seen);
        let mut out = fired(*on, trigger.memory, prev, &now);
        mem.insert(trigger.name.clone(), Memo { test: test.clone(), seen: now });
        match trigger.memory {
            Memory::Store => self.save(),
            Memory::Session if !out.is_empty() && !self.session.spent.insert(test) => out.clear(),
            Memory::Session => {}
        }
        Some(out)
    }

    fn save(&self) {
        if let Some(path) = &self.store {
            write(path, &Record::new(&self.key, self.last.clone(), self.stored.clone()));
        }
    }
}

/// The keys whose change fires a trigger that compares on `on`, between
/// what it saw before (`prev`) and what it sees now. With nothing seen
/// before, store memory takes `now` as its baseline and fires nothing, and
/// session memory compares against nothing seen, so `enters` fires for
/// each item that meets `where` and `added` for each item.
fn fired(on: On, memory: Memory, prev: Option<&Seen>, now: &Seen) -> Vec<String> {
    let empty = Seen::new();
    let prev = match (prev, memory) {
        (Some(p), _) => p,
        (None, Memory::Store) => return Vec::new(),
        (None, Memory::Session) => &empty,
    };
    let held = |s: &Seen, k: &str| s.get(k) == Some(&Value::Bool(true));
    let keys = |s: &Seen, keep: &dyn Fn(&str) -> bool| s.keys().filter(|k| keep(k)).cloned().collect();
    match on {
        On::Added => keys(now, &|k| !prev.contains_key(k)),
        On::Removed => keys(prev, &|k| !now.contains_key(k)),
        On::Changed => keys(now, &|k| prev.get(k).is_some_and(|p| Some(p) != now.get(k))),
        On::Enters => keys(now, &|k| held(now, k) && !held(prev, k)),
        On::Leaves => keys(prev, &|k| held(prev, k) && !held(now, k)),
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
        self.failure = self.inner.take_failure();
        self.last = Some(Poll { at: sensor_trait::epoch_secs(), ok: self.failure.is_none(), detail: self.failure.clone(), observations: out.len() });
        self.save();
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

    /// The compiled sensor's state, and the session memory on one line
    /// under [`SESSION_MEMORY`] when there is any.
    fn export_state(&self) -> Vec<(String, String)> {
        let mut out = self.inner.export_state();
        if self.session != SessionMemory::default() {
            if let Ok(line) = serde_json::to_string(&self.session) {
                out.push((SESSION_MEMORY.to_string(), line));
            }
        }
        out
    }

    /// Take the session memory from its key, and hand the rest to the
    /// compiled sensor.
    fn import_state(&mut self, state: &[(String, String)]) {
        let (memo, rest): (Vec<_>, Vec<_>) = state.iter().cloned().partition(|(k, _)| k == SESSION_MEMORY);
        if let Some(m) = memo.last().and_then(|(_, v)| serde_json::from_str(v).ok()) {
            self.session = m;
        }
        if !rest.is_empty() {
            self.inner.import_state(&rest);
        }
    }

    fn take_failure(&mut self) -> Option<String> {
        self.failure.take()
    }
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
}

impl Record {
    pub fn new(key: &Key, poll: Option<Poll>, memory: BTreeMap<String, Memo>) -> Record {
        Record { store: STORE_FORMAT, sensor: key.sensor.clone(), project: key.project.clone(), session: key.session.clone(), poll, memory }
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
fn parse(path: &Path) -> Option<Record> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<Record>(&text).ok().filter(|r| r.store == STORE_FORMAT)
}

/// The store at `path`, when it is of this format and its key is `key`.
pub fn read_at(path: &Path, key: &Key) -> Option<Record> {
    parse(path).filter(|r| r.key() == *key)
}

/// Every store of this format under `dir`.
fn records_in(dir: &Path) -> impl Iterator<Item = (PathBuf, Record)> {
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

fn sweep_in(dir: &Path, now: SystemTime) -> usize {
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
fn outdated(path: &Path) -> bool {
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

fn last_polls_in(dir: &Path, project: &str, session: Option<&str>) -> BTreeMap<String, Poll> {
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

fn move_stores_in(dir: &Path, from: &str, to: &str) -> usize {
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
fn write(path: &Path, rec: &Record) -> bool {
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

#[cfg(test)]
mod tests;
