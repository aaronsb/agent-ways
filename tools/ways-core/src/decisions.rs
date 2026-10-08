//! Reader for the decision log (ADR-701 §2).
//!
//! Each prompt and task scan writes one `kind: scan` record to
//! `decisions.jsonl`, and each pull writes one `kind: pull` record that names
//! the scan its turn joined through `scan_id`. Tuning, stats and the learning
//! loop of ADR-701 §8 read these records; the event stream stays a debugging
//! trail.
//!
//! [`Records`] streams the records of the dated archives, oldest first, then
//! the live file, one source in memory at a time. It is tolerant: a malformed
//! line, a line of an unknown kind and a line outside the [`Window`] are
//! skipped, and fields it does not know are ignored. [`Turns`] groups the
//! stream into turns and judges each pull as it goes.
//!
//! A turn is opened by a scan with `turn_start: true` and keyed by
//! `(session, agent)`. The epoch is not a turn index: the command and file
//! lanes bump it on every tool call. Later scans of the same session and agent
//! with `turn_start: false` belong to the open turn. A pull joins the scan its
//! `scan_id` names, through an index of every scan in the window, so it joins
//! even after that scan's turn has closed.

use serde::Deserialize;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::event_archive::{self, DECISIONS};

/// A ts range, both ends inclusive, compared as UTC ISO text
/// (`YYYY-MM-DDThh:mm:ssZ` sorts as it reads). An end left `None` is open.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Window {
    pub since: Option<String>,
    pub until: Option<String>,
}

impl Window {
    /// Every record.
    pub fn all() -> Self {
        Window::default()
    }

    /// Whether a record stamped `ts` falls in the window.
    pub fn contains(&self, ts: &str) -> bool {
        self.since.as_deref().is_none_or(|s| ts >= s) && self.until.as_deref().is_none_or(|u| ts <= u)
    }
}

/// One top candidate of a scan.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Candidate {
    pub way: String,
    pub cosine: Option<f64>,
    pub share: Option<f64>,
    pub margin: Option<f64>,
}

/// What became of one way a scan matched or nearly matched.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Outcome {
    pub way: String,
    /// `fired`, `redisclosed`, `held_refire`, `held_context_cap`,
    /// `keyword_gated`, `near_miss`, `judge_block`, `ancestor_block`,
    /// `withheld_for_parent`, `stashed`, `not_fireable` or `error`.
    pub result: String,
    pub rank: Option<u32>,
    pub channel: Option<String>,
    pub p: Option<f64>,
    /// The judge's probability for a way it saw.
    pub p_yes: Option<f64>,
    /// `pass`, `block` or `would_block`, on a judged way the gate let through.
    pub verdict: Option<String>,
    pub threshold: Option<f64>,
    pub mode: Option<String>,
    pub ancestor: Option<String>,
    /// On a near miss: the smallest `tau_s - probability`.
    pub shortfall: Option<f64>,
    pub tau_s: Option<f64>,
    pub prob_en: Option<f64>,
    pub prob_multi: Option<f64>,
}

/// How the relevance gate ran on a scan.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Judge {
    /// `off`, `idle`, `judged` or `fallback`.
    pub status: String,
    pub reason: Option<String>,
    pub engine: Option<String>,
    pub model: Option<String>,
    pub judge_ms: Option<u64>,
    /// Ways the gate's cap left unjudged.
    pub capped: Vec<String>,
}

/// A `kind: scan` record: one prompt or task scan.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Scan {
    pub ts: String,
    pub scan_id: String,
    pub session: String,
    pub agent: String,
    pub epoch: u64,
    pub turn_start: bool,
    pub token_position: Option<u64>,
    /// `prompt` or `task`.
    pub surface: String,
    pub hook_event: String,
    pub scope: String,
    pub project: String,
    pub lane: Option<String>,
    pub basis: Option<String>,
    pub sidecar: bool,
    pub candidates: Vec<Candidate>,
    pub outcomes: Vec<Outcome>,
    pub judge: Option<Judge>,
}

/// A `kind: pull` record: one `ways_read` the hook saw, stamped or refused.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Pull {
    pub ts: String,
    pub session: String,
    pub agent: String,
    pub epoch: u64,
    pub token_position: Option<u64>,
    pub way: String,
    /// `first_fire`, `refire`, `suppressed`, or `none` when not consulted.
    pub window: String,
    pub out_of_band: bool,
    pub stamped: bool,
    /// Why nothing was stamped, when nothing was.
    pub reason: Option<String>,
    /// The scan the calling agent's last-scan marker named; `None` when it
    /// named none.
    pub scan_id: Option<String>,
}

