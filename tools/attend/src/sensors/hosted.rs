// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! A sensor as its file configures it: a compiled poll, or a poll result
//! run through the file's declared state and triggers, under the file's
//! name, cadence and threshold, with each poll's outcome, each trigger's
//! memory and the declared state recorded in the sensor's store.
//!
//! A store is one file under `$XDG_STATE_HOME/agent-ways/attend/sensor-state/`
//! for each sensor, project and session. Its name is an FNV-1a hash of the
//! three, so no part of the path comes from a sensor file. Each report goes
//! to one session, so what a sensor last saw is what that session was told,
//! and two sessions in one project keep separate stores.
//!
//! A sensor reads a store only for a resolved session id. Reading is
//! separate from writing: a test poll reads the session's store and writes
//! nothing. Two writers can change one store: the recording sensor of the
//! session's host, and a test fire with `--commit`. A recording sensor
//! reads its store again before a poll when the file changed since it last
//! wrote it, so a commit is not overwritten.
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
//! [`Sensor::export_state`], so a re-executed run keeps it. A trigger
//! definition's line is delivered at most once in the session, and one the
//! loop dropped is offered again ([`session`]).
//!
//! References:
//! - ADR-403 §1 and §3b: the store, where it lives, its retention, and the
//!   fresh baseline a discarded store or changed declaration gives.
//! - ADR-416 items 6 and 8: `memory: session` delivering its line at most
//!   once per session, and stores keyed by session and carried by a
//!   session's move.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use sensor_spec::{Cadence, Change, Expires, Match, Memory, On, SensorSpec, Test, Trigger};
use sensor_trait::{Focus, Sensor};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{eval, state};

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

/// What a poll result is, before triggers read it.
#[derive(Debug)]
pub enum Fetched {
    /// One JSON document, which the triggers and the state read.
    Json(Value),
    /// Observations as written, `magnitude|description`, which take no
    /// triggers or state.
    Lines(Vec<(f64, String)>),
}

/// Where a file sensor's poll result comes from: a command's run, or a
/// tool call on an MCP server.
pub trait Source: Send {
    /// One poll's result, or why the poll failed.
    fn fetch(&mut self) -> Result<Fetched, String>;
}

/// What runs a file sensor's poll.
pub enum Poller {
    /// A compiled sensor, which makes its own observations.
    Crate(Box<dyn Sensor>),
    /// A result the file's triggers and state evaluate.
    Spec(Box<Declared>),
}

/// A sensor whose triggers and state its file declares.
pub struct Declared {
    pub source: Box<dyn Source>,
    pub triggers: Vec<Trigger>,
    pub state: state::Declaration,
    /// Whether a top-level `state` object in the result holds emitted
    /// fields, as a command's output does.
    pub emits: bool,
    /// The one trigger a test fire names; none evaluates them all.
    pub only: Option<String>,
}

impl Declared {
    /// The triggers and state `spec` declares, read from `source`.
    pub fn of(spec: &SensorSpec, source: Box<dyn Source>) -> Declared {
        let emits = matches!(spec.block, sensor_spec::Block::Command(_));
        Declared { source, triggers: spec.triggers.clone(), state: spec.state.clone(), emits, only: None }
    }
}

/// What one trigger made of one poll.
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub trigger: String,
    /// The keys whose change fired it.
    pub fired: Vec<String>,
}

