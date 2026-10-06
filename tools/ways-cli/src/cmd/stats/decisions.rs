//! The decisions section of `ways tune stats`: turns, outcomes, the judge and
//! pulls, read from the decision log (ADR-701 §2, §8).
//!
//! The section streams the log through [`ways_core::decisions`] and aggregates
//! as it goes; it never holds more than the open turns. Turns are counted by
//! `turn_start` scans, and a pull joins its turn through `scan_id`.

use agent_fmt::{Align, Table};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use ways_core::decisions::{group, Grouped, Join, Joined, Records, Scan, Turn, Window};

const DAY_SECS: u64 = 86_400;

/// Outcome results the section names, in column order. Any other result is
/// still counted under its own name in the JSON.
pub(crate) const RESULTS: [&str; 10] = [
    "fired",
    "redisclosed",
    "held_refire",
    "held_context_cap",
    "keyword_gated",
    "near_miss",
    "judge_block",
    "ancestor_block",
    "withheld_for_parent",
    "stashed",
];

/// The human table's short headers for [`RESULTS`].
const SHORT: [&str; 10] = ["fired", "redisc", "refire", "ctxcap", "kwgate", "near", "jblock", "ablock", "withheld", "stashed"];

/// The reason a pull that still served the way carries: nothing was stamped
/// only because the way has no refire curve.
const SERVED_UNSTAMPED: &str = "no refire curve";

/// Ways shown in the near-miss leaders and the human outcome table.
const TOP: usize = 10;

/// Turns per calendar day over the window.
#[derive(Default, Clone, PartialEq, Debug)]
pub(crate) struct TurnsPerDay {
    pub(crate) days: u32,
    pub(crate) mean: f64,
    pub(crate) median: f64,
    pub(crate) p90: u32,
}

/// How the relevance gate ran across the window's scans.
#[derive(Default, Clone, PartialEq, Debug)]
pub(crate) struct JudgeTally {
    pub(crate) judged_scans: u32,
    pub(crate) fallback_scans: u32,
    /// Scans where the cap left ways unjudged, and how many ways in all.
    pub(crate) capped_scans: u32,
    pub(crate) capped_ways: u32,
    /// Verdicts on ways the judge saw: passes and shadow would-blocks ride on
    /// the outcome's `verdict`; an enforced block is a `judge_block` result.
    pub(crate) pass: u32,
    pub(crate) block: u32,
    pub(crate) would_block: u32,
}

impl JudgeTally {
    fn verdicts(&self) -> u32 {
        self.pass + self.block + self.would_block
    }

    fn rate(&self, n: u32) -> f64 {
        let total = self.verdicts();
        if total == 0 { 0.0 } else { f64::from(n) / f64::from(total) }
    }
}

/// What the window's pulls were. A served pull is a recall miss when its
/// context had not been given the way ([`Join`]); it is out of band when it
/// came inside the way's refire window. The two overlap.
#[derive(Default, Clone, PartialEq, Debug)]
pub(crate) struct PullTally {
    pub(crate) total: u32,
    /// Refused before anything was served: invalid id, disabled, not found.
    pub(crate) refused: u32,
    pub(crate) null_scan_id: u32,
    /// A `scan_id` that names no scan in the window.
    pub(crate) unjoined: u32,
    pub(crate) recall_miss: u32,
    /// The pull's turn had already delivered the way.
    pub(crate) delivered: u32,
    pub(crate) out_of_band: u32,
    pub(crate) recall_miss_by_way: Vec<(String, u32)>,
    pub(crate) out_of_band_by_way: Vec<(String, u32)>,
}