/// One decision record. A scan is boxed: it is twice a pull's size.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Record {
    Scan(Box<Scan>),
    Pull(Pull),
}

impl Record {
    pub fn ts(&self) -> &str {
        match self {
            Record::Scan(s) => &s.ts,
            Record::Pull(p) => &p.ts,
        }
    }
}

/// Parse one line, `None` for a blank, malformed or unknown-kind line.
pub fn parse_line(line: &str) -> Option<Record> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    serde_json::from_str(line).ok()
}

/// The decision log's sources in time order: archives oldest first, then the
/// live file when it exists.
pub fn sources() -> Vec<PathBuf> {
    crate::paths::decisions_log().parent().map(sources_in).unwrap_or_default()
}

/// [`sources`] for a decision log kept in `dir`.
pub fn sources_in(dir: &Path) -> Vec<PathBuf> {
    let mut found = event_archive::archives(dir, DECISIONS);
    let live = dir.join(DECISIONS.live_name());
    if live.exists() {
        found.push(live);
    }
    found
}

/// One source being read. The live file is streamed line by line. An archive
/// is held decoded, one day at a time: [`event_archive::read_source`] returns
/// the whole buffer, and its recovery from a torn gzip member is what the
/// reader relies on.
enum Source {
    Decoded { text: String, pos: usize },
    Live(BufReader<std::fs::File>),
}

impl Source {
    fn open(path: &Path) -> Option<Source> {
        if path.extension().is_some_and(|e| e == "gz") {
            Some(Source::Decoded { text: event_archive::read_source(path)?, pos: 0 })
        } else {
            event_archive::open_live_read(path).ok().map(|f| Source::Live(BufReader::new(f)))
        }
    }

    /// The next line, `None` at the end. An unreadable stretch ends the source.
    fn line(&mut self) -> Option<String> {
        match self {
            Source::Decoded { text, pos } => {
                let rest = text.get(*pos..).filter(|r| !r.is_empty())?;
                let end = rest.find('\n').map_or(rest.len(), |i| i + 1);
                *pos += end;
                Some(rest[..end].to_string())
            }
            Source::Live(r) => {
                let mut buf = Vec::new();
                match r.read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => None,
                    Ok(_) => Some(String::from_utf8_lossy(&buf).into_owned()),
                }
            }
        }
    }
}

/// A streaming reader over decision-log sources, one source open at a time;
/// an unreadable source is skipped.
pub struct Records {
    pending: std::vec::IntoIter<PathBuf>,
    current: Option<Source>,
    window: Window,
    skipped: usize,
}

impl Records {
    /// Records of `sources`, in order, that fall in `window`.
    pub fn new(sources: Vec<PathBuf>, window: Window) -> Self {
        Records { pending: sources.into_iter(), current: None, window, skipped: 0 }
    }

    /// Records of the installed decision log in `window`.
    pub fn open(window: Window) -> Self {
        Records::new(sources(), window)
    }

    /// Non-blank lines skipped so far as malformed or of an unknown kind.
    /// Lines outside the window are not counted.
    pub fn skipped(&self) -> usize {
        self.skipped
    }

    fn next_line(&mut self) -> Option<String> {
        loop {
            if let Some(line) = self.current.as_mut().and_then(Source::line) {
                return Some(line);
            }
            self.current = Source::open(&self.pending.next()?);
        }
    }
}

impl Iterator for Records {
    type Item = Record;

    fn next(&mut self) -> Option<Record> {
        loop {
            let line = self.next_line()?;
            if line.trim().is_empty() {
                continue;
            }
            match parse_line(&line) {
                Some(r) if self.window.contains(r.ts()) => return Some(r),
                Some(_) => {}
                None => self.skipped += 1,
            }
        }
    }
}

/// One turn: the scan that opened it and the scans attributed to it after,
/// in log order. Pulls are not held here: each is judged as it arrives (see
/// [`Joined`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Turn {
    /// The turn's place in the stream, counting from 0. A [`Joined`] pull
    /// names its turn by it.
    pub ordinal: u32,
    pub start: Scan,
    /// Later scans: the turn agent's own `turn_start: false` scans, and
    /// scans of other agents of the session with no turn of their own (a
    /// subagent's nested dispatch), attributed to the session's most recently
    /// active turn.
    pub scans: Vec<Scan>,
}

impl Turn {
    /// Every scan of the turn, the opening one first.
    pub fn all_scans(&self) -> impl Iterator<Item = &Scan> {
        std::iter::once(&self.start).chain(&self.scans)
    }
}

