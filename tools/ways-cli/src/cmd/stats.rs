//! Usage statistics from the events log (`ways events-log-path`).
//! Replaces stats.sh (348 lines).

use agent_fmt::{Align, Table};
use anyhow::Result;
use serde_json::json;
use std::collections::{BTreeMap, HashMap};

pub fn run(days: Option<u32>, project_filter: Option<&str>, json_output: bool, global: bool) -> Result<()> {
    // Default to project scope: CLAUDE_PROJECT_DIR > detect from cwd > global
    let detected_project = if !global && project_filter.is_none() {
        crate::util::project_root()
    } else {
        None
    };
    let project_filter = project_filter.or(detected_project.as_deref());
    if crate::paths::events_log_sources().is_empty() {
        if !json_output {
            println!("No events recorded yet. Stats will appear after ways start firing.");
        }
        return Ok(());
    }

    let content = ways_core::firing::load_events_text();
    let stats = report(&content, days, project_filter);

    if json_output {
        print_json(&stats);
    } else {
        print_human(&stats, days, project_filter);
    }

    Ok(())
}

struct Event {
    ts: String,
    event: String,
    way: String,
    trigger: String,
    scope: String,
    #[allow(dead_code)]
    project: String,
    #[allow(dead_code)]
    team: String,
    session: String,
    /// The agent the fire was delivered to (`main` or a subagent id). Empty
    /// when the row predates the field.
    agent_id: String,
    /// The model id stamped at fire time. `None` when the row predates the
    /// field; the literal `unknown` when stamping ran and found no model.
    model: Option<String>,
    check: String,
    distance: f64,
    anchored: bool,
    token_distance: f64,
}

/// Bucket for fire rows written before the `model` field existed. Parenthesised
/// so it cannot collide with a model id.
const UNSTAMPED: &str = "(unstamped)";

/// Number of ways above which a hook invocation lands in the open-ended bucket.
const INVOCATION_TAIL: usize = 4;

fn parse_events(content: &str, days: Option<u32>, project_filter: Option<&str>) -> Vec<Event> {
    let cutoff = days.map(|d| {
        agent_fmt::when::utc_iso(agent_fmt::when::now_secs() - (d as u64 * 86400))
    });

    content
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).ok()?;
            let ts = v["ts"].as_str().unwrap_or("").to_string();

            if let Some(ref c) = cutoff {
                if ts < *c {
                    return None;
                }
            }

            let project = v["project"].as_str().unwrap_or("").to_string();
            if let Some(pf) = project_filter {
                if !project.contains(pf) {
                    return None;
                }
            }

            Some(Event {
                ts,
                event: v["event"].as_str().unwrap_or("").to_string(),
                way: v["way"].as_str().unwrap_or("").to_string(),
                trigger: v["trigger"].as_str().unwrap_or("").to_string(),
                scope: v["scope"].as_str().unwrap_or("unknown").to_string(),
                project,
                team: v["team"].as_str().unwrap_or("").to_string(),
                session: v["session"].as_str().unwrap_or("").to_string(),
                agent_id: v["agent_id"].as_str().unwrap_or("").to_string(),
                model: v["model"].as_str().map(str::to_string),
                check: v["check"].as_str().unwrap_or("").to_string(),
                distance: v["distance"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
                anchored: v["anchored"].as_str() == Some("true"),
                token_distance: v["token_distance"]
                    .as_str()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0),
            })
        })
        .collect()
}

impl Event {
    fn model_bucket(&self) -> &str {
        self.model.as_deref().unwrap_or(UNSTAMPED)
    }
}

// ── Per-model breakdown ─────────────────────────────────────────

#[derive(Default, Clone, Copy, PartialEq, Debug)]
pub(crate) struct ModelTally {
    pub(crate) fires: u32,
    pub(crate) redisclosures: u32,
}