/// The decisions section, computed once for both renderers.
#[derive(Default, Clone, PartialEq, Debug)]
pub(crate) struct DecisionsReport {
    pub(crate) records: u32,
    /// Lines skipped as malformed or of an unknown kind.
    pub(crate) skipped: u32,
    pub(crate) turns: u32,
    pub(crate) turns_per_day: TurnsPerDay,
    pub(crate) scans: u32,
    /// Continuing scans with no open turn, such as one whose turn began
    /// before the window.
    pub(crate) orphan_scans: u32,
    /// Per way, outcome counts by result. Most outcomes first, ties by name.
    pub(crate) by_way: Vec<(String, BTreeMap<String, u32>)>,
    pub(crate) judge: JudgeTally,
    /// Ways by near-miss count with their mean shortfall, most first.
    pub(crate) near_miss_leaders: Vec<(String, u32, f64)>,
    pub(crate) pulls: PullTally,
}

/// Running totals while the stream is read.
#[derive(Default)]
struct Acc<'f> {
    project: Option<&'f str>,
    by_day: BTreeMap<String, u32>,
    turns: u32,
    scans: u32,
    orphan_scans: u32,
    by_way: HashMap<String, BTreeMap<String, u32>>,
    judge: JudgeTally,
    near: HashMap<String, (u32, f64, u32)>,
    pulls: PullTally,
    miss_by_way: HashMap<String, u32>,
    oob_by_way: HashMap<String, u32>,
}

impl Acc<'_> {
    fn in_project(&self, project: &str) -> bool {
        self.project.is_none_or(|pf| ways_core::util::in_project(project, pf))
    }

    fn scan(&mut self, s: &Scan) {
        if !self.in_project(&s.project) {
            return;
        }
        self.scans += 1;
        for o in &s.outcomes {
            *self.by_way.entry(o.way.clone()).or_default().entry(o.result.clone()).or_insert(0) += 1;
            match o.verdict.as_deref() {
                Some("pass") => self.judge.pass += 1,
                Some("would_block") => self.judge.would_block += 1,
                Some("block") => self.judge.block += 1,
                _ if o.result == "judge_block" => self.judge.block += 1,
                _ => {}
            }
            if o.result == "near_miss" {
                let e = self.near.entry(o.way.clone()).or_default();
                e.0 += 1;
                if let Some(sf) = o.shortfall {
                    e.1 += sf;
                    e.2 += 1;
                }
            }
        }
        if let Some(j) = &s.judge {
            match j.status.as_str() {
                "judged" => self.judge.judged_scans += 1,
                "fallback" => self.judge.fallback_scans += 1,
                _ => {}
            }
            if !j.capped.is_empty() {
                self.judge.capped_scans += 1;
                self.judge.capped_ways += j.capped.len() as u32;
            }
        }
    }

    fn turn(&mut self, t: &Turn) {
        if !self.in_project(&t.start.project) {
            return;
        }
        self.turns += 1;
        // A turn whose ts carries no date is counted, but has no day.
        if let Some(day) = t.start.ts.get(..10).filter(|d| agent_fmt::when::parse_utc_iso(&format!("{d}T00:00:00Z")).is_some()) {
            *self.by_day.entry(day.to_string()).or_insert(0) += 1;
        }
        for s in t.all_scans() {
            self.scan(s);
        }
    }

    /// A pull, as its scan judged it. Its project is the named scan's, so a
    /// project filter leaves out a pull that named none.
    fn pull(&mut self, j: &Joined) {
        if self.project.is_some() && !j.project.as_deref().is_some_and(|p| self.in_project(p)) {
            return;
        }
        let p = &j.pull;
        self.pulls.total += 1;
        if p.reason.as_deref().is_some_and(|r| r != SERVED_UNSTAMPED) {
            self.pulls.refused += 1;
            return;
        }
        if p.out_of_band {
            self.pulls.out_of_band += 1;
            *self.oob_by_way.entry(p.way.clone()).or_insert(0) += 1;
        }
        match j.join {
            Join::Delivered => self.pulls.delivered += 1,
            Join::RecallMiss => {
                self.pulls.recall_miss += 1;
                *self.miss_by_way.entry(p.way.clone()).or_insert(0) += 1;
            }
            Join::NoScanId => self.pulls.null_scan_id += 1,
            Join::Unknown => self.pulls.unjoined += 1,
        }
    }

    fn finish(self, span: Option<(u64, u64)>, records: u32, skipped: u32) -> DecisionsReport {
        let mut by_way: Vec<(String, BTreeMap<String, u32>)> = self.by_way.into_iter().collect();
        by_way.sort_by(|a, b| total(&b.1).cmp(&total(&a.1)).then_with(|| a.0.cmp(&b.0)));
        let mut near: Vec<(String, u32, f64)> =
            self.near.into_iter().map(|(w, (n, sum, with))| (w, n, if with == 0 { 0.0 } else { sum / f64::from(with) })).collect();
        near.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        near.truncate(TOP);
        let mut pulls = self.pulls;
        pulls.recall_miss_by_way = super::ranked(self.miss_by_way.iter().map(|(k, v)| (k.as_str(), *v)).collect());
        pulls.out_of_band_by_way = super::ranked(self.oob_by_way.iter().map(|(k, v)| (k.as_str(), *v)).collect());
        DecisionsReport {
            records,
            skipped,
            turns: self.turns,
            turns_per_day: turns_per_day(&self.by_day, span),
            scans: self.scans,
            orphan_scans: self.orphan_scans,
            by_way,
            judge: self.judge,
            near_miss_leaders: near,
            pulls,
        }
    }
}