/// A sensor under its file's settings.
pub struct FileSensor {
    name: String,
    cadence: Cadence,
    poller: Poller,
    key: Key,
    /// Where the store is read from; none without a session id.
    store: Option<PathBuf>,
    /// Whether polls write the store. A test poll reads it and writes
    /// nothing.
    record: bool,
    /// The last poll's outcome.
    last: Option<Poll>,
    /// What each trigger with `memory: store` last saw, as the store holds it.
    stored: BTreeMap<String, Memo>,
    /// The declared state, as the store holds it.
    state: Option<StoredState>,
    session: SessionMemory,
    /// The session triggers whose lines are out and not settled
    /// ([`session`]).
    outstanding: session::Outstanding,
    /// Whether a session trigger became spent since [`Sensor::take_spent`]
    /// last read it.
    newly_spent: bool,
    /// The last poll's failure, for a caller that reads it.
    failure: Option<String>,
    /// Why the stored state was discarded when the store was read.
    discarded: Option<String>,
    /// What each trigger made of the last poll.
    verdicts: Vec<Verdict>,
    /// How long each observation of the last poll stays deliverable, by
    /// description, for the triggers that set `expires`.
    lapses: Vec<(String, Expires)>,
    /// When this sensor last wrote its store, or read it, as the file's
    /// modification time.
    written: Option<SystemTime>,
    /// The nonce of the record this sensor last read or wrote; none when
    /// there was no record. A filesystem with coarse timestamps can leave
    /// two writes with one mtime, and the nonce still tells them apart.
    nonce: Option<u64>,
}

impl FileSensor {
    /// The compiled sensor `inner` as the sensor `name` of `project` for
    /// `session`. With a session id it reads its store, and with `record`
    /// as well it writes it.
    pub fn new(name: String, cadence: Cadence, inner: Box<dyn Sensor>, project: &str, session: Option<&str>, record: bool) -> Self {
        FileSensor::keyed(name, cadence, Poller::Crate(inner), project, session, record)
    }

    /// The sensor `spec` declares, its results read from `source`, as
    /// [`FileSensor::new`] keys it.
    pub fn declared(spec: &SensorSpec, source: Box<dyn Source>, project: &str, session: Option<&str>, record: bool) -> Self {
        FileSensor::keyed(spec.name.clone(), spec.cadence, Poller::Spec(Box::new(Declared::of(spec, source))), project, session, record)
    }

    fn keyed(name: String, cadence: Cadence, poller: Poller, project: &str, session: Option<&str>, record: bool) -> Self {
        let key = Key::new(&name, project, session);
        let store = key.session.is_some().then(|| key.path_in(&store_dir()));
        FileSensor::open(key, cadence, poller, store, record)
    }

    /// The sensor under `key`, its store at `store`, starting from what the
    /// store holds for that key.
    fn open(key: Key, cadence: Cadence, poller: Poller, store: Option<PathBuf>, record: bool) -> Self {
        let mut s = FileSensor {
            name: key.sensor.clone(),
            cadence,
            poller,
            key,
            store,
            record,
            last: None,
            stored: BTreeMap::new(),
            state: None,
            session: SessionMemory::default(),
            outstanding: session::Outstanding::new(),
            newly_spent: false,
            failure: None,
            discarded: None,
            verdicts: Vec::new(),
            lapses: Vec::new(),
            written: None,
            nonce: None,
        };
        s.load();
        s
    }

    /// Read what the store holds for this sensor's key: the last poll, the
    /// store memory and the declared state. A stored state that another
    /// declaration wrote, or that fails the check, is discarded with the
    /// memory of the state triggers, so they start from a fresh baseline.
    fn load(&mut self) {
        let Some(path) = self.store.clone() else { return };
        self.written = modified(&path);
        let rec = read_at(&path, &self.key);
        self.nonce = rec.as_ref().map(|r| r.nonce);
        let (last, mut stored, mut kept) = rec.map(|r| (r.poll, r.memory, r.state)).unwrap_or_default();
        if let (Poller::Spec(d), Some(s)) = (&self.poller, &kept) {
            let why = match state::check(&d.state, &s.fields) {
                _ if s.decl != state::fingerprint(&d.state) => Some("the state declaration changed".to_string()),
                Err(e) => Some(e),
                Ok(()) => None,
            };
            if let Some(why) = why {
                stored.retain(|name, _| !d.triggers.iter().any(|t| &t.name == name && matches!(t.test, Test::State(_))));
                self.discarded = Some(format!("its stored state was discarded, so it starts from a baseline: {why}"));
                kept = None;
            }
        }
        (self.last, self.stored, self.state) = (last, stored, kept);
    }