/// Fires and re-disclosures per model, most fires first. Rows without the
/// field land in [`UNSTAMPED`] so the share of pre-stamping history is visible
/// rather than silently dropped.
fn model_breakdown(events: &[Event]) -> Vec<(String, ModelTally)> {
    let mut by_model: HashMap<&str, ModelTally> = HashMap::new();
    for e in events {
        match e.event.as_str() {
            "way_fired" => by_model.entry(e.model_bucket()).or_default().fires += 1,
            "way_redisclosed" => by_model.entry(e.model_bucket()).or_default().redisclosures += 1,
            _ => {}
        }
    }
    let mut rows: Vec<(String, ModelTally)> =
        by_model.into_iter().map(|(m, t)| (m.to_string(), t)).collect();
    rows.sort_by(|a, b| b.1.fires.cmp(&a.1.fires).then_with(|| a.0.cmp(&b.0)));
    rows
}

/// Per way, fires split by model. Only `way_fired` rows count, matching the
/// "Top ways" section this sits beside.
fn way_model_split(events: &[Event]) -> HashMap<&str, HashMap<&str, u32>> {
    let mut split: HashMap<&str, HashMap<&str, u32>> = HashMap::new();
    for e in events.iter().filter(|e| e.event == "way_fired") {
        *split
            .entry(&e.way)
            .or_default()
            .entry(e.model_bucket())
            .or_insert(0) += 1;
    }
    split
}

// ── Ways per hook invocation ────────────────────────────────────

/// Map a `trigger` value onto the hook lane that delivered it. The prompt lane
/// fires ways under `keyword` and the `semantic:embedding:*` /
/// `semantic:late-interaction:*` channels (the queued-message lane, which runs
/// the same matcher on PostToolUse, is folded in with it); the bash lane under
/// `semantic:bash:*`. Everything else is its own lane, named by the segment
/// before the first colon: `bash`, `file`, `state`, `attend:*`, and the one
/// legacy `bash:semantic:en` spelling in early history all resolve that way.
fn trigger_channel(trigger: &str) -> &str {
    match trigger {
        "keyword" => "prompt",
        t if t.starts_with("semantic:embedding") || t.starts_with("semantic:late-interaction") => {
            "prompt"
        }
        t if t.starts_with("semantic:bash") => "bash",
        t => t.split(':').next().unwrap_or(t),
    }
}

#[derive(Default, Clone, PartialEq, Debug)]
pub(crate) struct InvocationLoad {
    pub(crate) invocations: u32,
    /// Invocations that fired exactly 1, 2, 3 ways, then 4 or more.
    pub(crate) buckets: [u32; INVOCATION_TAIL],
    pub(crate) max: u32,
}

/// How many ways each hook invocation fired, per channel. An invocation is
/// approximated as the set of `way_fired` rows sharing (session, agent,
/// timestamp, channel): the log carries no invocation id, and one `ways`
/// process writes all its fires within the same second. The agent is in the
/// key because subagent hooks report the parent's session id, so parallel
/// agents firing in the same second would otherwise merge into one oversized
/// invocation. Two residual errors pull in opposite directions: a slow
/// invocation that straddles a second boundary counts as two (low), and rows
/// written before `agent_id` existed still merge parallel agents (high), so
/// the tail on historical data is an upper bound.
fn ways_per_invocation(events: &[Event]) -> Vec<(String, InvocationLoad)> {
    let mut per_invocation: HashMap<(&str, &str, &str, &str), u32> = HashMap::new();
    for e in events.iter().filter(|e| e.event == "way_fired") {
        *per_invocation
            .entry((&e.session, &e.agent_id, &e.ts, trigger_channel(&e.trigger)))
            .or_insert(0) += 1;
    }
    let mut by_channel: BTreeMap<&str, InvocationLoad> = BTreeMap::new();
    for ((_, _, _, channel), n) in per_invocation {
        let load = by_channel.entry(channel).or_default();
        load.invocations += 1;
        let idx = (n as usize).clamp(1, INVOCATION_TAIL) - 1;
        load.buckets[idx] += 1;
        load.max = load.max.max(n);
    }
    let mut rows: Vec<(String, InvocationLoad)> =
        by_channel.into_iter().map(|(c, l)| (c.to_string(), l)).collect();
    rows.sort_by(|a, b| b.1.invocations.cmp(&a.1.invocations).then_with(|| a.0.cmp(&b.0)));
    rows
}

