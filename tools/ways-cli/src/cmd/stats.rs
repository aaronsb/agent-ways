//! Usage statistics from ~/.claude/stats/events.jsonl.
//! Replaces stats.sh (348 lines).

use anyhow::Result;
use serde_json::json;
use std::collections::HashMap;

pub fn run(days: Option<u32>, project_filter: Option<&str>, json_output: bool, global: bool) -> Result<()> {
    // Default to project scope: CLAUDE_PROJECT_DIR > detect from cwd > global
    let detected_project = if !global && project_filter.is_none() {
        std::env::var("CLAUDE_PROJECT_DIR")
            .ok()
            .or_else(detect_project_dir)
    } else {
        None
    };
    let project_filter = project_filter.or(detected_project.as_deref());
    let stats_file = crate::paths::events_log();

    if !stats_file.is_file() {
        if !json_output {
            println!("No events recorded yet. Stats will appear after ways start firing.");
        }
        return Ok(());
    }

    let content = std::fs::read_to_string(&stats_file)?;
    let events = parse_events(&content, days, project_filter);

    if json_output {
        print_json(&events);
    } else {
        print_human(&events, days, project_filter);
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
    check: String,
    distance: f64,
    anchored: bool,
    token_distance: f64,
}

fn parse_events(content: &str, days: Option<u32>, project_filter: Option<&str>) -> Vec<Event> {
    let cutoff = days.map(|d| {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            - (d as u64 * 86400);
        format_ts(secs)
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

fn print_json(events: &[Event]) {
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

    let output = json!({
        "total_events": events.len(),
        "sessions": sessions,
        "way_fires": fires,
        "by_way": by_way,
        "by_trigger": by_trigger,
        "by_scope": by_scope,
        "check_fires": check_fires,
        "by_check": by_check,
        "check_avg_distance": avg_check_dist,
        "check_anchored": check_anchored,
        "redisclosures": redisclosures,
        "redisclose_avg_token_distance": avg_redisclose_dist,
    });

    println!("{}", serde_json::to_string_pretty(&output).unwrap_or_default());
}

fn print_human(events: &[Event], days: Option<u32>, project_filter: Option<&str>) {
    let fires: Vec<&Event> = events.iter().filter(|e| e.event == "way_fired").collect();
    let sessions: u32 = events
        .iter()
        .filter(|e| e.event == "session_start")
        .count() as u32;
    let redisclosures: u32 = events
        .iter()
        .filter(|e| e.event == "way_redisclosed")
        .count() as u32;

    let first_ts = events.first().map(|e| &e.ts[..10]).unwrap_or("?");
    let last_ts = events.last().map(|e| &e.ts[..10]).unwrap_or("?");

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
        sessions,
        fires.len(),
        redisclosures
    );
    println!();

    // Top ways
    println!("Top ways:");
    let mut way_counts: HashMap<&str, u32> = HashMap::new();
    for f in &fires {
        *way_counts.entry(&f.way).or_insert(0) += 1;
    }
    let mut sorted: Vec<(&&str, &u32)> = way_counts.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));
    let max = sorted.first().map(|(_, c)| **c).unwrap_or(1);

    for (way, count) in sorted.iter().take(10) {
        let bar_len = (**count as usize * 20) / max.max(1) as usize;
        let bar: String = "█".repeat(bar_len.max(1));
        println!("  {:<30} {:>3}  {bar}", way, count);
    }
    println!();

    // By trigger
    println!("By trigger:");
    let mut trigger_counts: HashMap<&str, u32> = HashMap::new();
    for f in &fires {
        *trigger_counts.entry(&f.trigger).or_insert(0) += 1;
    }
    let mut sorted_t: Vec<(&&str, &u32)> = trigger_counts.iter().collect();
    sorted_t.sort_by(|a, b| b.1.cmp(a.1));
    let total_fires = fires.len().max(1);
    for (trigger, count) in &sorted_t {
        let pct = **count as usize * 100 / total_fires;
        println!("  {:<10} {:>3} ({pct}%)", trigger, count);
    }
    println!();

    // Check stats
    let check_events: Vec<&Event> = events.iter().filter(|e| e.event == "check_fired").collect();
    if !check_events.is_empty() {
        println!("Check fires: {}", check_events.len());
        let mut check_counts: HashMap<&str, u32> = HashMap::new();
        for c in &check_events {
            *check_counts.entry(&c.check).or_insert(0) += 1;
        }
        let mut sorted_c: Vec<(&&str, &u32)> = check_counts.iter().collect();
        sorted_c.sort_by(|a, b| b.1.cmp(a.1));
        for (check, count) in sorted_c.iter().take(10) {
            println!("  {:<30} {:>3}", check, count);
        }
        println!();
    }
}

fn format_ts(secs: u64) -> String {
    let days = secs / 86400;
    let tod = secs % 86400;
    let (y, m, d) = days_to_ymd(days);
    let h = tod / 3600;
    let min = (tod % 3600) / 60;
    let s = tod % 60;
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{min:02}:{s:02}Z")
}

fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

use crate::util::detect_project_dir;