/// How a pull relates to what its context had already been given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Join {
    /// The context already had the way. A pull by the agent that wrote the
    /// named scan: that agent's scans in its own turn, up to the pull, fired
    /// or re-disclosed it (or, for a scan outside such a turn, the scan
    /// itself did). A pull by another agent (a subagent whose
    /// marker names its dispatch scan): the named scan stashed it.
    Delivered,
    /// The pull's context had not been given the way.
    RecallMiss,
    /// The pull named no scan.
    NoScanId,
    /// The pull named a scan that is not in the window.
    Unknown,
}

/// A pull with its verdict, the turn of the scan it named and that scan's
/// project.
#[derive(Clone, Debug, PartialEq)]
pub struct Joined {
    pub pull: Pull,
    pub join: Join,
    pub turn: Option<u32>,
    pub project: Option<String>,
}

/// What [`Turns`] hands on.
#[derive(Clone, Debug, PartialEq)]
pub enum Grouped {
    /// A turn, complete: its agent started another, it was idle past
    /// [`IDLE_SECS`], or the stream ended.
    Turn(Turn),
    /// A `turn_start: false` scan with no turn in its session to belong to,
    /// such as one whose turn began before the window.
    Orphan(Scan),
    /// A pull, judged when it arrives.
    Pull(Joined),
}

/// An open turn idle this long, by record ts, is handed on.
pub const IDLE_SECS: u64 = 86_400;
const SWEEP_SECS: u64 = 3_600;

/// What a later pull needs of one scan, held for every scan in the window.
/// Names are interned, so an entry is a few words.
struct ScanInfo {
    turn: Option<u32>,
    agent: u32,
    project: u32,
    /// Ways this scan stashed for a subagent.
    stashed: Box<[u32]>,
    /// Ways this scan fired or re-disclosed; read only for a scan with no turn.
    delivered: Box<[u32]>,
}

/// What a later pull needs of one turn.
struct TurnInfo {
    /// The agent whose prompt opened the turn.
    agent: u32,
    /// Ways the turn agent's scans fired or re-disclosed so far.
    delivered: Vec<u32>,
}

struct Open {
    turn: Turn,
    agent: u32,
    last: u64,
}

#[derive(Default)]
struct Names {
    ids: HashMap<Box<str>, u32>,
    names: Vec<Box<str>>,
}

impl Names {
    fn id(&mut self, s: &str) -> u32 {
        if let Some(&i) = self.ids.get(s) {
            return i;
        }
        let i = self.names.len() as u32;
        self.names.push(s.into());
        self.ids.insert(s.into(), i);
        i
    }

    fn find(&self, s: &str) -> Option<u32> {
        self.ids.get(s).copied()
    }
}

/// Groups a record stream into turns and judges each pull, as it arrives.
///
/// Every scan in the window is indexed by `scan_id`, so a pull joins the scan
/// it names whether or not that scan's turn is still open: a background
/// subagent's pull, naming the dispatch scan of a turn main has since moved
/// past, still joins. A turn is handed on when its agent starts another,
/// when no scan has joined it for [`IDLE_SECS`] of record time, or at the end.
#[derive(Default)]
pub struct Turns {
    open: HashMap<(String, String), Open>,
    scans: HashMap<Box<str>, ScanInfo>,
    turns: Vec<TurnInfo>,
    names: Names,
    /// Agents that open turns of their own: main, and any agent seen writing
    /// a `turn_start` scan (a teammate).
    turn_agents: std::collections::HashSet<u32>,
    /// The record clock: the highest ts accepted so far, and a jump of more
    /// than [`IDLE_SECS`] seen once and not yet confirmed.
    clock: u64,
    jump: bool,
    last_sweep: u64,
}

fn ways_with(scan: &Scan, names: &mut Names, results: &[&str]) -> Box<[u32]> {
    scan.outcomes.iter().filter(|o| results.contains(&o.result.as_str())).map(|o| names.id(&o.way)).collect()
}

const DELIVERED: &[&str] = &["fired", "redisclosed"];

/// The agent id of a session's main agent.
const MAIN: &str = "main";

impl Turns {
    pub fn new() -> Self {
        Turns::default()
    }

    /// Scans indexed so far, for measuring the index.
    pub fn indexed(&self) -> usize {
        self.scans.len()
    }

