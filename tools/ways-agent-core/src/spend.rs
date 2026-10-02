//! What the judge has spent, read from `judge_call` events: the
//! aggregation `ways agent cost` prints and `ways session` reports.
//!
//! A call whose cost is unknown is counted apart and never summed as zero.

use std::collections::BTreeMap;

use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::Value;

/// One `judge_call` event, reduced to what the report needs.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub ts: String,
    pub session: String,
    pub project: String,
    pub cost_usd: Option<f64>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
pub enum By {
    Session,
    Project,
    Day,
    Month,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Group {
    pub key: String,
    pub calls: u64,
    pub known_calls: u64,
    pub unknown_calls: u64,
    /// Sum over the known calls only; `None` while no call's cost is known.
    pub cost_usd: Option<f64>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

#[derive(Debug, Serialize)]
pub struct Report {
    /// Earliest `ts` in the whole log, before any filter: where history begins.
    pub covers_since: Option<String>,
    pub total: Group,
    pub by_session: Vec<Group>,
    pub by_project: Vec<Group>,
    pub by_day: Vec<Group>,
    pub by_month: Vec<Group>,
}

/// Parse one JSONL line; `None` unless it is a `judge_call` event.
pub fn parse_line(line: &str) -> Option<Call> {
    let v: Value = serde_json::from_str(line).ok()?;
    let s = |k: &str| v.get(k).and_then(Value::as_str);
    if s("event")? != "judge_call" {
        return None;
    }
    let n = |k: &str| s(k).and_then(|x| x.trim().parse::<u64>().ok()).unwrap_or(0);
    let cost = if s("cost_source") == Some("unknown") { None } else { s("cost_usd").and_then(|c| c.trim().parse::<f64>().ok()).filter(|c| c.is_finite()) };
    Some(Call {
        ts: s("ts")?.to_string(),
        session: s("session").unwrap_or("").to_string(),
        project: s("project").unwrap_or("").to_string(),
        cost_usd: cost,
        input_tokens: n("input_tokens"),
        output_tokens: n("output_tokens"),
        cache_read_tokens: n("cache_read_tokens"),
        cache_write_tokens: n("cache_write_tokens"),
    })
}

pub fn parse_log(text: &str) -> Vec<Call> {
    text.lines().filter_map(parse_line).collect()
}

/// Validate a `YYYY-MM-DD` date: the shape and the month and day ranges, not the calendar.
pub fn parse_date(s: &str) -> Result<String> {
    let b = s.as_bytes();
    let digits = |r: std::ops::Range<usize>| b.get(r).is_some_and(|d| d.iter().all(u8::is_ascii_digit));
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' || !digits(0..4) || !digits(5..7) || !digits(8..10) {
        bail!("bad date {s:?}: expected YYYY-MM-DD");
    }
    let (m, d): (u32, u32) = (s[5..7].parse()?, s[8..10].parse()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        bail!("bad date {s:?}: month or day out of range");
    }
    Ok(s.to_string())
}

/// Keep calls at or after `since` (UTC date) and, if given, of one session.
pub fn filter(calls: Vec<Call>, since: Option<&str>, session: Option<&str>) -> Vec<Call> {
    calls
        .into_iter()
        .filter(|c| since.is_none_or(|d| c.ts.get(..10).is_some_and(|day| day >= d)))
        .filter(|c| session.is_none_or(|s| c.session == s))
        .collect()
}

/// Whether a call's recorded `project` is `scope`: the same path, a
/// trailing slash aside. Exact, so a worktree under the project is its own.
pub fn same_project(project: &str, scope: &str) -> bool {
    project.trim_end_matches('/') == scope.trim_end_matches('/')
}

/// Keep the calls of one project, matched by [`same_project`].
pub fn filter_project(calls: Vec<Call>, project: Option<&str>) -> Vec<Call> {
    calls.into_iter().filter(|c| project.is_none_or(|p| same_project(&c.project, p))).collect()
}

fn key_of(c: &Call, by: By) -> String {
    match by {
        By::Session => c.session.clone(),
        By::Project => c.project.clone(),
        By::Day => c.ts.get(..10).unwrap_or("").to_string(),
        By::Month => c.ts.get(..7).unwrap_or("").to_string(),
    }
}

fn add(g: &mut Group, c: &Call) {
    g.calls += 1;
    match c.cost_usd {
        Some(x) => {
            g.known_calls += 1;
            g.cost_usd = Some(g.cost_usd.unwrap_or(0.0) + x);
        }
        None => g.unknown_calls += 1,
    }
    g.input_tokens += c.input_tokens;
    g.output_tokens += c.output_tokens;
    g.cache_read_tokens += c.cache_read_tokens;
    g.cache_write_tokens += c.cache_write_tokens;
}

pub fn total(calls: &[Call]) -> Group {
    let mut g = Group { key: "total".into(), ..Group::default() };
    calls.iter().for_each(|c| add(&mut g, c));
    g
}

/// Group calls. Day and month: newest first. Session and project: highest cost first.
pub fn aggregate(calls: &[Call], by: By) -> Vec<Group> {
    let mut map: BTreeMap<String, Group> = BTreeMap::new();
    for c in calls {
        let key = key_of(c, by);
        add(map.entry(key.clone()).or_insert_with(|| Group { key, ..Group::default() }), c);
    }
    let mut groups: Vec<Group> = map.into_values().collect();
    match by {
        By::Day | By::Month => groups.reverse(),
        By::Session | By::Project => groups.sort_by(|a, b| match (a.cost_usd, b.cost_usd) {
            (Some(x), Some(y)) => y.total_cmp(&x),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
        .then_with(|| a.key.cmp(&b.key))),
    }
    groups
}

impl Group {
    /// Every token the calls moved: input, output, and cache reads and writes.
    pub fn tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens + self.cache_read_tokens + self.cache_write_tokens
    }

    /// The group's tokens, short: `950`, `12.3K`, `4.1M`.
    pub fn tokens_short(&self) -> String {
        // The unit is chosen where its one decimal no longer rounds up into
        // the next: 999,950 is `1.0M`, never `1000.0K`.
        let n = self.tokens();
        match n {
            0..1_000 => n.to_string(),
            1_000..999_950 => format!("{:.1}K", n as f64 / 1e3),
            _ => format!("{:.1}M", n as f64 / 1e6),
        }
    }

    /// The group's cost, short: `$0.0123`, `$0.0123 + 2 unknown`, or
    /// `cost unknown` when no call's cost is known.
    pub fn cost_short(&self) -> String {
        match self.cost_usd {
            None => "cost unknown".into(),
            Some(c) if self.unknown_calls > 0 => format!("${c:.4} + {} unknown", self.unknown_calls),
            Some(c) => format!("${c:.4}"),
        }
    }
}

/// The earliest `ts` among the calls.
pub fn covers_since(calls: &[Call]) -> Option<String> {
    calls.iter().map(|c| c.ts.as_str()).min().map(str::to_string)
}

pub fn report(calls: &[Call], covers_since: Option<String>) -> Report {
    Report {
        covers_since,
        total: total(calls),
        by_session: aggregate(calls, By::Session),
        by_project: aggregate(calls, By::Project),
        by_day: aggregate(calls, By::Day),
        by_month: aggregate(calls, By::Month),
    }
}

fn describe(g: &Group) -> String {
    let n = |k: u64| format!("{k} {}", if k == 1 { "call" } else { "calls" });
    let cost = match g.cost_usd {
        None => "cost unknown".to_string(),
        Some(c) if g.unknown_calls > 0 => format!("${c:.4} + {} of unknown cost", n(g.unknown_calls)),
        Some(c) => format!("${c:.4}"),
    };
    format!("{}, {cost}, tokens {} in / {} out", n(g.calls), g.input_tokens, g.output_tokens)
}

/// The report as text. `log_from` is the date of the earliest judge call the
/// log still holds, given when it bounds the query: the log is compacted, so
/// calls before it are gone.
pub fn render_text(calls: &[Call], by: By, log_from: Option<&str>) -> String {
    if calls.is_empty() {
        return "no judge calls recorded\n".into();
    }
    let mut out = format!("total: {}\n", describe(&total(calls)));
    for g in aggregate(calls, by) {
        out.push_str(&format!("  {}: {}\n", g.key, describe(&g)));
    }
    if let Some(d) = log_from {
        out.push_str(&format!("earliest call in the log: {}\n", d.get(..10).unwrap_or(d)));
    }
    out
}

/// Read every events log source; unreadable files are skipped.
pub fn load() -> Vec<Call> {
    ways_core::paths::events_log_sources().iter().filter_map(|p| std::fs::read_to_string(p).ok()).flat_map(|t| parse_log(&t)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = r#"{"event":"judge_call","ts":"2026-10-02T16:55:01Z","session":"s1","project":"/p/a","outcome":"judged","input_tokens":"100","output_tokens":"10","cost_usd":"0.0100","cost_source":"provider"}
{"event":"judge_call","ts":"2026-10-02T17:00:00Z","session":"s2","project":"/p/b","outcome":"judged","input_tokens":"50","output_tokens":"5","cost_usd":"0.0300","cost_source":"price_table"}
{"event":"judge_call","ts":"2026-09-30T10:00:00Z","session":"s1","project":"/p/a","outcome":"fallback","reason":"timeout","cost_source":"unknown"}
{"event":"judge_call","ts":"2026-09-01T10:00:00Z","session":"s3","project":"/p/a","input_tokens":"7","output_tokens":"1","cache_read_tokens":"3","cache_write_tokens":"2","cost_source":"unknown"}
{"event":"other","ts":"2026-10-02T00:00:00Z"}
not json
{"event":"judge_call"}
"#;

    fn calls() -> Vec<Call> {
        parse_log(LOG)
    }

    #[test]
    fn short_forms_of_tokens_and_cost() {
        let g = |input, cost, unknown| Group { input_tokens: input, cost_usd: cost, unknown_calls: unknown, ..Group::default() };
        assert_eq!(g(999, Some(0.0123), 0).tokens_short(), "999");
        assert_eq!(g(12_345, None, 0).tokens_short(), "12.3K");
        assert_eq!(g(4_100_000, None, 0).tokens_short(), "4.1M");
        assert_eq!(g(999_949, None, 0).tokens_short(), "999.9K");
        assert_eq!(g(999_950, None, 0).tokens_short(), "1.0M");
        assert_eq!(g(0, Some(0.0123), 0).cost_short(), "$0.0123");
        assert_eq!(g(0, Some(0.0123), 2).cost_short(), "$0.0123 + 2 unknown");
        assert_eq!(g(0, None, 3).cost_short(), "cost unknown");
        let all = Group { input_tokens: 1, output_tokens: 2, cache_read_tokens: 3, cache_write_tokens: 4, ..Group::default() };
        assert_eq!(all.tokens(), 10);
    }

    #[test]
    fn a_project_matches_exactly_a_trailing_slash_aside() {
        let kept = |p: &str| filter_project(calls(), Some(p)).into_iter().map(|c| c.session).collect::<Vec<_>>();
        assert_eq!(kept("/p/a"), ["s1", "s1", "s3"]);
        assert_eq!(kept("/p/a/"), ["s1", "s1", "s3"]);
        assert!(kept("/p").is_empty(), "a parent path is not the project");
        assert_eq!(filter_project(calls(), None).len(), 4);
    }

    #[test]
    fn malformed_and_foreign_lines_are_skipped() {
        assert_eq!(calls().len(), 4);
    }

    #[test]
    fn groups_by_day_newest_first() {
        let g = aggregate(&calls(), By::Day);
        let keys: Vec<_> = g.iter().map(|g| g.key.as_str()).collect();
        assert_eq!(keys, ["2026-10-02", "2026-09-30", "2026-09-01"]);
        assert_eq!(g[0].calls, 2);
        assert!((g[0].cost_usd.unwrap() - 0.04).abs() < 1e-9);
    }

    #[test]
    fn groups_by_month() {
        let g = aggregate(&calls(), By::Month);
        assert_eq!(g.iter().map(|g| g.key.as_str()).collect::<Vec<_>>(), ["2026-10", "2026-09"]);
        assert_eq!(g[1].calls, 2);
        assert_eq!(g[1].unknown_calls, 2);
    }

    #[test]
    fn groups_by_session_and_project_highest_cost_first() {
        let s = aggregate(&calls(), By::Session);
        assert_eq!(s[0].key, "s2");
        assert_eq!(s[1].key, "s1");
        let p = aggregate(&calls(), By::Project);
        assert_eq!(p[0].key, "/p/b");
        assert_eq!(p[1].calls, 3);
    }

    #[test]
    fn unknown_calls_are_counted_never_summed_as_zero() {
        let t = total(&calls());
        assert_eq!((t.calls, t.known_calls, t.unknown_calls), (4, 2, 2));
        assert!((t.cost_usd.unwrap() - 0.04).abs() < 1e-9);
        assert_eq!((t.cache_read_tokens, t.cache_write_tokens), (3, 2));
        let text = render_text(&calls(), By::Day, None);
        assert!(text.contains("$0.0400 + 2 calls of unknown cost"));
        assert!(!text.contains("known calls only"));
        assert!(!render_text(&calls()[..2], By::Day, None).contains("unknown"));
    }

    #[test]
    fn all_unknown_group_has_null_cost_and_sorts_last() {
        let g = aggregate(&calls(), By::Session);
        assert_eq!(g.iter().map(|g| g.key.as_str()).collect::<Vec<_>>(), ["s2", "s1", "s3"]);
        assert_eq!(g[2].cost_usd, None);
        let v = serde_json::to_value(report(&calls(), None)).unwrap();
        assert!(v["by_session"][2]["cost_usd"].is_null());
        assert!(v["total"]["cost_usd"].is_number());
    }

    #[test]
    fn covers_since_is_the_earliest_ts_before_filters() {
        let all = calls();
        let covers = covers_since(&all);
        assert_eq!(covers.as_deref(), Some("2026-09-01T10:00:00Z"));
        let v = serde_json::to_value(report(&filter(all, Some("2026-10-01"), None), covers.clone())).unwrap();
        assert_eq!(v["covers_since"], "2026-09-01T10:00:00Z");
        assert!(serde_json::to_value(report(&[], None)).unwrap()["covers_since"].is_null());
        let text = render_text(&calls(), By::Day, covers.as_deref());
        assert!(text.starts_with("total: 4 calls, ") && text.ends_with("earliest call in the log: 2026-09-01\n"));
        assert!(!render_text(&calls(), By::Day, None).contains("earliest"));
    }

    #[test]
    fn filters_by_since_and_session() {
        assert_eq!(filter(calls(), Some("2026-09-30"), None).len(), 3);
        assert_eq!(filter(calls(), None, Some("s1")).len(), 2);
        assert_eq!(filter(calls(), Some("2026-10-01"), Some("s1")).len(), 1);
    }

    #[test]
    fn dates_validate() {
        assert!(parse_date("2026-10-02").is_ok());
        for bad in ["2026-1-02", "yesterday", "2026-13-01", "2026-10-32", ""] {
            assert!(parse_date(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn empty_says_so_and_json_has_all_groupings() {
        assert_eq!(render_text(&[], By::Day, None), "no judge calls recorded\n");
        let v = serde_json::to_value(report(&calls(), None)).unwrap();
        for k in ["total", "by_session", "by_project", "by_day", "by_month"] {
            assert!(v.get(k).is_some(), "{k}");
        }
        assert_eq!(v["total"]["unknown_calls"], 2);
    }

    #[test]
    fn text_output_is_exact() {
        let text = render_text(&calls(), By::Day, Some("2026-09-01T10:00:00Z"));
        let want = "total: 4 calls, $0.0400 + 2 calls of unknown cost, tokens 157 in / 16 out\n\
\x20 2026-10-02: 2 calls, $0.0400, tokens 150 in / 15 out\n\
\x20 2026-09-30: 1 call, cost unknown, tokens 0 in / 0 out\n\
\x20 2026-09-01: 1 call, cost unknown, tokens 7 in / 1 out\n\
earliest call in the log: 2026-09-01\n";
        assert_eq!(text, want);
    }
}