    /// Read the store again when another writer changed it since this
    /// sensor last wrote it, such as a test fire with `--commit`, so what
    /// that writer kept is not overwritten.
    fn refresh(&mut self) {
        if let (Some(path), true) = (&self.store, self.record) {
            if modified(path) != self.written || stored_nonce(path, &self.key) != self.nonce {
                self.load();
            }
        }
    }

    /// Evaluate only the trigger `name` from now on, as a test fire of one
    /// trigger does.
    pub fn only(&mut self, name: &str) {
        if let Poller::Spec(d) = &mut self.poller {
            d.only = Some(name.to_string());
        }
    }

    /// What each trigger made of the last poll.
    pub fn verdicts(&self) -> &[Verdict] {
        &self.verdicts
    }

    /// The declared state after the last poll.
    pub fn state(&self) -> Option<&state::Fields> {
        self.state.as_ref().map(|s| &s.fields)
    }

    /// Record what the trigger `trigger` sees on this poll, and return the
    /// keys whose change fires it. Memory kept under another definition of
    /// the trigger counts as none.
    ///
    /// An item trigger sees each item's key mapped to what it compares; a
    /// state trigger sees its fields' values under their names, and fires
    /// once, under the key `$`, when they change as it says.
    ///
    /// Store memory reaches the store when the sensor next saves: after a
    /// poll, or on [`FileSensor::commit`]. A fresh store's first poll sets a
    /// baseline. Session memory starts from nothing seen, and a trigger
    /// definition with it is delivered at most once in the session, whatever
    /// it compares on. It fires nothing while its lines are out, and is
    /// spent once they have all settled ([`session`]). Until then the keys
    /// that fired keep what the trigger saw before, so a released trigger
    /// fires again on the next poll whose result still meets it. Once spent,
    /// a later change, such as a condition that leaves and enters again,
    /// fires nothing. The mark is kept by fingerprint, so a renamed trigger
    /// stays spent and an edited one fires once more.
    pub fn observe(&mut self, trigger: &Trigger, now: Seen) -> Vec<String> {
        let test = trigger.fingerprint();
        let mem = match trigger.memory {
            Memory::Store => &mut self.stored,
            Memory::Session => &mut self.session.held,
        };
        let prev = mem.get(&trigger.name).filter(|m| m.test == test).map(|m| &m.seen);
        let mut out = match &trigger.test {
            Test::Items { on, .. } => fired(*on, trigger.memory, prev, &now),
            Test::State(changes) => match prev {
                Some(prev) if state_fired(changes, trigger.match_, prev, &now) => vec!["$".to_string()],
                _ => Vec::new(),
            },
        };
        let mut seen = now;
        if trigger.memory == Memory::Session {
            if self.session.spent.contains(&test) {
                out.clear();
            }
            for k in &out {
                match prev.and_then(|p| p.get(k)) {
                    Some(v) => seen.insert(k.clone(), v.clone()),
                    None => seen.remove(k),
                };
            }
            if self.outstanding.contains_key(&test) {
                out.clear();
            }
        }
        mem.insert(trigger.name.clone(), Memo { test, seen });
        out
    }

    /// What a trigger last saw, under its current definition.
    fn prior(&self, trigger: &Trigger) -> Option<Seen> {
        let mem = match trigger.memory {
            Memory::Store => &self.stored,
            Memory::Session => &self.session.held,
        };
        mem.get(&trigger.name).filter(|m| m.test == trigger.fingerprint()).map(|m| m.seen.clone())
    }