    /// Take one record, handing on whatever it completes.
    pub fn push(&mut self, record: Record, emit: &mut impl FnMut(Grouped)) {
        if let Some(now) = agent_fmt::when::parse_utc_iso(record.ts()).filter(|&t| self.advance(t)) {
            if now >= self.last_sweep + SWEEP_SECS {
                self.last_sweep = now;
                self.close_idle(now, emit);
            }
        }
        match record {
            Record::Scan(scan) => self.scan(*scan, emit),
            Record::Pull(pull) => {
                let joined = self.join(pull);
                emit(Grouped::Pull(joined));
            }
        }
    }

    /// Whether ts `t` may drive the sweep clock. A record more than
    /// [`IDLE_SECS`] past the clock is held back once: a lone future-dated
    /// record would otherwise hand on every open turn at once and stall the
    /// sweep until record time caught up. A second record past it confirms a
    /// real gap in the log. The record itself is read either way.
    fn advance(&mut self, t: u64) -> bool {
        if self.clock != 0 && t > self.clock + IDLE_SECS && !self.jump {
            self.jump = true;
            return false;
        }
        self.jump = false;
        self.clock = self.clock.max(t);
        true
    }

    fn scan(&mut self, scan: Scan, emit: &mut impl FnMut(Grouped)) {
        let now = agent_fmt::when::parse_utc_iso(&scan.ts).unwrap_or(self.last_sweep);
        let agent = self.names.id(&scan.agent);
        let key = (scan.session.clone(), scan.agent.clone());
        let turn_key = if scan.turn_start {
            if let Some(done) = self.open.remove(&key) {
                emit(Grouped::Turn(done.turn));
            }
            let ordinal = self.turns.len() as u32;
            self.turns.push(TurnInfo { agent, delivered: Vec::new() });
            self.turn_agents.insert(agent);
            let start = scan.clone();
            self.open.insert(key.clone(), Open { turn: Turn { ordinal, start, scans: Vec::new() }, agent, last: now });
            Some(key)
        } else if self.open.contains_key(&key) {
            Some(key)
        } else if scan.agent == MAIN || self.turn_agents.contains(&agent) {
            // An agent that opens its own turns, whose turn is not open here
            // (idle, or begun before the window), never joins another's.
            None
        } else {
            // No turn of its own: a nested dispatch from inside a subagent.
            // It belongs to the session's most recently active turn.
            self.open.iter().filter(|(k, _)| k.0 == scan.session).max_by_key(|(_, o)| (o.last, o.turn.ordinal)).map(|(k, _)| k.clone())
        };
        let info = ScanInfo {
            turn: turn_key.as_ref().map(|k| self.open[k].turn.ordinal),
            agent,
            project: self.names.id(&scan.project),
            stashed: ways_with(&scan, &mut self.names, &["stashed"]),
            delivered: ways_with(&scan, &mut self.names, DELIVERED),
        };
        if let Some(k) = &turn_key {
            let open = self.open.get_mut(k).expect("open turn");
            if open.agent == agent {
                self.turns[open.turn.ordinal as usize].delivered.extend(info.delivered.iter());
            }
            open.last = open.last.max(now);
            if !scan.turn_start {
                open.turn.scans.push(scan.clone());
            }
        }
        if !scan.scan_id.is_empty() {
            self.scans.insert(scan.scan_id.as_str().into(), info);
        }
        if turn_key.is_none() {
            emit(Grouped::Orphan(scan));
        }
    }

    fn join(&self, pull: Pull) -> Joined {
        let Some(id) = pull.scan_id.as_deref() else { return Joined { pull, join: Join::NoScanId, turn: None, project: None } };
        let Some(info) = self.scans.get(id) else { return Joined { pull, join: Join::Unknown, turn: None, project: None } };
        let way = self.names.find(&pull.way);
        let has = |set: &[u32]| way.is_some_and(|w| set.contains(&w));
        let delivered = if self.names.find(&pull.agent) == Some(info.agent) {
            match info.turn.map(|t| &self.turns[t as usize]).filter(|t| t.agent == info.agent) {
                Some(t) => has(&t.delivered),
                None => has(&info.delivered),
            }
        } else {
            has(&info.stashed)
        };
        let project = Some(self.names.names[info.project as usize].to_string());
        Joined { pull, join: if delivered { Join::Delivered } else { Join::RecallMiss }, turn: info.turn, project }
    }

    fn close_idle(&mut self, now: u64, emit: &mut impl FnMut(Grouped)) {
        let idle: Vec<(String, String)> = self.open.iter().filter(|(_, o)| o.last + IDLE_SECS < now).map(|(k, _)| k.clone()).collect();
        let mut done: Vec<Turn> = idle.iter().filter_map(|k| self.open.remove(k)).map(|o| o.turn).collect();
        done.sort_by_key(|t| t.ordinal);
        for t in done {
            emit(Grouped::Turn(t));
        }
    }

