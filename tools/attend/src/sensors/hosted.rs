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
//! and two sessions in one project keep separate stores. A caller with no
//! session id, such as `attend run` whose session did not resolve, keys its
//! stores by sensor and project with an empty session, which every such
//! caller in the project shares. A test poll keeps no store.
//!
//! The file carries its format. A file of another format, such as one
//! written before stores were keyed by session, or one whose sensor,
//! project and session are not the key asked for, reads as no store: the
//! sensor starts from a baseline, and the old file is left as it is.
//!
//! References:
//! - ADR-403 §1 and §3b: the store, where it lives, and the baseline a
//!   fresh store sets.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sensor_spec::{Cadence, Memory, On};
use sensor_trait::{Focus, Sensor};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The store format this build reads and writes.
pub const STORE_FORMAT: u32 = 2;

/// What one trigger saw on one poll: each item's key, `$` for a trigger
/// with no key, mapped to the value it compares. For `enters` and `leaves`
/// that is whether `where` held, as a boolean; for `changed` it is the
/// item; for `added` and `removed` any value.
pub type Seen = BTreeMap<String, Value>;

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

    /// The store's path under the store directory.
    pub fn path(&self) -> PathBuf {
        self.path_in(&store_dir())
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
    /// Where each poll's outcome and the store memory are written; none for
    /// a test poll.
    store: Option<PathBuf>,
    /// The last poll's outcome.
    last: Option<Poll>,
    /// What each trigger with `memory: store` last saw, as the store holds it.
    stored: BTreeMap<String, Seen>,
    /// What each trigger with `memory: session` last saw. It lives as long
    /// as this process.
    held: BTreeMap<String, Seen>,
    /// The last poll's failure, for a caller that reads it.
    failure: Option<String>,
}

impl FileSensor {
    /// The sensor `name` of `project` for `session`. With `record`, it
    /// reads and writes its store; without, it is a test poll and keeps
    /// none.
    pub fn new(name: String, cadence: Cadence, inner: Box<dyn Sensor>, project: &str, session: Option<&str>, record: bool) -> Self {
        let key = Key::new(&name, project, session);
        let store = record.then(|| key.path());
        FileSensor::open(key, cadence, inner, store)
    }

    /// The sensor under `key`, its store at `store`, starting from what the
    /// store holds for that key.
    fn open(key: Key, cadence: Cadence, inner: Box<dyn Sensor>, store: Option<PathBuf>) -> Self {
        let rec = store.as_deref().and_then(|p| read_at(p, &key));
        let (last, stored) = rec.map(|r| (r.poll, r.memory)).unwrap_or_default();
        FileSensor { name: key.sensor.clone(), cadence, inner, key, store, last, stored, held: BTreeMap::new(), failure: None }
    }