    /// Run `result` through the declared state and every trigger, and
    /// return the observations the fired triggers make. Nothing is written
    /// here: a poll saves once after it, and a test fire only through
    /// [`FileSensor::commit`].
    ///
    /// A state write that fails a check is refused whole and becomes the
    /// poll's failure. The item triggers still run, and the state triggers
    /// are skipped, so their memory is neither created nor advanced.
    pub fn evaluate(&mut self, result: &Value) -> Vec<(f64, String)> {
        let Poller::Spec(d) = &self.poller else { return Vec::new() };
        let (triggers, decl, emits, only) = (d.triggers.clone(), d.state.clone(), d.emits, d.only.clone());
        let prev = self.state.as_ref().map_or_else(|| state::fresh(&decl), |s| s.fields.clone());
        let emitted = emits.then(|| result.get("state").and_then(Value::as_object)).flatten();
        let (next, refused) = match state::write(&decl, &prev, result, emitted) {
            Ok(next) => (next, false),
            Err(why) => {
                self.failure = Some(format!("the state write was refused: {why}"));
                (prev.clone(), true)
            }
        };
        if !decl.is_empty() && !refused {
            self.state = Some(StoredState { decl: state::fingerprint(&decl), fields: next.clone() });
        }
        self.verdicts.clear();
        self.lapses.clear();
        let mut out = Vec::new();
        for t in triggers.iter().filter(|t| only.as_ref().is_none_or(|o| *o == t.name)) {
            if refused && matches!(t.test, Test::State(_)) {
                self.verdicts.push(Verdict { trigger: t.name.clone(), fired: Vec::new() });
                continue;
            }
            let (now, items) = seen_by(t, result, &next);
            let before = self.prior(t).unwrap_or_default();
            let keys = self.observe(t, now);
            let mut offered = std::collections::BTreeSet::new();
            for k in &keys {
                let gone = keyed_item(t, k);
                let item = items.get(k).or_else(|| before.get(k).filter(|v| !v.is_boolean())).or(gone.as_ref());
                let fill = eval::Fill { item, state: &next, prev: &prev, count: keys.len() };
                let text = match &t.description {
                    Some(tpl) => eval::fill(tpl, &fill),
                    None if k == "$" => format!("{} {}", self.name, t.name),
                    None => format!("{} {}: {k}", self.name, t.name),
                };
                let text = super::deliverable(&text);
                if t.memory == Memory::Session {
                    offered.insert(text.clone());
                }
                if let Some(e) = t.expires {
                    self.lapses.push((text.clone(), e));
                }
                out.push((t.magnitude.unwrap_or(1.0), text));
            }
            self.offer(t.fingerprint(), offered);
            self.verdicts.push(Verdict { trigger: t.name.clone(), fired: keys });
        }
        out
    }

    /// Write the trigger memory and declared state the last evaluation
    /// left, as a test fire with `--commit` keeps them. Writes nothing for
    /// a sensor that does not record. Refused when another writer, such as
    /// the session's own poll, changed the store since this sensor read it,
    /// since the commit would overwrite what that writer kept.
    pub fn commit(&mut self) -> Result<(), String> {
        if let (Some(path), true) = (&self.store, self.record) {
            if stored_nonce(path, &self.key) != self.nonce {
                return Err("the store changed during the fire; fire again".into());
            }
        }
        self.save();
        Ok(())
    }

    fn save(&mut self) {
        if let (Some(path), true) = (&self.store, self.record) {
            let rec = Record { state: self.state.clone(), ..Record::new(&self.key, self.last.clone(), self.stored.clone()) };
            if write(path, &rec) {
                self.nonce = Some(rec.nonce);
            }
            self.written = modified(path);
        }
    }
}

/// The nonce of the record at `path` under `key`; none when there is none.
fn stored_nonce(path: &Path, key: &Key) -> Option<u64> {
    read_at(path, key).map(|r| r.nonce)
}

/// A value that differs for each record written: the time in
/// nanoseconds, the process and a count within it, hashed.
fn fresh_nonce() -> u64 {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    agent_identity::identity::fnv1a_64(format!("{nanos}:{}:{seq}", std::process::id()).as_bytes())
}