    /// Hand on every turn still open, in the order they opened.
    pub fn finish(mut self, emit: &mut impl FnMut(Grouped)) {
        let mut rest: Vec<Turn> = self.open.drain().map(|(_, o)| o.turn).collect();
        rest.sort_by_key(|t| t.ordinal);
        for t in rest {
            emit(Grouped::Turn(t));
        }
    }
}

/// Group a record stream into turns and judged pulls, handing each on as it
/// completes.
pub fn group(records: impl IntoIterator<Item = Record>, mut emit: impl FnMut(Grouped)) {
    let mut turns = Turns::new();
    for r in records {
        turns.push(r, &mut emit);
    }
    turns.finish(&mut emit);
}

#[cfg(test)]
mod fixture {
    //! Synthetic decision-log lines shared by the reader's tests.

    /// Session `s1`, agent `main`: turn 1 (epoch 3) fires `d/a` and nearly
    /// misses `d/n`; a task scan stashes `d/t`; turn 2 (epoch 3 again: the
    /// epoch is no turn index) holds `d/b` on the refire window.
    pub const S1: &[&str] = &[
        r#"{"ts":"2026-09-01T10:00:00Z","kind":"scan","scan_id":"a1","session":"s1","agent":"main","epoch":3,"turn_start":true,"surface":"prompt","project":"/p","outcomes":[{"way":"d/a","result":"fired","rank":1,"channel":"keyword","p_yes":0.9,"verdict":"pass"},{"way":"d/n","result":"near_miss","shortfall":0.04}],"judge":{"status":"judged","engine":"anthropic"},"future_field":1}"#,
        r#"{"ts":"2026-09-01T10:00:05Z","kind":"scan","scan_id":"a2","session":"s1","agent":"main","epoch":4,"turn_start":false,"surface":"task","project":"/p","outcomes":[{"way":"d/t","result":"stashed","rank":1}]}"#,
        r#"{"ts":"2026-09-01T10:01:00Z","kind":"pull","session":"s1","agent":"main","epoch":5,"way":"d/a","window":"refire","out_of_band":false,"stamped":true,"scan_id":"a1"}"#,
        r#"{"ts":"2026-09-01T10:02:00Z","kind":"pull","session":"s1","agent":"main","epoch":5,"way":"d/x","window":"suppressed","out_of_band":true,"stamped":true,"scan_id":"a2"}"#,
        r#"{"ts":"2026-09-01T11:00:00Z","kind":"scan","scan_id":"a3","session":"s1","agent":"main","epoch":3,"turn_start":true,"surface":"prompt","project":"/p","outcomes":[{"way":"d/b","result":"held_refire","rank":1}],"judge":{"status":"fallback","reason":"deadline","capped":["d/z"]}}"#,
    ];

    /// Session `s2`, interleaved with `s1`.
    pub const S2: &[&str] = &[
        r#"{"ts":"2026-09-01T10:00:30Z","kind":"scan","scan_id":"b1","session":"s2","agent":"main","epoch":1,"turn_start":true,"surface":"prompt","project":"/q","outcomes":[{"way":"d/a","result":"judge_block","p_yes":0.05,"threshold":0.3},{"way":"d/n","result":"near_miss","shortfall":0.02}],"judge":{"status":"judged"}}"#,
        r#"{"ts":"2026-09-02T09:00:00Z","kind":"pull","session":"s2","agent":"main","epoch":1,"way":"d/c","window":"first_fire","out_of_band":false,"stamped":true,"scan_id":null}"#,
        r#"{"ts":"2026-09-02T09:00:10Z","kind":"pull","session":"s2","agent":"main","epoch":1,"way":"d/a","window":"first_fire","out_of_band":false,"stamped":true,"scan_id":"b1"}"#,
    ];

    pub const MALFORMED: &str = r#"{"ts":"2026-09-01T10:00:40Z","kind":"scan","scan_id":"#;
    pub const UNKNOWN_KIND: &str = r#"{"ts":"2026-09-01T10:00:41Z","kind":"verdict","session":"s1"}"#;

    /// The archive holds the first day's records up to the malformed line;
    /// the live file the rest. `s1` and `s2` interleave across both.
    pub fn split() -> (Vec<&'static str>, Vec<&'static str>) {
        let archive = vec![S1[0], S2[0], MALFORMED, S1[1], S1[2], UNKNOWN_KIND];
        let live = vec![S1[3], S1[4], S2[1], S2[2]];
        (archive, live)
    }