fn total(m: &BTreeMap<String, u32>) -> u32 {
    m.values().sum()
}

/// Turns per day over `span` (first and last second, inclusive), or over the
/// first to the last day that had a turn. Days with no turn count as zero.
fn turns_per_day(by_day: &BTreeMap<String, u32>, span: Option<(u64, u64)>) -> TurnsPerDay {
    let day_secs = |d: &str| agent_fmt::when::parse_utc_iso(&format!("{d}T00:00:00Z"));
    let span = span.or_else(|| Some((day_secs(by_day.keys().next()?)?, day_secs(by_day.keys().next_back()?)?)));
    let Some((first, last)) = span else { return TurnsPerDay::default() };
    let mut counts: Vec<u32> = Vec::new();
    let mut day = first / DAY_SECS * DAY_SECS;
    while day <= last {
        counts.push(by_day.get(&agent_fmt::when::utc_date(day)).copied().unwrap_or(0));
        day += DAY_SECS;
    }
    if counts.is_empty() {
        return TurnsPerDay::default();
    }
    let n = counts.len();
    let mean = f64::from(counts.iter().sum::<u32>()) / n as f64;
    counts.sort_unstable();
    let median = if n % 2 == 1 { f64::from(counts[n / 2]) } else { f64::from(counts[n / 2 - 1] + counts[n / 2]) / 2.0 };
    // Nearest rank.
    let p90 = counts[(n * 9).div_ceil(10).max(1) - 1];
    TurnsPerDay { days: n as u32, mean, median, p90 }
}

/// The window and calendar for `--days d` at Unix second `now`: the `d`
/// calendar days ending today, from the start of the first through `now`, so
/// every turn the window counts has a day on the calendar. Without `days`,
/// every record, and a calendar spanning the days that had a turn.
pub(crate) fn days_window(days: Option<u32>, now: u64) -> (Window, Option<(u64, u64)>) {
    let Some(d) = days else { return (Window::all(), None) };
    let start = (now / DAY_SECS).saturating_sub(u64::from(d.max(1)) - 1) * DAY_SECS;
    (Window { since: Some(agent_fmt::when::utc_iso(start)), until: None }, Some((start, now)))
}