/// When the file at `path` was last written, or none when it cannot be read.
fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// The item a trigger with a `key` saw under `k`, as far as the key tells:
/// `{<key>: k}`, nested for a dotted key. A `leaves` trigger whose item is
/// gone from the result fills its description from this.
fn keyed_item(t: &Trigger, k: &str) -> Option<Value> {
    let Test::Items { key: Some(field), .. } = &t.test else { return None };
    Some(field.rsplit('.').fold(Value::String(k.to_string()), |inner, part| serde_json::json!({ part: inner })))
}

/// What the trigger `t` sees in `result`, with the state `fields` after the
/// poll's write: each key mapped to what it compares, and each key's item.
///
/// An item's key is its `key` field, or for a trigger without one, `$`
/// when the path selects one value and the item's JSON text when it may
/// select many. For `enters` and `leaves` every item is seen, mapped to
/// whether it meets `where`; for the other comparisons only the items that
/// meet it are seen.
fn seen_by(t: &Trigger, result: &Value, fields: &state::Fields) -> (Seen, BTreeMap<String, Value>) {
    let mut seen = Seen::new();
    let mut items = BTreeMap::new();
    match &t.test {
        Test::State(changes) => {
            seen.extend(changes.keys().map(|f| (f.clone(), fields.get(f).cloned().unwrap_or(Value::Null))));
        }
        Test::Items { path, key, on, where_ } => {
            for item in eval::select(path, result) {
                let k = match key {
                    Some(field) => match eval::field(item, field) {
                        Some(v) => eval::text(v),
                        None => continue,
                    },
                    None if !path.is_wild() => "$".to_string(),
                    None => item.to_string(),
                };
                let meets = eval::holds(where_, t.match_, item);
                match on {
                    On::Enters | On::Leaves => {
                        seen.insert(k.clone(), Value::Bool(meets));
                    }
                    _ if meets => {
                        seen.insert(k.clone(), item.clone());
                    }
                    _ => continue,
                }
                items.insert(k, item.clone());
            }
        }
    }
    (seen, items)
}

/// Whether `second`, a poll result taken right after `first`, kept what
/// `first` returned, as `attend sensor probe` checks a tool that should
/// change nothing by being read: every item an item trigger selects from
/// `first`, by its key, is selected from `second`, and for `enters` and
/// `leaves` still meets `where` as it did; every field a state trigger
/// watches holds the same value after each result's write. With no
/// triggers the two results are compared whole. The error names the first
/// trigger that saw a difference, and what it was.
///
/// A first result in which no item trigger selects anything, or an empty
/// one for a sensor with no triggers, shows nothing that a read could have
/// changed, and is refused: the probe has nothing to compare.
///
/// References:
/// - ADR-403 §4: the probe's call comparison.
pub fn kept(spec: &SensorSpec, first: &Value, second: &Value) -> Result<(), String> {
    const NOTHING: &str = "nothing to compare; probe while the tool returns items";
    let empty = |v: &Value| match v {
        Value::Null => true,
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        _ => false,
    };
    if spec.triggers.is_empty() {
        return match () {
            _ if empty(first) => Err(NOTHING.into()),
            _ if eval::same(first, second) => Ok(()),
            _ => Err("the second call returned another result".into()),
        };
    }
    let write = |r: &Value| state::write(&spec.state, &state::fresh(&spec.state), r, None);
    let mut selected = false;
    for t in &spec.triggers {
        match &t.test {
            Test::State(changes) => {
                let (a, b) = (write(first)?, write(second)?);
                if let Some(f) = changes.keys().find(|f| a.get(*f) != b.get(*f)) {
                    return Err(format!("trigger {}: the second call left {f} at another value", t.name));
                }
            }
            Test::Items { on, .. } => {
                let (seen_a, a) = seen_by(t, first, &state::Fields::new());
                let (seen_b, b) = seen_by(t, second, &state::Fields::new());
                selected |= !a.is_empty();
                if let Some(k) = a.keys().find(|k| !b.contains_key(*k)) {
                    return Err(format!("trigger {}: the second call did not return the item {k}", t.name));
                }
                if matches!(on, On::Enters | On::Leaves) {
                    if let Some(k) = seen_a.keys().find(|k| seen_a.get(*k) != seen_b.get(*k)) {
                        return Err(format!("trigger {}: the item {k} meets where differently in the second call", t.name));
                    }
                }
            }
        }
    }
    let items = spec.triggers.iter().any(|t| matches!(t.test, Test::Items { .. }));
    match items && !selected {
        true => Err(NOTHING.into()),
        false => Ok(()),
    }
}