    /// Write the fixture into `dir`: a gzip archive and a live file.
    pub fn write(dir: &std::path::Path) {
        let (archive, live) = split();
        let day = agent_fmt::when::parse_utc_iso("2026-09-01T23:00:00Z").unwrap();
        crate::event_archive::append(dir, crate::event_archive::DECISIONS, day, (archive.join("\n") + "\n").as_bytes()).unwrap();
        std::fs::write(dir.join("decisions.jsonl"), live.join("\n") + "\n").unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ways-decisions-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn grouped(records: Vec<Record>) -> Vec<Grouped> {
        let mut out = Vec::new();
        group(records, |g| out.push(g));
        out
    }

    #[test]
    fn reads_the_archive_then_the_live_file_and_skips_what_it_cannot_parse() {
        let d = dir("read");
        fixture::write(&d);
        let sources = sources_in(&d);
        assert_eq!(sources.len(), 2, "one archive and the live file: {sources:?}");
        assert!(sources[0].to_string_lossy().ends_with(".jsonl.gz"));
        let mut reader = Records::new(sources, Window::all());
        let ts: Vec<String> = reader.by_ref().map(|r| r.ts().to_string()).collect();
        assert_eq!(reader.skipped(), 2, "the malformed line and the unknown kind");
        assert_eq!(
            ts,
            [
                "2026-09-01T10:00:00Z",
                "2026-09-01T10:00:30Z",
                "2026-09-01T10:00:05Z",
                "2026-09-01T10:01:00Z",
                "2026-09-01T10:02:00Z",
                "2026-09-01T11:00:00Z",
                "2026-09-02T09:00:00Z",
                "2026-09-02T09:00:10Z",
            ],
            "archive lines first, in file order"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn typed_fields_come_through_and_unknown_fields_are_ignored() {
        let Some(Record::Scan(s)) = parse_line(fixture::S1[0]) else { panic!("a scan") };
        assert_eq!((s.scan_id.as_str(), s.epoch, s.turn_start), ("a1", 3, true));
        assert_eq!(s.outcomes[0].verdict.as_deref(), Some("pass"));
        assert_eq!(s.outcomes[1].shortfall, Some(0.04));
        assert_eq!(s.judge.unwrap().status, "judged");
        let Some(Record::Pull(p)) = parse_line(fixture::S2[1]) else { panic!("a pull") };
        assert_eq!((p.way.as_str(), p.scan_id), ("d/c", None));
        assert!(parse_line(fixture::MALFORMED).is_none());
        assert!(parse_line(fixture::UNKNOWN_KIND).is_none());
    }

    #[test]
    fn the_window_bounds_both_ends_by_ts() {
        let d = dir("window");
        fixture::write(&d);
        let w = Window { since: Some("2026-09-01T10:00:30Z".into()), until: Some("2026-09-01T11:00:00Z".into()) };
        let n = Records::new(sources_in(&d), w).count();
        assert_eq!(n, 4, "b1, two pulls and a3; a2 is before since, though later in the file");
        let _ = std::fs::remove_dir_all(&d);
    }

    fn turns_of(out: &[Grouped]) -> Vec<(&str, Vec<&str>)> {
        out.iter()
            .filter_map(|g| if let Grouped::Turn(t) = g { Some((t.start.scan_id.as_str(), t.scans.iter().map(|s| s.scan_id.as_str()).collect())) } else { None })
            .collect()
    }

    fn joins_of(out: &[Grouped]) -> Vec<(&str, &str, Join)> {
        out.iter()
            .filter_map(|g| if let Grouped::Pull(j) = g { Some((j.pull.agent.as_str(), j.pull.way.as_str(), j.join)) } else { None })
            .collect()
    }

    fn lines(ls: &[String]) -> Vec<Record> {
        ls.iter().map(|l| parse_line(l).unwrap_or_else(|| panic!("{l}"))).collect()
    }

    fn scan(ts: &str, id: &str, agent: &str, start: bool, outcomes: &[(&str, &str)]) -> String {
        let o: Vec<String> = outcomes.iter().map(|(w, r)| format!(r#"{{"way":"{w}","result":"{r}"}}"#)).collect();
        format!(r#"{{"ts":"{ts}","kind":"scan","scan_id":"{id}","session":"s","agent":"{agent}","turn_start":{start},"project":"/p","outcomes":[{}]}}"#, o.join(","))
    }

    fn pull(ts: &str, agent: &str, way: &str, scan_id: &str) -> String {
        format!(r#"{{"ts":"{ts}","kind":"pull","session":"s","agent":"{agent}","way":"{way}","stamped":true,"scan_id":"{scan_id}"}}"#)
    }

    #[test]
    fn turns_open_on_turn_start_and_pulls_are_judged_as_they_arrive() {
        let (archive, live) = fixture::split();
        let records: Vec<Record> = archive.into_iter().chain(live).filter_map(parse_line).collect();
        let out = grouped(records);
        assert_eq!(turns_of(&out), [("a1", vec!["a2"]), ("b1", vec![]), ("a3", vec![])], "three turns, though s1's two turns share epoch 3");
        assert_eq!(
            joins_of(&out),
            [
                ("main", "d/a", Join::Delivered),
                ("main", "d/x", Join::RecallMiss),
                ("main", "d/c", Join::NoScanId),
                ("main", "d/a", Join::RecallMiss),
            ],
            "a1 fired d/a; nothing in s1's turn fired d/x; s2's d/a was judge-blocked"
        );
    }

    #[test]
    fn a_continuing_scan_with_no_turn_in_its_session_is_an_orphan() {
        let out = grouped(vec![parse_line(fixture::S1[1]).unwrap()]);
        assert!(matches!(&out[..], [Grouped::Orphan(s)] if s.scan_id == "a2"));
    }

    /// m1 fires d/a and its task scan t1 stashes d/y for sub1. main's next
    /// prompt m2 closes the turn before sub1, running in the background, pulls.
    fn late_session() -> Vec<String> {
        vec![
            scan("2026-09-01T10:00:00Z", "m1", "main", true, &[("d/a", "fired")]),
            scan("2026-09-01T10:00:10Z", "t1", "main", false, &[("d/y", "stashed")]),
            scan("2026-09-01T10:01:00Z", "m2", "main", true, &[]),
            pull("2026-09-01T10:02:00Z", "sub1", "d/y", "t1"),
            pull("2026-09-01T10:02:10Z", "sub1", "d/a", "t1"),
            pull("2026-09-01T10:02:20Z", "main", "d/y", "m2"),
        ]
    }

    #[test]
    fn a_late_subagent_pull_joins_the_dispatch_scan_after_the_next_prompt() {
        let out = grouped(lines(&late_session()));
        assert_eq!(turns_of(&out), [("m1", vec!["t1"]), ("m2", vec![])]);
        let joined: Vec<(&str, Option<u32>)> = out.iter().filter_map(|g| if let Grouped::Pull(j) = g { Some((j.pull.way.as_str(), j.turn)) } else { None }).collect();
        assert_eq!(joined, [("d/y", Some(0)), ("d/a", Some(0)), ("d/y", Some(1))], "sub1's pulls join m1's turn though m2 closed it");
    }

    #[test]
    fn delivered_is_judged_per_context() {
        let mut ls = late_session();
        ls.push(pull("2026-09-01T10:00:20Z", "main", "d/y", "m1"));
        let out = grouped(lines(&ls));
        assert_eq!(
            joins_of(&out),
            [
                ("sub1", "d/y", Join::Delivered),
                ("sub1", "d/a", Join::RecallMiss),
                ("main", "d/y", Join::RecallMiss),
                ("main", "d/y", Join::RecallMiss),
            ],
            "sub1 was stashed d/y, not d/a, which only main had; main was never given d/y, stashed only for sub1"
        );
    }

    #[test]
    fn a_main_pull_counts_only_what_its_turn_delivered_before_it() {
        let ls = vec![
            scan("2026-09-01T10:00:00Z", "m1", "main", true, &[]),
            pull("2026-09-01T10:00:05Z", "main", "d/a", "m1"),
            scan("2026-09-01T10:00:10Z", "m1b", "main", false, &[("d/a", "fired")]),
            pull("2026-09-01T10:00:20Z", "main", "d/a", "m1"),
        ];
        assert_eq!(joins_of(&grouped(lines(&ls))), [("main", "d/a", Join::RecallMiss), ("main", "d/a", Join::Delivered)]);
    }

    #[test]
    fn a_nested_dispatch_is_indexed_and_its_subagent_pull_joins_it() {
        let ls = vec![
            scan("2026-09-01T10:00:00Z", "m1", "main", true, &[("d/a", "fired")]),
            scan("2026-09-01T10:00:10Z", "t1", "main", false, &[("d/y", "stashed")]),
            scan("2026-09-01T10:00:30Z", "t2", "sub1", false, &[("d/z", "stashed")]),
            pull("2026-09-01T10:00:40Z", "sub2", "d/z", "t2"),
            pull("2026-09-01T10:00:50Z", "sub2", "d/y", "t2"),
            pull("2026-09-01T10:00:55Z", "sub1", "d/a", "t2"),
        ];
        let out = grouped(lines(&ls));
        assert_eq!(turns_of(&out), [("m1", vec!["t1", "t2"])], "sub1's dispatch belongs to the session's turn, not an orphan");
        assert_eq!(
            joins_of(&out),
            [("sub2", "d/z", Join::Delivered), ("sub2", "d/y", Join::RecallMiss), ("sub1", "d/a", Join::RecallMiss)],
            "sub1's own pull naming t2 reads t2, not main's turn it was counted with"
        );
    }

    #[test]
    fn a_main_scan_never_joins_a_teammates_turn() {
        // Main's turn began before the window; teammate `mate` (same session)
        // has a turn open when main's queued-message scan q1 arrives.
        let ls = vec![
            scan("2026-09-01T10:00:00Z", "tm", "mate", true, &[("d/b", "fired")]),
            scan("2026-09-01T10:00:10Z", "q1", "main", false, &[("d/q", "fired")]),
            pull("2026-09-01T10:00:20Z", "main", "d/q", "q1"),
            pull("2026-09-01T10:00:30Z", "main", "d/b", "q1"),
        ];
        let out = grouped(lines(&ls));
        assert_eq!(turns_of(&out), [("tm", vec![])], "q1 is main's, not the teammate's");
        assert!(out.iter().any(|g| matches!(g, Grouped::Orphan(s) if s.scan_id == "q1")));
        assert_eq!(joins_of(&out), [("main", "d/q", Join::Delivered), ("main", "d/b", Join::RecallMiss)], "judged against main's own scan");
    }

    #[test]
    fn a_future_dated_record_does_not_hand_on_every_open_turn() {
        let ls = vec![
            scan("2026-09-01T10:00:00Z", "m1", "main", true, &[]),
            r#"{"ts":"2027-01-01T00:00:00Z","kind":"scan","scan_id":"f1","session":"f","agent":"main","turn_start":true,"project":"/p","outcomes":[]}"#.to_string(),
            pull("2026-09-01T10:00:20Z", "main", "d/a", "m1"),
        ];
        let out = grouped(lines(&ls));
        let order: Vec<&str> = out.iter().map(|g| match g { Grouped::Turn(t) => t.start.scan_id.as_str(), Grouped::Pull(_) => "pull", Grouped::Orphan(_) => "?" }).collect();
        assert_eq!(order, ["pull", "m1", "f1"], "m1 stays open past the future-dated f1, which is still read");
    }

    #[test]
    fn a_real_gap_in_the_log_is_confirmed_by_the_next_record() {
        let other = |ts: &str, id: &str| format!(r#"{{"ts":"{ts}","kind":"scan","scan_id":"{id}","session":"{id}","agent":"main","turn_start":true,"project":"/p","outcomes":[]}}"#);
        let ls = vec![
            scan("2026-09-01T10:00:00Z", "m1", "main", true, &[]),
            other("2026-09-03T10:00:00Z", "x1"),
            other("2026-09-03T10:01:00Z", "y1"),
            pull("2026-09-03T10:02:00Z", "main", "d/a", "m1"),
        ];
        let out = grouped(lines(&ls));
        let order: Vec<&str> = out.iter().map(|g| match g { Grouped::Turn(t) => t.start.scan_id.as_str(), Grouped::Pull(_) => "pull", Grouped::Orphan(_) => "?" }).collect();
        assert_eq!(order, ["m1", "pull", "x1", "y1"], "y1 confirms the two-day gap and m1 is handed on");
    }

    #[test]
    fn an_idle_open_turn_is_handed_on_after_a_day() {
        let ls = vec![
            scan("2026-09-01T10:00:00Z", "m1", "main", true, &[]),
            r#"{"ts":"2026-09-02T11:00:00Z","kind":"scan","scan_id":"x1","session":"other","agent":"main","turn_start":true,"project":"/p","outcomes":[]}"#.to_string(),
            r#"{"ts":"2026-09-02T11:00:01Z","kind":"scan","scan_id":"x2","session":"other","agent":"main","turn_start":true,"project":"/p","outcomes":[]}"#.to_string(),
        ];
        let out = grouped(lines(&ls));
        let order: Vec<&str> = out.iter().map(|g| if let Grouped::Turn(t) = g { t.start.scan_id.as_str() } else { "?" }).collect();
        assert_eq!(order, ["m1", "x1", "x2"], "m1 is handed on as x1 arrives 25h later, before x2 closes x1");
    }
}