/// Everything `ways tune stats` prints, computed once. The JSON and human
/// renderers format this, and the session screen's stats tab reads it
/// directly. Count tables are sorted most-first, ties by name, so the output
/// is stable run to run.
#[derive(Default, Clone, PartialEq, Debug)]
pub(crate) struct StatsReport {
    pub(crate) total_events: usize,
    pub(crate) sessions: u32,
    pub(crate) fires: u32,
    pub(crate) redisclosures: u32,
    /// Timestamps of the first and last event in the period (full ISO text).
    pub(crate) first_ts: Option<String>,
    pub(crate) last_ts: Option<String>,
    pub(crate) by_way: Vec<(String, u32)>,
    pub(crate) by_trigger: Vec<(String, u32)>,
    pub(crate) by_scope: Vec<(String, u32)>,
    pub(crate) by_model: Vec<(String, ModelTally)>,
    /// Per way, fires split by model.
    pub(crate) by_way_model: HashMap<String, HashMap<String, u32>>,
    pub(crate) ways_per_invocation: Vec<(String, InvocationLoad)>,
    pub(crate) check_fires: u32,
    pub(crate) by_check: Vec<(String, u32)>,
    pub(crate) check_avg_distance: f64,
    pub(crate) check_anchored: u32,
    pub(crate) redisclose_avg_token_distance: f64,
}

/// Count table as rows, most first, ties by name.
fn ranked(counts: HashMap<&str, u32>) -> Vec<(String, u32)> {
    let mut rows: Vec<(String, u32)> =
        counts.into_iter().map(|(k, n)| (k.to_string(), n)).collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    rows
}

/// Parse the events log text and aggregate it into a [`StatsReport`].
pub(crate) fn report(content: &str, days: Option<u32>, project_filter: Option<&str>) -> StatsReport {
    let events = parse_events(content, days, project_filter);
    aggregate(&events)
}

fn aggregate(events: &[Event]) -> StatsReport {
    let mut by_way: HashMap<&str, u32> = HashMap::new();
    let mut by_trigger: HashMap<&str, u32> = HashMap::new();
    let mut by_scope: HashMap<&str, u32> = HashMap::new();
    let mut by_check: HashMap<&str, u32> = HashMap::new();
    let mut sessions = 0u32;
    let mut fires = 0u32;
    let mut check_fires = 0u32;
    let mut redisclosures = 0u32;
    let mut check_distances: Vec<f64> = Vec::new();
    let mut check_anchored = 0u32;
    let mut redisclose_distances: Vec<f64> = Vec::new();

    for e in events {
        match e.event.as_str() {
            "session_start" => sessions += 1,
            "way_fired" => {
                fires += 1;
                *by_way.entry(&e.way).or_insert(0) += 1;
                *by_trigger.entry(&e.trigger).or_insert(0) += 1;
                *by_scope.entry(&e.scope).or_insert(0) += 1;
            }
            "check_fired" => {
                check_fires += 1;
                *by_check.entry(&e.check).or_insert(0) += 1;
                check_distances.push(e.distance);
                if e.anchored {
                    check_anchored += 1;
                }
            }
            "way_redisclosed" => {
                redisclosures += 1;
                redisclose_distances.push(e.token_distance);
            }
            _ => {}
        }
    }

    let avg_check_dist = if check_distances.is_empty() {
        0.0
    } else {
        check_distances.iter().sum::<f64>() / check_distances.len() as f64
    };
    let avg_redisclose_dist = if redisclose_distances.is_empty() {
        0.0
    } else {
        redisclose_distances.iter().sum::<f64>() / redisclose_distances.len() as f64
    };

    let by_way_model = way_model_split(events)
        .into_iter()
        .map(|(way, split)| {
            (
                way.to_string(),
                split.into_iter().map(|(m, n)| (m.to_string(), n)).collect(),
            )
        })
        .collect();

    StatsReport {
        total_events: events.len(),
        sessions,
        fires,
        redisclosures,
        first_ts: events.first().map(|e| e.ts.clone()),
        last_ts: events.last().map(|e| e.ts.clone()),
        by_way: ranked(by_way),
        by_trigger: ranked(by_trigger),
        by_scope: ranked(by_scope),
        by_model: model_breakdown(events),
        by_way_model,
        ways_per_invocation: ways_per_invocation(events),
        check_fires,
        by_check: ranked(by_check),
        check_avg_distance: avg_check_dist,
        check_anchored,
        redisclose_avg_token_distance: avg_redisclose_dist,
    }
}