/// Whether a state trigger's fields changed as `changes` says between
/// `prev` and `now`, combined by `how`. A number rises or falls by value.
fn state_fired(changes: &BTreeMap<String, Change>, how: Match, prev: &Seen, now: &Seen) -> bool {
    let moved = |(field, change): (&String, &Change)| {
        let (a, b) = (prev.get(field), now.get(field));
        let num = |v: Option<&Value>| v.and_then(Value::as_f64);
        match change {
            Change::Changed => match (a, b) {
                (Some(x), Some(y)) => !eval::same(x, y),
                (x, y) => x != y,
            },
            Change::Rises => matches!((num(a), num(b)), (Some(x), Some(y)) if y > x),
            Change::Falls => matches!((num(a), num(b)), (Some(x), Some(y)) if y < x),
        }
    };
    match how {
        Match::All => changes.iter().all(moved),
        Match::Any => changes.iter().any(moved),
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
        match &self.poller {
            Poller::Crate(inner) => inner.description(),
            Poller::Spec(_) => "",
        }
    }

    fn source(&self) -> String {
        match &self.poller {
            Poller::Crate(inner) => inner.source(),
            Poller::Spec(_) => String::new(),
        }
    }

    fn poll(&mut self, focus: &Focus) -> Vec<(f64, String)> {
        self.failure = None;
        self.refresh();
        let out = match &mut self.poller {
            Poller::Crate(inner) => {
                let out = inner.poll(focus);
                self.failure = inner.take_failure();
                out
            }
            Poller::Spec(d) => match d.source.fetch() {
                Err(why) => {
                    self.failure = Some(why);
                    Vec::new()
                }
                Ok(Fetched::Lines(lines)) => lines.into_iter().map(|(m, d)| (m, super::deliverable(&d))).collect(),
                Ok(Fetched::Json(result)) => self.evaluate(&result),
            },
        };
        let detail = self.failure.clone().or_else(|| self.discarded.take());
        self.last = Some(Poll { at: sensor_trait::epoch_secs(), ok: self.failure.is_none(), detail, observations: out.len() });
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
        let mut out = match &self.poller {
            Poller::Crate(inner) => inner.export_state(),
            Poller::Spec(_) => Vec::new(),
        };
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
        if let (Poller::Crate(inner), false) = (&mut self.poller, rest.is_empty()) {
            inner.import_state(&rest);
        }
    }

    fn take_failure(&mut self) -> Option<String> {
        self.failure.take()
    }

    /// Spend each session trigger whose line `description` is.
    fn delivered(&mut self, description: &str) {
        self.settle(description);
    }

    fn dropped(&mut self, description: &str) {
        self.release(description);
    }

    fn waits_on(&self, description: &str) -> bool {
        self.awaits(description)
    }

    fn take_spent(&mut self) -> bool {
        std::mem::take(&mut self.newly_spent)
    }

    /// How long the observation `description` of the last poll stays
    /// deliverable: its trigger's `expires`. None when the trigger sets no
    /// `expires`, or no trigger made it; the writer then applies its
    /// lane's default.
    fn lapse(&self, description: &str) -> Option<sensor_trait::Lapse> {
        self.lapses.iter().find(|(d, _)| d == description).map(|(_, e)| match e {
            Expires::Seconds(n) => sensor_trait::Lapse::After(*n),
            Expires::Session => sensor_trait::Lapse::UntilRead,
        })
    }
}

mod session;
use session::SessionMemory;

mod store;
pub use store::*;

#[cfg(test)]
mod tests;
