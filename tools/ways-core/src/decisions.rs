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
//! stream into turns as it goes.
//!
//! A turn is opened by a scan with `turn_start: true` and keyed by
//! `(session, agent)`. The epoch is not a turn index: the command and file
//! lanes bump it on every tool call. Later scans of the same session and agent
//! with `turn_start: false` belong to the open turn, and a pull belongs to the
//! turn holding the scan its `scan_id` names.

use serde::Deserialize;
use std::collections::HashMap;
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
    crate::paths::decisions_log_sources()
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

/// A streaming reader over decision-log sources. Holds one source's text at a
/// time; an unreadable source is skipped.
pub struct Records {
    pending: std::vec::IntoIter<PathBuf>,
    text: String,
    pos: usize,
    window: Window,
    skipped: usize,
}

impl Records {
    /// Records of `sources`, in order, that fall in `window`.
    pub fn new(sources: Vec<PathBuf>, window: Window) -> Self {
        Records { pending: sources.into_iter(), text: String::new(), pos: 0, window, skipped: 0 }
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
}

impl Iterator for Records {
    type Item = Record;

    fn next(&mut self) -> Option<Record> {
        loop {
            if self.pos >= self.text.len() {
                let path = self.pending.next()?;
                self.text = event_archive::read_source(&path).unwrap_or_default();
                self.pos = 0;
                continue;
            }
            let rest = &self.text[self.pos..];
            let end = rest.find('\n').map_or(rest.len(), |i| i + 1);
            let line = &rest[..end];
            self.pos += end;
            if line.trim().is_empty() {
                continue;
            }
            match parse_line(line) {
                Some(r) if self.window.contains(r.ts()) => return Some(r),
                Some(_) => {}
                None => self.skipped += 1,
            }
        }
    }
}

/// One turn: the scan that opened it, the scans of the same session and agent
/// that followed it, and the pulls that joined one of its scans.
#[derive(Clone, Debug, PartialEq)]
pub struct Turn {
    pub start: Scan,
    /// Later scans of the turn, `turn_start: false`, in log order.
    pub scans: Vec<Scan>,
    pub pulls: Vec<Pull>,
}

impl Turn {
    /// Every scan of the turn, the opening one first.
    pub fn all_scans(&self) -> impl Iterator<Item = &Scan> {
        std::iter::once(&self.start).chain(&self.scans)
    }

    /// Whether any scan of the turn fired or re-disclosed `way`.
    pub fn delivered(&self, way: &str) -> bool {
        self.all_scans().flat_map(|s| &s.outcomes).any(|o| o.way == way && matches!(o.result.as_str(), "fired" | "redisclosed"))
    }
}

/// What [`Turns`] hands on.
#[derive(Clone, Debug, PartialEq)]
pub enum Grouped {
    /// A turn, complete: its session and agent started another, or the stream
    /// ended.
    Turn(Turn),
    /// A `turn_start: false` scan with no open turn for its session and
    /// agent, such as one whose turn began before the window.
    Orphan(Scan),
    /// A pull whose `scan_id` is null, or names no scan of an open turn.
    Unjoined(Pull),
}

/// Groups a record stream into turns as it arrives. Holds only the open turn
/// of each `(session, agent)` and an index of their scan ids, so a closed turn
/// is handed on and dropped.
#[derive(Default)]
pub struct Turns {
    open: HashMap<(String, String), Turn>,
    by_scan: HashMap<String, (String, String)>,
}

impl Turns {
    pub fn new() -> Self {
        Turns::default()
    }

    /// Take one record, handing on whatever it completes.
    pub fn push(&mut self, record: Record, emit: &mut impl FnMut(Grouped)) {
        match record {
            Record::Scan(scan) => {
                let scan = *scan;
                let key = (scan.session.clone(), scan.agent.clone());
                if scan.turn_start {
                    if let Some(done) = self.open.remove(&key) {
                        self.close(done, emit);
                    }
                    if !scan.scan_id.is_empty() {
                        self.by_scan.insert(scan.scan_id.clone(), key.clone());
                    }
                    self.open.insert(key, Turn { start: scan, scans: Vec::new(), pulls: Vec::new() });
                } else if let Some(turn) = self.open.get_mut(&key) {
                    if !scan.scan_id.is_empty() {
                        self.by_scan.insert(scan.scan_id.clone(), key);
                    }
                    turn.scans.push(scan);
                } else {
                    emit(Grouped::Orphan(scan));
                }
            }
            Record::Pull(pull) => {
                let turn = pull.scan_id.as_ref().and_then(|id| self.by_scan.get(id)).and_then(|key| self.open.get_mut(key));
                match turn {
                    Some(t) => t.pulls.push(pull),
                    None => emit(Grouped::Unjoined(pull)),
                }
            }
        }
    }

    /// Hand on every turn still open, oldest start first.
    pub fn finish(mut self, emit: &mut impl FnMut(Grouped)) {
        let mut rest: Vec<Turn> = self.open.drain().map(|(_, t)| t).collect();
        rest.sort_by(|a, b| a.start.ts.cmp(&b.start.ts).then_with(|| a.start.scan_id.cmp(&b.start.scan_id)));
        for t in rest {
            emit(Grouped::Turn(t));
        }
    }

    fn close(&mut self, turn: Turn, emit: &mut impl FnMut(Grouped)) {
        for s in turn.all_scans() {
            self.by_scan.remove(&s.scan_id);
        }
        emit(Grouped::Turn(turn));
    }
}

/// Group a record stream into turns, handing each on as it completes.
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

    #[test]
    fn turns_open_on_turn_start_and_collect_their_scans_and_pulls() {
        let (archive, live) = fixture::split();
        let records: Vec<Record> = archive.into_iter().chain(live).filter_map(parse_line).collect();
        let out = grouped(records);
        let turns: Vec<&Turn> = out.iter().filter_map(|g| if let Grouped::Turn(t) = g { Some(t) } else { None }).collect();
        let shape: Vec<(&str, Vec<&str>, Vec<&str>)> = turns
            .iter()
            .map(|t| (t.start.scan_id.as_str(), t.scans.iter().map(|s| s.scan_id.as_str()).collect(), t.pulls.iter().map(|p| p.way.as_str()).collect()))
            .collect();
        assert_eq!(
            shape,
            [("a1", vec!["a2"], vec!["d/a", "d/x"]), ("b1", vec![], vec!["d/a"]), ("a3", vec![], vec![])],
            "three turns, though s1's two turns share epoch 3"
        );
        assert!(turns[0].delivered("d/a") && !turns[0].delivered("d/x"));
        let unjoined: Vec<&str> = out.iter().filter_map(|g| if let Grouped::Unjoined(p) = g { Some(p.way.as_str()) } else { None }).collect();
        assert_eq!(unjoined, ["d/c"], "the null scan_id pull");
    }

    #[test]
    fn a_continuing_scan_with_no_open_turn_is_an_orphan() {
        let out = grouped(vec![parse_line(fixture::S1[1]).unwrap()]);
        assert!(matches!(&out[..], [Grouped::Orphan(s)] if s.scan_id == "a2"));
    }
}