/// A count table as a JSON object, keys in row order.
fn counts_json(rows: &[(String, u32)]) -> serde_json::Map<String, serde_json::Value> {
    rows.iter().map(|(k, n)| (k.clone(), json!(n))).collect()
}

fn print_json(r: &StatsReport) {
    let by_model: serde_json::Map<String, serde_json::Value> = r
        .by_model
        .iter()
        .map(|(m, t)| (m.clone(), json!({"fires": t.fires, "redisclosures": t.redisclosures})))
        .collect();
    let by_way_model: serde_json::Map<String, serde_json::Value> = r
        .by_way_model
        .iter()
        .map(|(way, split)| (way.clone(), json!(split)))
        .collect();
    let ways_per_invocation: serde_json::Map<String, serde_json::Value> = r
        .ways_per_invocation
        .iter()
        .map(|(channel, load)| {
            (
                channel.clone(),
                json!({
                    "invocations": load.invocations,
                    "1": load.buckets[0],
                    "2": load.buckets[1],
                    "3": load.buckets[2],
                    "4+": load.buckets[3],
                    "max": load.max,
                }),
            )
        })
        .collect();

    let output = json!({
        "total_events": r.total_events,
        "sessions": r.sessions,
        "way_fires": r.fires,
        "by_way": counts_json(&r.by_way),
        "by_trigger": counts_json(&r.by_trigger),
        "by_scope": counts_json(&r.by_scope),
        "by_model": by_model,
        "by_way_model": by_way_model,
        "ways_per_invocation": ways_per_invocation,
        "check_fires": r.check_fires,
        "by_check": counts_json(&r.by_check),
        "check_avg_distance": r.check_avg_distance,
        "check_anchored": r.check_anchored,
        "redisclosures": r.redisclosures,
        "redisclose_avg_token_distance": r.redisclose_avg_token_distance,
    });

    println!("{}", serde_json::to_string_pretty(&output).unwrap_or_default());
}