/// Aggregate the decision records of `sources` in `window`. `span` bounds the
/// turns-per-day calendar; `None` spans the days that had a turn.
pub(crate) fn report(sources: Vec<PathBuf>, window: Window, project: Option<&str>, span: Option<(u64, u64)>) -> DecisionsReport {
    let mut records = Records::new(sources, window);
    let mut acc = Acc { project, ..Default::default() };
    let mut n = 0u32;
    group(
        std::iter::from_fn(|| {
            let r = records.next();
            n += u32::from(r.is_some());
            r
        }),
        |g| match g {
            Grouped::Turn(t) => acc.turn(&t),
            Grouped::Orphan(s) => {
                if acc.in_project(&s.project) {
                    acc.orphan_scans += 1;
                }
                acc.scan(&s);
            }
            Grouped::Pull(j) => acc.pull(&j),
        },
    );
    let skipped = records.skipped() as u32;
    acc.finish(span, n, skipped)
}

/// The section as JSON; `None` (no decision log) is an explicit absence.
pub(crate) fn json_value(r: Option<&DecisionsReport>) -> Value {
    let Some(r) = r else { return json!({ "present": false }) };
    let by_way: serde_json::Map<String, Value> = r.by_way.iter().map(|(w, m)| (w.clone(), json!(m))).collect();
    let leaders: Vec<Value> =
        r.near_miss_leaders.iter().map(|(w, n, sf)| json!({"way": w, "near_miss": n, "mean_shortfall": round(*sf)})).collect();
    let j = &r.judge;
    let p = &r.pulls;
    json!({
        "present": true,
        "records": r.records,
        "skipped_lines": r.skipped,
        "turns": r.turns,
        "turns_per_day": {
            "days": r.turns_per_day.days,
            "mean": round(r.turns_per_day.mean),
            "median": r.turns_per_day.median,
            "p90": r.turns_per_day.p90,
        },
        "scans": r.scans,
        "orphan_scans": r.orphan_scans,
        "by_way": by_way,
        "judge": {
            "judged_scans": j.judged_scans,
            "fallback_scans": j.fallback_scans,
            "capped_scans": j.capped_scans,
            "capped_ways": j.capped_ways,
            "verdicts": j.verdicts(),
            "pass": j.pass,
            "block": j.block,
            "would_block": j.would_block,
            "pass_rate": round(j.rate(j.pass)),
            "block_rate": round(j.rate(j.block)),
            "would_block_rate": round(j.rate(j.would_block)),
        },
        "near_miss_leaders": leaders,
        "pulls": {
            "total": p.total,
            "refused": p.refused,
            "null_scan_id": p.null_scan_id,
            "unjoined": p.unjoined,
            "recall_miss": p.recall_miss,
            "delivered": p.delivered,
            "out_of_band": p.out_of_band,
            "recall_miss_by_way": super::counts_json(&p.recall_miss_by_way),
            "out_of_band_by_way": super::counts_json(&p.out_of_band_by_way),
        },
    })
}

