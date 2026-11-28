// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! What the judge has spent, read from `judge_call` events: the
//! aggregation `ways agent cost` prints and `ways session` reports.
//!
//! A call whose cost is unknown is counted apart and never summed as zero.
//! A call on the Claude Code account (`cost_source: account`) is counted
//! apart from both, with an estimate at Anthropic's API list price that is
//! never added to the billed cost (#1301).

use std::collections::BTreeMap;

use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::Value;

use crate::cost::{account_estimate, Usage};

/// The purpose of a `judge_call` event that names none: every call logged
/// before the continuance judge was a disclosure call.
pub const DISCLOSURE: &str = "disclosure";

/// One `judge_call` event, reduced to what the report needs.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub ts: String,
    pub session: String,
    pub project: String,
    /// What the call judged: [`DISCLOSURE`], or
    /// [`crate::continuance::PURPOSE`] for a continuance call. The totals
    /// count every purpose.
    pub purpose: String,
    pub cost_usd: Option<f64>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    /// Logged with `cost_source: account`: a call on the Claude Code account.
    pub account: bool,
    /// An account call's cost at Anthropic's API list price, when its model
    /// has one: an estimate, never billed cost.
    pub estimate_usd: Option<f64>,
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
    /// Calls on the Claude Code account: neither known nor unknown, and
    /// never in `cost_usd`. Their tokens are in the totals above as well.
    pub account_calls: u64,
    pub account_input_tokens: u64,
    pub account_output_tokens: u64,
    /// The account calls' cost at Anthropic's API list price, summed over
    /// the `account_estimated_calls` whose model has one: an estimate, never
    /// billed cost. `None` while no account call has one.
    pub account_estimate_usd: Option<f64>,
    pub account_estimated_calls: u64,
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
    let account = s("cost_source") == Some("account");
    let cost = if account || s("cost_source") == Some("unknown") { None } else { s("cost_usd").and_then(|c| c.trim().parse::<f64>().ok()).filter(|c| c.is_finite()) };
    let usage = Usage {
        input_tokens: n("input_tokens"),
        output_tokens: n("output_tokens"),
        cache_read_tokens: n("cache_read_tokens"),
        cache_write_tokens: n("cache_write_tokens"),
        provider_cost_usd: None,
    };
    let estimate_usd = if account { s("model").and_then(|m| account_estimate(m, &usage)) } else { None };
    Some(Call {
        ts: s("ts")?.to_string(),
        session: s("session").unwrap_or("").to_string(),
        project: s("project").unwrap_or("").to_string(),
        purpose: s("purpose").filter(|p| !p.is_empty()).unwrap_or(DISCLOSURE).to_string(),
        cost_usd: cost,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_read_tokens: usage.cache_read_tokens,
        cache_write_tokens: usage.cache_write_tokens,
        account,
        estimate_usd,
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

/// Keep the calls of one project and the paths under it, matched by
/// [`ways_core::util::in_project`].
pub fn filter_project(calls: Vec<Call>, project: Option<&str>) -> Vec<Call> {
    calls.into_iter().filter(|c| project.is_none_or(|p| ways_core::util::in_project(&c.project, p))).collect()
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
    if c.account {
        g.account_calls += 1;
        g.account_input_tokens += c.input_tokens;
        g.account_output_tokens += c.output_tokens;
        if let Some(e) = c.estimate_usd {
            g.account_estimated_calls += 1;
            g.account_estimate_usd = Some(g.account_estimate_usd.unwrap_or(0.0) + e);
        }
    } else {
        match c.cost_usd {
            Some(x) => {
                g.known_calls += 1;
                g.cost_usd = Some(g.cost_usd.unwrap_or(0.0) + x);
            }
            None => g.unknown_calls += 1,
        }
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
    /// `cost unknown` when no call's cost is known and none is on the
    /// account. Account calls add their list-price estimate, `~$0.0456 est`,
    /// or `on account` when their model has none, between the billed cost
    /// and the unknown count: `$0.0123 + ~$0.0456 est + 2 unknown`.
    pub fn cost_short(&self) -> String {
        if self.account_calls == 0 {
            return match self.cost_usd {
                None => "cost unknown".into(),
                Some(c) if self.unknown_calls > 0 => format!("${c:.4} + {} unknown", self.unknown_calls),
                Some(c) => format!("${c:.4}"),
            };
        }
        let mut parts: Vec<String> = self.cost_usd.map(|c| format!("${c:.4}")).into_iter().collect();
        parts.push(self.account_estimate_usd.map_or_else(|| "on account".into(), |e| format!("~${e:.4} est")));
        if self.unknown_calls > 0 {
            parts.push(format!("{} unknown", self.unknown_calls));
        }
        parts.join(" + ")
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
    // The billed cost covers the calls off the account. A group whose calls
    // are all on the account has no billed cost to show.
    let cost = match g.cost_usd {
        None if g.unknown_calls == 0 && g.account_calls > 0 => String::new(),
        None if g.account_calls > 0 => format!("{} of unknown cost, ", n(g.unknown_calls)),
        None => "cost unknown, ".to_string(),
        Some(c) if g.unknown_calls > 0 => format!("${c:.4} + {} of unknown cost, ", n(g.unknown_calls)),
        Some(c) => format!("${c:.4}, "),
    };
    let mut out = format!("{}, {cost}tokens {} in / {} out", n(g.calls), g.input_tokens, g.output_tokens);
    if g.account_calls > 0 {
        out.push_str(&format!("; {} on the Claude Code account, tokens {} in / {} out", n(g.account_calls), g.account_input_tokens, g.account_output_tokens));
        if let Some(e) = g.account_estimate_usd {
            let of = if g.account_estimated_calls < g.account_calls { format!(" for {} of them", g.account_estimated_calls) } else { String::new() };
            out.push_str(&format!(", ~${e:.4} at API list price{of} (estimate)"));
        }
    }
    out
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
    ways_core::paths::events_log_sources().iter().filter_map(|p| ways_core::event_archive::read_source(p)).flat_map(|t| parse_log(&t)).collect()
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
    fn a_project_takes_the_paths_under_it() {
        let kept = |p: &str| filter_project(calls(), Some(p)).into_iter().map(|c| c.session).collect::<Vec<_>>();
        assert_eq!(kept("/p/a"), ["s1", "s1", "s3"]);
        assert_eq!(kept("/p/a/"), ["s1", "s1", "s3"]);
        assert_eq!(kept("/p").len(), 4, "a parent takes the projects under it");
        assert!(kept("/p/a-other").is_empty(), "a sibling whose name begins the same is not under it");
        assert_eq!(kept("/p/").len(), 4);
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

    /// Two account calls (one on a model with no list price), a deadline,
    /// and one priced call, all on one day.
    const ACCOUNT_LOG: &str = r#"{"event":"judge_call","ts":"2026-11-20T09:00:00Z","session":"s1","project":"/p/a","outcome":"judged","engine":"claude-code","provider":"claude-code","model":"claude-haiku-4-5","input_tokens":"1000000","output_tokens":"100000","cache_read_tokens":"0","cache_write_tokens":"0","cost_source":"account"}
{"event":"judge_call","ts":"2026-11-20T09:01:00Z","session":"s1","project":"/p/a","outcome":"judged","engine":"claude-code","provider":"claude-code","model":"claude-sonnet-4-5","input_tokens":"10","output_tokens":"1","cost_source":"account"}
{"event":"judge_call","ts":"2026-11-20T09:02:00Z","session":"s1","project":"/p/a","outcome":"fallback","reason":"deadline","engine":"claude-code","provider":"claude-code","model":"claude-haiku-4-5","cost_source":"unknown"}
{"event":"judge_call","ts":"2026-11-20T09:03:00Z","session":"s2","project":"/p/b","outcome":"judged","input_tokens":"50","output_tokens":"5","cost_usd":"0.0300","cost_source":"price_table"}
"#;

    #[test]
    fn account_calls_are_counted_apart_from_unknown_ones() {
        let calls = parse_log(ACCOUNT_LOG);
        let t = total(&calls);
        assert_eq!((t.calls, t.known_calls, t.unknown_calls, t.account_calls), (4, 1, 1, 2));
        assert_eq!((t.account_input_tokens, t.account_output_tokens), (1_000_010, 100_001));
        assert_eq!((t.input_tokens, t.output_tokens), (1_000_060, 100_006), "account tokens are in the totals");
        // The estimate covers the haiku call only and is not billed cost.
        assert_eq!(t.account_estimated_calls, 1);
        assert!((t.account_estimate_usd.unwrap() - 1.5).abs() < 1e-9);
        assert!((t.cost_usd.unwrap() - 0.03).abs() < 1e-9, "the estimate is not in the billed total");
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!((v["account_calls"].as_u64(), v["unknown_calls"].as_u64()), (Some(2), Some(1)));
        assert!((v["cost_usd"].as_f64().unwrap() - 0.03).abs() < 1e-9);
    }

    #[test]
    fn account_calls_print_on_their_own_clause_with_a_labelled_estimate() {
        let calls = parse_log(ACCOUNT_LOG);
        let text = render_text(&calls, By::Day, None);
        let want = "total: 4 calls, $0.0300 + 1 call of unknown cost, tokens 1000060 in / 100006 out; \
2 calls on the Claude Code account, tokens 1000010 in / 100001 out, ~$1.5000 at API list price for 1 of them (estimate)\n";
        assert!(text.starts_with(want), "{text}");
        // A day of haiku account calls only: no billed cost, no "unknown".
        let haiku = render_text(&calls[..1], By::Day, None);
        assert_eq!(
            haiku.lines().next().unwrap(),
            "total: 1 call, tokens 1000000 in / 100000 out; 1 call on the Claude Code account, tokens 1000000 in / 100000 out, ~$1.5000 at API list price (estimate)"
        );
        assert!(!haiku.contains("unknown"), "{haiku}");
        // A model with no list price shows no estimate.
        assert!(!render_text(&calls[1..2], By::Day, None).contains('$'));
    }

    #[test]
    fn account_calls_beside_unknown_or_billed_ones_print_exactly() {
        let calls = parse_log(ACCOUNT_LOG);
        // Account and unknown calls, none of known cost.
        let text = render_text(&calls[..3], By::Day, None);
        let clause = "1 call of unknown cost, tokens 1000010 in / 100001 out; \
2 calls on the Claude Code account, tokens 1000010 in / 100001 out, ~$1.5000 at API list price for 1 of them (estimate)\n";
        assert_eq!(text, format!("total: 3 calls, {clause}\x20 2026-11-20: 3 calls, {clause}"));
        // Account calls and a billed one, none unknown.
        let billed = [calls[0].clone(), calls[3].clone()];
        let clause = "$0.0300, tokens 1000050 in / 100005 out; \
1 call on the Claude Code account, tokens 1000000 in / 100000 out, ~$1.5000 at API list price (estimate)\n";
        assert_eq!(render_text(&billed, By::Day, None), format!("total: 2 calls, {clause}\x20 2026-11-20: 2 calls, {clause}"));
    }

    #[test]
    fn the_short_cost_always_shows_account_calls() {
        let calls = parse_log(ACCOUNT_LOG);
        let short = |c: &[Call]| total(c).cost_short();
        assert_eq!(short(&calls[..1]), "~$1.5000 est");
        assert_eq!(short(&calls[1..2]), "on account");
        assert_eq!(short(&calls[1..3]), "on account + 1 unknown");
        assert_eq!(short(&calls[..3]), "~$1.5000 est + 1 unknown", "not \"cost unknown\" (#1301)");
        assert_eq!(short(&[calls[0].clone(), calls[3].clone()]), "$0.0300 + ~$1.5000 est");
        assert_eq!(short(&calls), "$0.0300 + ~$1.5000 est + 1 unknown");
        assert_eq!(short(&calls[2..3]), "cost unknown");
        assert_eq!(short(&calls[3..]), "$0.0300");
    }

    #[test]
    fn a_continuance_call_keeps_its_purpose_and_counts_in_the_totals() {
        let log = r#"{"event":"judge_call","ts":"2026-11-27T09:00:00Z","session":"s1","project":"/p/a","purpose":"continuance","outcome":"judged","engine":"anthropic","provider":"anthropic","model":"claude-haiku-5-5","input_tokens":"4000","output_tokens":"200","cost_usd":"0.00050000","cost_source":"price_table"}
{"event":"judge_call","ts":"2026-11-27T09:01:00Z","session":"s1","project":"/p/a","purpose":"continuance","outcome":"judged","engine":"claude-code","provider":"claude-code","model":"claude-haiku-5-5","input_tokens":"4000","output_tokens":"200","cost_source":"account"}
"#;
        let mut calls = parse_log(log);
        calls.extend(parse_log(LOG));
        let purposes: Vec<&str> = calls.iter().map(|c| c.purpose.as_str()).collect();
        assert_eq!(purposes, ["continuance", "continuance", "disclosure", "disclosure", "disclosure", "disclosure"]);
        let t = total(&calls);
        assert_eq!((t.calls, t.known_calls, t.account_calls), (6, 3, 1));
        assert!((t.cost_usd.unwrap() - 0.0405).abs() < 1e-9);
        // The account call is estimated at Haiku 5.5's list price.
        assert!((t.account_estimate_usd.unwrap() - 0.0005).abs() < 1e-9);
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