fn print_human(r: &StatsReport, days: Option<u32>, project_filter: Option<&str>) {
    let first_ts = r.first_ts.as_deref().map(|t| &t[..10]).unwrap_or("?");
    let last_ts = r.last_ts.as_deref().map(|t| &t[..10]).unwrap_or("?");

    println!("\nWays of Working — Usage Stats\n");

    if let Some(d) = days {
        println!("  Period:  last {d} days");
    } else if first_ts != last_ts {
        println!("  Period:  {first_ts} → {last_ts}");
    } else {
        println!("  Date:    {first_ts}");
    }
    if let Some(pf) = project_filter {
        println!("  Project: {pf}");
    }
    println!();
    println!(
        "  Sessions: {}  |  Way fires: {}  |  Re-disclosures: {}",
        r.sessions, r.fires, r.redisclosures
    );
    println!();

    // Top ways
    println!("Top ways:");
    let max = r.by_way.first().map(|(_, c)| *c).unwrap_or(1);

    for (way, count) in r.by_way.iter().take(10) {
        let bar_len = (*count as usize * 20) / max.max(1) as usize;
        let bar: String = "█".repeat(bar_len.max(1));
        println!("  {:<30} {:>3}  {bar}", way, count);
    }
    println!();

    // By trigger
    println!("By trigger:");
    let total_fires = (r.fires as usize).max(1);
    for (trigger, count) in &r.by_trigger {
        let pct = *count as usize * 100 / total_fires;
        println!("  {:<10} {:>3} ({pct}%)", trigger, count);
    }
    println!();

    // By model: fires and re-disclosures per model id stamped at fire time.
    let models = &r.by_model;
    if !models.is_empty() {
        println!("By model:");
        let mut t = Table::new(&["Model", "Fires", "Re-disclosures"]);
        t.no_auto_fit();
        t.align(1, Align::Right);
        t.align(2, Align::Right);
        for (model, tally) in models {
            t.add_owned(vec![
                model.clone(),
                tally.fires.to_string(),
                tally.redisclosures.to_string(),
            ]);
        }
        t.print();
        if models.iter().any(|(m, _)| m == UNSTAMPED) {
            println!("  {UNSTAMPED}: rows written before the model field existed.");
        }
        println!();

        // Top ways by model: the same top-ten ways, one column per model.
        let model_cols: Vec<&str> = models.iter().map(|(m, _)| m.as_str()).collect();
        let mut headers = vec!["Way"];
        headers.extend(model_cols.iter().copied());
        println!("Top ways by model:");
        let mut t = Table::new(&headers);
        t.no_auto_fit();
        for col in 1..headers.len() {
            t.align(col, Align::Right);
        }
        for (way, _) in r.by_way.iter().take(10) {
            let per_model = r.by_way_model.get(way);
            let mut row = vec![way.to_string()];
            for m in &model_cols {
                let n = per_model.and_then(|s| s.get(*m)).copied().unwrap_or(0);
                row.push(if n == 0 { "-".to_string() } else { n.to_string() });
            }
            t.add_owned(row);
        }
        t.print();
        println!();
    }

    // Ways per hook invocation: how many ways one hook call delivered.
    let loads = &r.ways_per_invocation;
    if !loads.is_empty() {
        println!("Ways per hook invocation:");
        let mut t = Table::new(&["Channel", "Invocations", "1", "2", "3", "4+", "Max"]);
        t.no_auto_fit();
        for col in 1..7 {
            t.align(col, Align::Right);
        }
        for (channel, load) in loads {
            t.add_owned(vec![
                channel.clone(),
                load.invocations.to_string(),
                load.buckets[0].to_string(),
                load.buckets[1].to_string(),
                load.buckets[2].to_string(),
                load.buckets[3].to_string(),
                load.max.to_string(),
            ]);
        }
        t.print();
        println!("  An invocation is the way_fired rows sharing session, agent, second, and channel.");
        println!();
    }

    // Check stats
    if r.check_fires > 0 {
        println!("Check fires: {}", r.check_fires);
        for (check, count) in r.by_check.iter().take(10) {
            println!("  {:<30} {:>3}", check, count);
        }
        println!();
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn events(lines: &[&str]) -> Vec<Event> {
        parse_events(&lines.join("\n"), None, None)
    }

    const FIRE_FABLE_A: &str = r#"{"ts":"2026-09-01T10:00:00Z","event":"way_fired","way":"d/a","trigger":"keyword","session":"s1","model":"claude-fable-5-1"}"#;
    const FIRE_FABLE_B: &str = r#"{"ts":"2026-09-01T10:00:00Z","event":"way_fired","way":"d/b","trigger":"semantic:embedding:en","session":"s1","model":"claude-fable-5-1"}"#;
    const FIRE_OPUS_A: &str = r#"{"ts":"2026-09-01T11:00:00Z","event":"way_fired","way":"d/a","trigger":"bash","session":"s2","model":"claude-opus-5"}"#;
    const REDISCLOSE_OPUS: &str = r#"{"ts":"2026-09-01T11:05:00Z","event":"way_redisclosed","way":"d/a","trigger":"bash","session":"s2","model":"claude-opus-5"}"#;
    const FIRE_UNSTAMPED: &str = r#"{"ts":"2026-08-01T10:00:00Z","event":"way_fired","way":"d/c","trigger":"file","session":"s0"}"#;

    #[test]
    fn model_breakdown_counts_fires_and_redisclosures_per_model() {
        let evs = events(&[FIRE_FABLE_A, FIRE_FABLE_B, FIRE_OPUS_A, REDISCLOSE_OPUS, FIRE_UNSTAMPED]);
        let rows = model_breakdown(&evs);
        assert_eq!(rows[0].0, "claude-fable-5-1");
        assert_eq!(rows[0].1, ModelTally { fires: 2, redisclosures: 0 });
        // Ties on fires sort by name: "(" precedes "c".
        assert_eq!(rows[1].0, UNSTAMPED);
        assert_eq!(rows[1].1, ModelTally { fires: 1, redisclosures: 0 });
        assert_eq!(rows[2].0, "claude-opus-5");
        assert_eq!(rows[2].1, ModelTally { fires: 1, redisclosures: 1 });
    }

    #[test]
    fn unknown_is_a_model_bucket_distinct_from_unstamped() {
        // Stamping ran and found nothing versus the field never existing:
        // both are visible, and separately, so resolution failures are measurable.
        let unknown = r#"{"ts":"2026-09-01T10:00:00Z","event":"way_fired","way":"d/a","trigger":"state","session":"s3","model":"unknown"}"#;
        let evs = events(&[unknown, FIRE_UNSTAMPED]);
        let names: Vec<String> = model_breakdown(&evs).into_iter().map(|(m, _)| m).collect();
        assert!(names.iter().any(|m| m == "unknown"));
        assert!(names.iter().any(|m| m == UNSTAMPED));
    }

    #[test]
    fn way_model_split_counts_fires_only() {
        let evs = events(&[FIRE_FABLE_A, FIRE_OPUS_A, REDISCLOSE_OPUS]);
        let split = way_model_split(&evs);
        let a = &split["d/a"];
        assert_eq!(a["claude-fable-5-1"], 1);
        assert_eq!(a["claude-opus-5"], 1, "the redisclosure must not count");
    }

    #[test]
    fn trigger_channel_folds_lanes() {
        assert_eq!(trigger_channel("keyword"), "prompt");
        assert_eq!(trigger_channel("semantic:embedding:en"), "prompt");
        assert_eq!(trigger_channel("semantic:embedding:multi"), "prompt");
        assert_eq!(trigger_channel("semantic:late-interaction:en"), "prompt");
        assert_eq!(trigger_channel("bash"), "bash");
        assert_eq!(trigger_channel("semantic:bash:en"), "bash");
        // One early row in local history spells the bash semantic channel this
        // way; the first-segment fallback lands it with the rest of the lane.
        assert_eq!(trigger_channel("bash:semantic:en"), "bash");
        assert_eq!(trigger_channel("file"), "file");
        assert_eq!(trigger_channel("state"), "state");
        assert_eq!(trigger_channel("attend:context-pressure"), "attend");
        assert_eq!(trigger_channel("check-pull"), "check-pull");
    }

    #[test]
    fn ways_per_invocation_groups_by_session_agent_second_and_channel() {
        // s1 at 10:00:00 fired two ways on the prompt lane (keyword + semantic)
        // in one invocation; s2 fired one way on bash. A second-later fire in
        // s1 is a separate invocation.
        let later = r#"{"ts":"2026-09-01T10:00:01Z","event":"way_fired","way":"d/c","trigger":"keyword","session":"s1","model":"claude-fable-5-1"}"#;
        let evs = events(&[FIRE_FABLE_A, FIRE_FABLE_B, FIRE_OPUS_A, REDISCLOSE_OPUS, later]);
        let loads = ways_per_invocation(&evs);
        let prompt = &loads.iter().find(|(c, _)| c == "prompt").unwrap().1;
        assert_eq!(prompt.invocations, 2);
        assert_eq!(prompt.buckets, [1, 1, 0, 0]);
        assert_eq!(prompt.max, 2);
        let bash = &loads.iter().find(|(c, _)| c == "bash").unwrap().1;
        assert_eq!(bash.invocations, 1, "the redisclosure is not a way_fired row");
        assert_eq!(bash.buckets, [1, 0, 0, 0]);
        assert_eq!(bash.max, 1);
    }

    #[test]
    fn parallel_agents_under_one_session_are_separate_invocations() {
        // Subagent hooks report the parent's session id. Three agents each
        // firing two bash ways in the same second are three invocations of
        // two, not one invocation of six.
        let rows: Vec<String> = ["main", "agent-a", "agent-b"]
            .iter()
            .flat_map(|agent| {
                (0..2).map(move |i| {
                    format!(
                        r#"{{"ts":"2026-09-01T12:00:00Z","event":"way_fired","way":"d/w{i}","trigger":"bash","session":"parent","agent_id":"{agent}"}}"#
                    )
                })
            })
            .collect();
        let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
        let loads = ways_per_invocation(&events(&refs));
        let bash = &loads.iter().find(|(c, _)| c == "bash").unwrap().1;
        assert_eq!(bash.invocations, 3);
        assert_eq!(bash.buckets, [0, 3, 0, 0]);
        assert_eq!(bash.max, 2);
    }

    #[test]
    fn ways_per_invocation_tail_bucket_is_open_ended() {
        let rows: Vec<String> = (0..7)
            .map(|i| {
                format!(
                    r#"{{"ts":"2026-09-01T12:00:00Z","event":"way_fired","way":"d/w{i}","trigger":"semantic:bash:en","session":"s9"}}"#
                )
            })
            .collect();
        let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
        let loads = ways_per_invocation(&events(&refs));
        let bash = &loads.iter().find(|(c, _)| c == "bash").unwrap().1;
        assert_eq!(bash.invocations, 1);
        assert_eq!(bash.buckets, [0, 0, 0, 1]);
        assert_eq!(bash.max, 7);
    }

    #[test]
    fn invocation_rows_sort_by_invocations_then_name() {
        let evs = events(&[FIRE_FABLE_A, FIRE_FABLE_B, FIRE_OPUS_A, FIRE_UNSTAMPED]);
        let channels: Vec<String> = ways_per_invocation(&evs).into_iter().map(|(c, _)| c).collect();
        // One invocation each: alphabetical.
        assert_eq!(channels, vec!["bash", "file", "prompt"]);
    }

    #[test]
    fn report_aggregates_a_small_log() {
        let rows = [
            json!({"ts":"2026-09-01T09:00:00Z","event":"session_start","session":"s1"}),
            json!({"ts":"2026-09-01T10:00:00Z","event":"way_fired","way":"d/a","trigger":"keyword","scope":"project","session":"s1","model":"claude-fable-5-1"}),
            json!({"ts":"2026-09-01T10:00:00Z","event":"way_fired","way":"d/b","trigger":"semantic:embedding:en","scope":"project","session":"s1","model":"claude-fable-5-1"}),
            json!({"ts":"2026-09-01T11:00:00Z","event":"way_fired","way":"d/a","trigger":"bash","session":"s2"}),
            json!({"ts":"2026-09-01T11:05:00Z","event":"way_redisclosed","way":"d/a","session":"s2","token_distance":"0.5"}),
            json!({"ts":"2026-09-02T08:00:00Z","event":"check_fired","check":"c/x","distance":"0.25","anchored":"true"}),
            json!({"ts":"2026-09-02T08:01:00Z","event":"check_fired","check":"c/x","distance":"0.75"}),
        ];
        let text: Vec<String> = rows.iter().map(|r| r.to_string()).collect();
        let r = report(&text.join("\n"), None, None);
        assert_eq!(r.total_events, 7);
        assert_eq!((r.sessions, r.fires, r.redisclosures), (1, 3, 1));
        assert_eq!(r.first_ts.as_deref(), Some("2026-09-01T09:00:00Z"));
        assert_eq!(r.last_ts.as_deref(), Some("2026-09-02T08:01:00Z"));
        assert_eq!(r.by_way, vec![("d/a".to_string(), 2), ("d/b".to_string(), 1)]);
        assert_eq!(r.by_trigger.len(), 3);
        assert_eq!(r.by_scope, vec![("project".to_string(), 2), ("unknown".to_string(), 1)]);
        assert_eq!(r.check_fires, 2);
        assert_eq!(r.by_check, vec![("c/x".to_string(), 2)]);
        assert_eq!(r.check_avg_distance, 0.5);
        assert_eq!(r.check_anchored, 1);
        assert_eq!(r.redisclose_avg_token_distance, 0.5);
        assert_eq!(r.by_model[0].0, "claude-fable-5-1");
        assert_eq!(r.by_model[0].1, ModelTally { fires: 2, redisclosures: 0 });
        assert_eq!(r.by_way_model["d/a"][UNSTAMPED], 1);
        // One invocation each, so alphabetical; the prompt one fired two ways.
        assert_eq!(r.ways_per_invocation[1].0, "prompt");
        assert_eq!(r.ways_per_invocation[1].1.max, 2);
    }
}