fn round(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

/// Print the section; `None` is one line saying there is no log.
pub(crate) fn print_human(r: Option<&DecisionsReport>) {
    let Some(r) = r else {
        println!("{}", ABSENT);
        return;
    };
    for line in head_lines(r) {
        println!("{line}");
    }
    if !r.by_way.is_empty() {
        println!("Outcomes by way:");
        let mut headers = vec!["Way"];
        headers.extend(SHORT);
        let mut t = Table::new(&headers);
        t.no_auto_fit();
        for col in 1..headers.len() {
            t.align(col, Align::Right);
        }
        for (way, m) in r.by_way.iter().take(TOP) {
            let mut row = vec![way.clone()];
            row.extend(RESULTS.iter().map(|k| m.get(*k).map_or("-".to_string(), u32::to_string)));
            t.add_owned(row);
        }
        t.print();
        if r.by_way.len() > TOP {
            println!("  … {} more ways (--json lists all)", r.by_way.len() - TOP);
        }
        println!();
    }
    for line in tail_lines(r) {
        println!("{line}");
    }
}

/// The one line printed when there is no decision log.
const ABSENT: &str = "Decisions: no decision log yet.";

/// The lines above the outcome table: turns and scans.
fn head_lines(r: &DecisionsReport) -> Vec<String> {
    let d = &r.turns_per_day;
    let mut scans = format!("  Scans: {}", r.scans);
    if r.orphan_scans > 0 {
        scans += &format!(" ({} outside a turn)", r.orphan_scans);
    }
    if r.skipped > 0 {
        scans += &format!("  |  skipped lines: {}", r.skipped);
    }
    vec![
        "Decisions (from the decision log):".to_string(),
        format!(
            "  Turns: {}  |  per day over {} day{}: mean {:.1}, median {:.1}, p90 {}",
            r.turns,
            d.days,
            if d.days == 1 { "" } else { "s" },
            d.mean,
            d.median,
            d.p90
        ),
        scans,
        String::new(),
    ]
}

/// The lines below the outcome table: the judge, near misses and pulls.
fn tail_lines(r: &DecisionsReport) -> Vec<String> {
    let j = &r.judge;
    let mut out = vec![
        format!(
            "Judge: {} verdicts  |  pass {:.0}%  block {:.0}%  would_block {:.0}%  |  judged scans {}, fallback {}, capped {} ({} ways)",
            j.verdicts(),
            100.0 * j.rate(j.pass),
            100.0 * j.rate(j.block),
            100.0 * j.rate(j.would_block),
            j.judged_scans,
            j.fallback_scans,
            j.capped_scans,
            j.capped_ways
        ),
        String::new(),
    ];
    if !r.near_miss_leaders.is_empty() {
        out.push("Near-miss leaders:".into());
        for (way, n, sf) in &r.near_miss_leaders {
            out.push(format!("  {way:<30} {n:>3}  mean shortfall {sf:.3}"));
        }
        out.push(String::new());
    }
    let p = &r.pulls;
    out.push(format!(
        "Pulls: {}  |  recall misses {}  |  out-of-band {}  |  already delivered {}  |  null scan_id {}  |  unjoined {}  |  refused {}",
        p.total, p.recall_miss, p.out_of_band, p.delivered, p.null_scan_id, p.unjoined, p.refused
    ));
    for (label, rows) in [("recall misses", &p.recall_miss_by_way), ("out-of-band", &p.out_of_band_by_way)] {
        if !rows.is_empty() {
            let list: Vec<String> = rows.iter().take(TOP).map(|(w, n)| format!("{w} {n}")).collect();
            out.push(format!("  {label}: {}", list.join(", ")));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Session `s1` (project `/p`) and `s2` (project `/q`) interleave across a
    /// gzip archive and the live file. `s1`'s two turns share epoch 3 and
    /// `s2`'s share epoch 1, and every pull's epoch differs from its turn's.
    const ARCHIVE: &[&str] = &[
        r#"{"ts":"2026-09-01T10:00:00Z","kind":"scan","scan_id":"a1","session":"s1","agent":"main","epoch":3,"turn_start":true,"surface":"prompt","project":"/p","outcomes":[{"way":"d/a","result":"fired","verdict":"pass","p_yes":0.9},{"way":"d/n","result":"near_miss","shortfall":0.04},{"way":"d/k","result":"keyword_gated"},{"way":"d/h","result":"held_context_cap"}],"judge":{"status":"judged"}}"#,
        r#"{"ts":"2026-09-01T10:00:30Z","kind":"scan","scan_id":"b1","session":"s2","agent":"main","epoch":1,"turn_start":true,"surface":"prompt","project":"/q","outcomes":[{"way":"d/a","result":"judge_block","p_yes":0.05},{"way":"d/n","result":"near_miss","shortfall":0.02},{"way":"d/a/c","result":"ancestor_block","ancestor":"d/a"},{"way":"d/w","result":"fired","verdict":"would_block"}],"judge":{"status":"judged"}}"#,
        r#"{"ts":"2026-09-01T10:00:40Z","kind":"scan","scan_id":"#,
        r#"{"ts":"2026-09-01T10:00:50Z","kind":"scan","scan_id":"a2","session":"s1","agent":"main","epoch":4,"turn_start":false,"surface":"task","project":"/p","outcomes":[{"way":"d/t","result":"stashed"},{"way":"d/p/c","result":"withheld_for_parent"}],"judge":{"status":"fallback","reason":"deadline","capped":["d/z"]}}"#,
        r#"{"ts":"2026-09-01T10:01:00Z","kind":"pull","session":"s1","agent":"main","epoch":5,"way":"d/a","window":"refire","out_of_band":false,"stamped":true,"scan_id":"a1"}"#,
    ];
    const LIVE: &[&str] = &[
        r#"{"ts":"2026-09-01T10:02:00Z","kind":"pull","session":"s1","agent":"main","epoch":6,"way":"d/x","window":"suppressed","out_of_band":true,"stamped":true,"scan_id":"a2"}"#,
        r#"{"ts":"2026-09-01T11:00:00Z","kind":"scan","scan_id":"a3","session":"s1","agent":"main","epoch":3,"turn_start":true,"surface":"prompt","project":"/p","outcomes":[{"way":"d/b","result":"redisclosed"},{"way":"d/r","result":"held_refire"},{"way":"d/n","result":"near_miss","shortfall":0.06}]}"#,
        r#"{"ts":"2026-09-02T09:00:00Z","kind":"pull","session":"s2","agent":"main","epoch":2,"way":"d/c","window":"first_fire","out_of_band":false,"stamped":true,"scan_id":null}"#,
        r#"{"ts":"2026-09-02T09:00:10Z","kind":"pull","session":"s2","agent":"main","epoch":2,"way":"d/a","window":"first_fire","out_of_band":false,"stamped":true,"scan_id":"b1"}"#,
        r#"{"ts":"2026-09-03T08:00:00Z","kind":"scan","scan_id":"b2","session":"s2","agent":"main","epoch":1,"turn_start":true,"surface":"prompt","project":"/q","outcomes":[]}"#,
        r#"{"ts":"2026-09-03T08:00:20Z","kind":"pull","session":"s2","agent":"main","epoch":2,"way":"d/bad","window":"none","out_of_band":false,"stamped":false,"reason":"not found","scan_id":"b2"}"#,
    ];

    fn fixture(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ways-stats-decisions-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let day = agent_fmt::when::parse_utc_iso("2026-09-01T23:00:00Z").unwrap();
        ways_core::event_archive::append(&d, ways_core::event_archive::DECISIONS, day, (ARCHIVE.join("\n") + "\n").as_bytes()).unwrap();
        std::fs::write(d.join("decisions.jsonl"), LIVE.join("\n") + "\n").unwrap();
        d
    }

    fn run(tag: &str, project: Option<&str>) -> DecisionsReport {
        run_window(tag, project, Window::all(), None)
    }

    fn run_window(tag: &str, project: Option<&str>, window: Window, span: Option<(u64, u64)>) -> DecisionsReport {
        let d = fixture(tag);
        let r = report(ways_core::decisions::sources_in(&d), window, project, span);
        let _ = std::fs::remove_dir_all(&d);
        r
    }

    /// A report over `live` alone, as the live file.
    fn run_live(tag: &str, live: &[&str]) -> DecisionsReport {
        let d = std::env::temp_dir().join(format!("ways-stats-decisions-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("decisions.jsonl"), live.join("\n") + "\n").unwrap();
        let r = report(ways_core::decisions::sources_in(&d), Window::all(), None, None);
        let _ = std::fs::remove_dir_all(&d);
        r
    }

    #[test]
    fn a_days_window_starts_where_its_calendar_does() {
        // Two days ending 09-03 09:00: the calendar is 09-02 and 09-03, so the
        // window starts 09-02 00:00 and leaves out s1's turns of 09-01.
        let now = agent_fmt::when::parse_utc_iso("2026-09-03T09:00:00Z").unwrap();
        let (window, span) = days_window(Some(2), now);
        assert_eq!(window.since.as_deref(), Some("2026-09-02T00:00:00Z"));
        let r = run_window("days", None, window, span);
        assert_eq!(r.turns, 1, "b2 only");
        assert_eq!(r.turns_per_day, TurnsPerDay { days: 2, mean: 0.5, median: 0.5, p90: 1 }, "every counted turn is on the calendar");
        let (_, one) = days_window(Some(1), now);
        assert_eq!(one, Some((agent_fmt::when::parse_utc_iso("2026-09-03T00:00:00Z").unwrap(), now)));
    }

    #[test]
    fn a_turn_with_no_ts_is_counted_but_does_not_empty_the_calendar() {
        let live = [
            r#"{"kind":"scan","scan_id":"z0","session":"s0","agent":"main","turn_start":true,"project":"/p","outcomes":[]}"#,
            r#"{"ts":"2026-09-01T10:00:00Z","kind":"scan","scan_id":"z1","session":"s1","agent":"main","turn_start":true,"project":"/p","outcomes":[]}"#,
            r#"{"ts":"2026-09-02T10:00:00Z","kind":"scan","scan_id":"z2","session":"s1","agent":"main","turn_start":true,"project":"/p","outcomes":[]}"#,
        ];
        let r = run_live("nots", &live);
        assert_eq!(r.turns, 3);
        assert_eq!(r.turns_per_day, TurnsPerDay { days: 2, mean: 1.0, median: 1.0, p90: 1 });
    }

    #[test]
    fn a_background_subagent_pull_after_the_next_prompt_is_judged_by_its_dispatch() {
        let live = [
            r#"{"ts":"2026-09-01T10:00:00Z","kind":"scan","scan_id":"m1","session":"s","agent":"main","turn_start":true,"project":"/p","outcomes":[{"way":"d/a","result":"fired"}]}"#,
            r#"{"ts":"2026-09-01T10:00:10Z","kind":"scan","scan_id":"t1","session":"s","agent":"main","turn_start":false,"project":"/p","outcomes":[{"way":"d/y","result":"stashed"}]}"#,
            r#"{"ts":"2026-09-01T10:01:00Z","kind":"scan","scan_id":"m2","session":"s","agent":"main","turn_start":true,"project":"/p","outcomes":[]}"#,
            r#"{"ts":"2026-09-01T10:02:00Z","kind":"pull","session":"s","agent":"sub1","way":"d/y","stamped":true,"scan_id":"t1"}"#,
            r#"{"ts":"2026-09-01T10:02:10Z","kind":"pull","session":"s","agent":"sub1","way":"d/a","stamped":true,"scan_id":"t1"}"#,
        ];
        let p = run_live("late", &live).pulls;
        assert_eq!((p.total, p.delivered, p.recall_miss, p.unjoined), (2, 1, 1, 0));
        assert_eq!(p.recall_miss_by_way, [("d/a".to_string(), 1)], "sub1 was stashed d/y; d/a only main had");
    }

    fn counts(r: &DecisionsReport) -> Vec<(&str, Vec<(&str, u32)>)> {
        r.by_way.iter().map(|(w, m)| (w.as_str(), m.iter().map(|(k, n)| (k.as_str(), *n)).collect())).collect()
    }

    #[test]
    fn turns_are_counted_by_turn_start_across_archive_and_live_file() {
        let r = run("turns", None);
        assert_eq!((r.records, r.skipped), (10, 1), "four archive records and six live; the malformed line is skipped");
        assert_eq!(r.turns, 4, "a1, b1, a3, b2: turn_start scans, though two pairs share an epoch");
        assert_eq!((r.scans, r.orphan_scans), (5, 0), "the task scan joins s1's first turn");
        // 09-01: 3, 09-02: 0, 09-03: 1.
        assert_eq!(r.turns_per_day, TurnsPerDay { days: 3, mean: 4.0 / 3.0, median: 1.0, p90: 3 });
    }

    #[test]
    fn outcomes_are_counted_per_way_and_result() {
        let r = run("outcomes", None);
        assert_eq!(
            counts(&r),
            [
                ("d/n", vec![("near_miss", 3)]),
                ("d/a", vec![("fired", 1), ("judge_block", 1)]),
                ("d/a/c", vec![("ancestor_block", 1)]),
                ("d/b", vec![("redisclosed", 1)]),
                ("d/h", vec![("held_context_cap", 1)]),
                ("d/k", vec![("keyword_gated", 1)]),
                ("d/p/c", vec![("withheld_for_parent", 1)]),
                ("d/r", vec![("held_refire", 1)]),
                ("d/t", vec![("stashed", 1)]),
                ("d/w", vec![("fired", 1)]),
            ]
        );
        assert_eq!(r.near_miss_leaders.len(), 1);
        let (way, n, sf) = &r.near_miss_leaders[0];
        assert_eq!((way.as_str(), *n), ("d/n", 3));
        assert!((sf - 0.04).abs() < 1e-9, "mean of 0.04, 0.02, 0.06: {sf}");
    }

    #[test]
    fn the_judge_tally_reads_verdicts_blocks_fallbacks_and_caps() {
        let j = run("judge", None).judge;
        assert_eq!((j.pass, j.block, j.would_block), (1, 1, 1));
        assert_eq!((j.judged_scans, j.fallback_scans, j.capped_scans, j.capped_ways), (2, 1, 1, 1));
        assert!((j.rate(j.pass) - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn a_pull_of_a_way_its_turn_never_delivered_is_a_recall_miss() {
        let p = run("pulls", None).pulls;
        assert_eq!(p.total, 5);
        assert_eq!(p.delivered, 1, "s1 pulled d/a, which a1 fired");
        assert_eq!(p.recall_miss, 2, "d/x joined via the task scan; d/a in s2 was judge-blocked, not fired");
        assert_eq!(p.recall_miss_by_way, [("d/a".to_string(), 1), ("d/x".to_string(), 1)]);
        assert_eq!((p.out_of_band, p.out_of_band_by_way.clone()), (1, vec![("d/x".to_string(), 1)]));
        assert_eq!((p.null_scan_id, p.unjoined, p.refused), (1, 0, 1));
    }

    #[test]
    fn a_project_filter_keeps_its_turns_and_drops_pulls_with_no_turn() {
        let r = run("project", Some("/p"));
        assert_eq!(r.turns, 2, "s1's two turns");
        assert_eq!(r.scans, 3);
        assert!(r.by_way.iter().all(|(w, _)| !matches!(w.as_str(), "d/w" | "d/a/c")), "s2's ways are out");
        assert_eq!((r.pulls.total, r.pulls.delivered, r.pulls.recall_miss, r.pulls.null_scan_id), (2, 1, 1, 0));
    }

    #[test]
    fn the_section_renders_and_its_absence_is_explicit() {
        let r = run("render", None);
        let v = json_value(Some(&r));
        assert_eq!(v["present"], true);
        assert_eq!(v["by_way"]["d/a"], json!({"fired": 1, "judge_block": 1}));
        assert_eq!(v["pulls"]["recall_miss_by_way"], json!({"d/a": 1, "d/x": 1}));
        assert_eq!(v["near_miss_leaders"][0], json!({"way": "d/n", "near_miss": 3, "mean_shortfall": 0.04}));
        assert_eq!(json_value(None), json!({"present": false}));
        let text = [head_lines(&r), tail_lines(&r)].concat().join("\n");
        assert!(text.contains("Turns: 4  |  per day over 3 days: mean 1.3, median 1.0, p90 3"), "{text}");
        assert!(text.contains("recall misses 2"), "{text}");
        assert!(text.contains("recall misses: d/a 1, d/x 1"), "{text}");
    }
}