    /// Record what the trigger `trigger`, comparing on `on`, sees on this
    /// poll, and return the keys whose change fires it. Store memory is
    /// written to the store at once; session memory stays in this process.
    // Nothing in attend runs triggers yet. The caller is the sensor host
    // that runs command and mcp triggers.
    #[allow(dead_code)]
    pub fn observe(&mut self, trigger: &str, on: On, memory: Memory, now: Seen) -> Vec<String> {
        let mem = match memory {
            Memory::Store => &mut self.stored,
            Memory::Session => &mut self.held,
        };
        let out = fired(on, memory, mem.get(trigger), &now);
        mem.insert(trigger.to_string(), now);
        if memory == Memory::Store {
            self.save();
        }
        out
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
// Its one caller outside the tests is `FileSensor::observe`.
#[allow(dead_code)]
pub fn fired(on: On, memory: Memory, prev: Option<&Seen>, now: &Seen) -> Vec<String> {
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

    fn export_state(&self) -> Vec<(String, String)> {
        self.inner.export_state()
    }

    fn import_state(&mut self, state: &[(String, String)]) {
        self.inner.import_state(state)
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
    pub memory: BTreeMap<String, Seen>,
}

impl Record {
    pub fn new(key: &Key, poll: Option<Poll>, memory: BTreeMap<String, Seen>) -> Record {
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
/// moved. A store `to` already has is kept, and `from`'s is removed.
pub fn move_stores(from: &str, to: &str) -> usize {
    move_stores_in(&store_dir(), from, to)
}

fn move_stores_in(dir: &Path, from: &str, to: &str) -> usize {
    let mut moved = 0;
    for (path, r) in records_in(dir).filter(|(_, r)| !from.is_empty() && r.session.as_deref() == Some(from)) {
        let key = Key::new(&r.sensor, &r.project, Some(to));
        let target = key.path_in(dir);
        if read_at(&target, &key).is_none() {
            write(&target, &Record { session: key.session.clone(), ..r });
            moved += 1;
        }
        std::fs::remove_file(&path).ok();
    }
    moved
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
    use sensor_spec::{Memory, On};
    use serde_json::json;

    /// A scratch store directory, removed when dropped.
    struct Dir(PathBuf);

    impl Dir {
        fn new(tag: &str) -> Dir {
            let d = std::env::temp_dir().join(format!("attend-hosted-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            Dir(d)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    struct Quiet;

    impl Sensor for Quiet {
        fn name(&self) -> &str {
            "quiet"
        }
        fn poll(&mut self, _: &Focus) -> Vec<(f64, String)> {
            Vec::new()
        }
        fn emission_threshold(&self) -> f64 {
            1.0
        }
        fn base_interval(&self) -> Duration {
            Duration::from_secs(60)
        }
        fn min_interval(&self) -> Duration {
            Duration::from_secs(10)
        }
        fn decay_threshold(&self) -> u32 {
            3
        }
    }

    fn sensor(dir: &Path, session: Option<&str>) -> FileSensor {
        let key = Key::new("ways-update", "/p", session);
        let path = key.path_in(dir);
        FileSensor::open(key, sensor_spec::DEFAULTS, Box::new(Quiet), Some(path))
    }

    fn seen(holds: bool) -> Seen {
        Seen::from([("$".to_string(), json!(holds))])
    }

    /// The FNV-1a name the store had when it was keyed by sensor and
    /// project alone.
    fn old_name(name: &str, project: &str) -> String {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in name.bytes().chain([0]).chain(project.bytes()) {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        format!("{h:016x}.json")
    }

    #[test]
    fn a_store_is_named_by_sensor_project_and_session() {
        let d = Path::new("/s");
        let a = Key::new("git", "/p", Some("s1")).path_in(d);
        assert_eq!(a, Key::new("git", "/p", Some("s1")).path_in(d));
        assert_ne!(a, Key::new("git", "/p", Some("s2")).path_in(d), "each session has its own store");
        assert_ne!(a, Key::new("git", "/q", Some("s1")).path_in(d));
        assert_ne!(a, Key::new("gi", "t/p", Some("s1")).path_in(d), "the separator keeps the parts apart");
        let none = Key::new("git", "/p", None).path_in(d);
        assert_eq!(none, Key::new("git", "/p", Some("")).path_in(d), "an empty id is no session");
        assert_ne!(none, a);
        assert_ne!(none.file_name().unwrap().to_str().unwrap(), old_name("git", "/p"), "no key lands on a store of the old naming");
        let stem = a.file_stem().unwrap().to_str().unwrap();
        assert!(stem.len() == 16 && stem.chars().all(|c| c.is_ascii_hexdigit()), "{stem}");
    }

    #[test]
    fn session_memory_fires_enters_on_the_first_poll_that_meets_where_and_not_again_in_the_host_s_life() {
        let d = Dir::new("session");
        let mut s = sensor(&d.0, Some("s1"));
        let fires: Vec<Vec<String>> = (0..4).map(|_| s.observe("behind", On::Enters, Memory::Session, seen(true))).collect();
        assert_eq!(fires, [vec!["$".to_string()], vec![], vec![], vec![]]);
        assert!(std::fs::read_dir(&d.0).unwrap().next().is_none(), "session memory writes no store");
        // A second host, as a second session or after a restart, starts empty and fires once of its own.
        let mut again = sensor(&d.0, Some("s1"));
        assert_eq!(again.observe("behind", On::Enters, Memory::Session, seen(true)), ["$"]);
    }

    #[test]
    fn store_memory_takes_a_fresh_store_s_first_poll_as_its_baseline() {
        let d = Dir::new("store");
        let mut s = sensor(&d.0, Some("s1"));
        let mut poll = |holds| s.observe("behind", On::Enters, Memory::Store, seen(holds));
        let fires: Vec<Vec<String>> = [true, true, false, true].into_iter().map(&mut poll).collect();
        assert_eq!(fires, [vec![], vec![], vec![], vec!["$".to_string()]], "the condition held at the baseline, so only the later entry fires");
        // A restart in the same session reads the memory back and fires nothing new.
        let mut again = sensor(&d.0, Some("s1"));
        assert_eq!(again.observe("behind", On::Enters, Memory::Store, seen(true)), Vec::<String>::new());
        // Another session's store is its own, so it starts from a baseline.
        let mut other = sensor(&d.0, Some("s2"));
        assert_eq!(other.observe("behind", On::Enters, Memory::Store, seen(true)), Vec::<String>::new());
        assert_eq!(other.observe("behind", On::Enters, Memory::Store, seen(true)), Vec::<String>::new());
    }

    #[test]
    fn each_comparison_fires_on_its_change() {
        let prev = Seen::from([("a".into(), json!(true)), ("b".into(), json!(1)), ("c".into(), json!(false))]);
        let now = Seen::from([("a".into(), json!(false)), ("b".into(), json!(2)), ("c".into(), json!(true)), ("d".into(), json!(true))]);
        let at = |on| fired(on, Memory::Store, Some(&prev), &now);
        assert_eq!(at(On::Added), ["d"]);
        assert_eq!(fired(On::Removed, Memory::Store, Some(&now), &prev), ["d"]);
        assert_eq!(at(On::Changed), ["a", "b", "c"]);
        assert_eq!(at(On::Enters), ["c", "d"]);
        assert_eq!(at(On::Leaves), ["a"]);
        assert_eq!(fired(On::Added, Memory::Store, None, &now), Vec::<String>::new(), "store memory with nothing seen sets a baseline");
        assert_eq!(fired(On::Added, Memory::Session, None, &now), ["a", "b", "c", "d"], "session memory compares against nothing seen");
        assert_eq!(fired(On::Leaves, Memory::Session, None, &now), Vec::<String>::new());
    }

    #[test]
    fn a_store_of_the_old_format_or_of_another_key_reads_as_no_store() {
        let d = Dir::new("old");
        let key = Key::new("ways-update", "/p", Some("s1"));
        let path = key.path_in(&d.0);
        // A file written before stores were keyed by session: no format and no session.
        std::fs::write(&path, r#"{"sensor":"ways-update","project":"/p","poll":{"at":1,"ok":true,"observations":0}}"#).unwrap();
        assert_eq!(read_at(&path, &key), None);
        let mut s = sensor(&d.0, Some("s1"));
        assert_eq!(s.observe("behind", On::Enters, Memory::Store, seen(true)), Vec::<String>::new(), "it starts from a baseline");
        // A current file whose key is not the one asked for is not taken as its store.
        let other = Key::new("ways-update", "/p", Some("s2"));
        assert_eq!(read_at(&path, &other), None);
        assert_eq!(read_at(&path, &key).map(|r| (r.store, r.session)), Some((STORE_FORMAT, Some("s1".to_string()))));
    }

    #[test]
    fn a_session_s_stores_move_with_it_and_its_memory_comes_too() {
        let d = Dir::new("move");
        let mut s = sensor(&d.0, Some("old"));
        s.observe("behind", On::Enters, Memory::Store, seen(false));
        assert_eq!(move_stores_in(&d.0, "old", "new"), 1);
        assert!(read_at(&Key::new("ways-update", "/p", Some("old")).path_in(&d.0), &Key::new("ways-update", "/p", Some("old"))).is_none());
        let mut moved = sensor(&d.0, Some("new"));
        assert_eq!(moved.observe("behind", On::Enters, Memory::Store, seen(true)), ["$"], "the memory moved, so this is an entry and not a baseline");
        assert_eq!(move_stores_in(&d.0, "old", "new"), 0, "a second move finds nothing");
    }

    #[test]
    fn the_last_poll_is_the_session_s_own_or_with_no_session_the_newest_in_the_project() {
        let d = Dir::new("last");
        let put = |sensor: &str, project: &str, session: Option<&str>, at: u64| {
            let key = Key::new(sensor, project, session);
            let rec = Record::new(&key, Some(Poll { at, ok: true, detail: None, observations: 0 }), BTreeMap::new());
            write(&key.path_in(&d.0), &rec);
        };
        put("git", "/p", Some("s1"), 10);
        put("git", "/p", Some("s2"), 20);
        put("git", "/q", Some("s3"), 30);
        put("processes", "/p", None, 5);
        std::fs::write(d.0.join(old_name("git", "/p")), r#"{"sensor":"git","project":"/p","poll":{"at":99,"ok":true}}"#).unwrap();
        let at = |session| last_polls_in(&d.0, "/p", session).into_iter().map(|(n, p)| (n, p.at)).collect::<Vec<_>>();
        assert_eq!(at(Some("s1")), [("git".to_string(), 10)]);
        assert_eq!(at(None), [("git".to_string(), 20), ("processes".to_string(), 5)], "the old-format store at 99 is not read");
    }
}
