//! Non-interactive JSON dump of a session's way-firing timeline.
//!
//! `rethink` renders the replay through a TUI; this module emits the same
//! reconstructed timeline — plus a session summary and the near-miss events the
//! TUI omits — as a single JSON document on stdout, for agents and scripts.
//! It depends on no terminal feature, so it runs in headless / CI contexts
//! where the interactive replay can't.

use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;

use super::rethink;
use crate::session;
use crate::util::detect_project_dir;

// ── Output shape ──────────────────────────────────────────────

#[derive(Serialize)]
struct SessionDump {
    session: String,
    project: String,
    context_window_k: u64,
    summary: Summary,
    frames: Vec<DumpFrame>,
    near_misses: Vec<NearMiss>,
}

#[derive(Serialize)]
struct Summary {
    epochs: usize,
    duration_secs: u64,
    distinct_ways: usize,
    total_fires: u64,
    redisclosures: u64,
    checks_fired: u64,
    near_misses: u64,
    trigger_breakdown: BTreeMap<String, u64>,
    top_ways: Vec<TopWay>,
}

#[derive(Serialize)]
struct TopWay {
    way: String,
    fires: u64,
}

#[derive(Serialize)]
struct DumpFrame {
    epoch: u64,
    timestamp: String,
    elapsed_secs: u64,
    token_position_k: u64,
    active_ways: Vec<DumpWay>,
    new_events: Vec<String>,
}

#[derive(Serialize)]
struct DumpWay {
    id: String,
    trigger: String,
    epoch_fired: u64,
    token_pos_k: u64,
    check_fires: u64,
    is_new: bool,
    is_redisclosed: bool,
    refire_threshold_k: u64,
}

#[derive(Serialize)]
struct NearMiss {
    epoch: u64,
    timestamp: String,
    way: String,
    score_en: f64,
    score_multi: f64,
    thr_en: f64,
    thr_multi: f64,
    margin: f64,
}

// ── Entry point ───────────────────────────────────────────────

/// Emit a session's reconstructed timeline as a single pretty-printed JSON
/// document. With no `session`, dumps the most recent session in scope.
pub fn run_json(session: Option<&str>, project: Option<&str>) -> Result<()> {
    let events_file = crate::paths::events_log();
    if !events_file.is_file() {
        println!("{{\"error\":\"no events recorded yet\"}}");
        return Ok(());
    }
    let content = std::fs::read_to_string(&events_file)?;

    // Auto-detect project scope when not given, mirroring `rethink::run`.
    let detected = if project.is_none() {
        std::env::var("CLAUDE_PROJECT_DIR")
            .ok()
            .or_else(detect_project_dir)
    } else {
        None
    };
    let scope = project.map(|s| s.to_string()).or(detected);

    let session_id = match session {
        Some(s) => s.to_string(),
        None => match most_recent_session(&content, scope.as_deref()) {
            Some(s) => s,
            None => {
                println!("{{\"error\":\"no sessions found\"}}");
                return Ok(());
            }
        }
    };

    match build_dump(&content, &session_id) {
        Some(dump) => println!("{}", serde_json::to_string_pretty(&dump)?),
        None => println!("{{\"error\":\"no events for session\",\"session\":\"{session_id}\"}}"),
    }
    Ok(())
}

// ── Assembly ──────────────────────────────────────────────────

fn build_dump(content: &str, session_id: &str) -> Option<SessionDump> {
    let project =
        rethink::find_session_project(content, session_id).unwrap_or_else(|| "unknown".to_string());
    let events = rethink::load_session_events(content, session_id);
    if events.is_empty() {
        return None;
    }

    let context_window = session::detect_context_window_for(&project, session_id);
    let frames = rethink::reconstruct_frames(&events, &project, session_id, context_window);

    let near_misses = build_near_misses(content, session_id, &frames);
    let summary = build_summary(content, session_id, &frames, near_misses.len());
    let dump_frames = frames.iter().map(to_dump_frame).collect();

    Some(SessionDump {
        session: session_id.to_string(),
        project,
        context_window_k: context_window / 1000,
        summary,
        frames: dump_frames,
        near_misses,
    })
}

fn to_dump_frame(f: &rethink::Frame) -> DumpFrame {
    let active_ways = f
        .ways
        .iter()
        .map(|w| DumpWay {
            id: w.id.clone(),
            trigger: w.trigger.clone(),
            epoch_fired: w.epoch_fired,
            token_pos_k: w.token_pos / 1000,
            check_fires: w.check_fires,
            is_new: w.is_new,
            is_redisclosed: w.is_redisclosed,
            refire_threshold_k: w.refire_threshold_k,
        })
        .collect();
    DumpFrame {
        epoch: f.epoch,
        timestamp: f.timestamp.clone(),
        elapsed_secs: f.elapsed_secs,
        token_position_k: f.token_position_k,
        active_ways,
        new_events: f.new_events.clone(),
    }
}

fn build_summary(
    content: &str,
    session_id: &str,
    frames: &[rethink::Frame],
    near_miss_count: usize,
) -> Summary {
    let mut total_fires = 0u64;
    let mut redisclosures = 0u64;
    let mut checks_fired = 0u64;
    let mut triggers: BTreeMap<String, u64> = BTreeMap::new();
    let mut way_fires: BTreeMap<String, u64> = BTreeMap::new();

    for v in session_events(content, session_id) {
        match v["event"].as_str() {
            Some("way_fired") => {
                total_fires += 1;
                if let Some(t) = v["trigger"].as_str() {
                    *triggers.entry(t.to_string()).or_insert(0) += 1;
                }
                if let Some(w) = v["way"].as_str() {
                    *way_fires.entry(w.to_string()).or_insert(0) += 1;
                }
            }
            Some("way_redisclosed") => redisclosures += 1,
            Some("check_fired") => checks_fired += 1,
            _ => {}
        }
    }

    let mut top_ways: Vec<TopWay> = way_fires
        .iter()
        .map(|(w, n)| TopWay {
            way: w.clone(),
            fires: *n,
        })
        .collect();
    top_ways.sort_by(|a, b| b.fires.cmp(&a.fires).then(a.way.cmp(&b.way)));
    top_ways.truncate(15);

    Summary {
        epochs: frames.len(),
        duration_secs: frames.last().map(|f| f.elapsed_secs).unwrap_or(0),
        distinct_ways: way_fires.len(),
        total_fires,
        redisclosures,
        checks_fired,
        near_misses: near_miss_count as u64,
        trigger_breakdown: triggers,
        top_ways,
    }
}

fn build_near_misses(content: &str, session_id: &str, frames: &[rethink::Frame]) -> Vec<NearMiss> {
    session_events(content, session_id)
        .filter(|v| v["event"].as_str() == Some("way_nearmiss"))
        .map(|v| {
            let ts = v["ts"].as_str().unwrap_or("").to_string();
            NearMiss {
                epoch: epoch_for_ts(frames, &ts),
                timestamp: ts,
                way: v["way"].as_str().unwrap_or("").to_string(),
                score_en: field_f64(&v, "score_en"),
                score_multi: field_f64(&v, "score_multi"),
                thr_en: field_f64(&v, "thr_en"),
                thr_multi: field_f64(&v, "thr_multi"),
                margin: field_f64(&v, "margin"),
            }
        })
        .collect()
}

// ── Parsing helpers ───────────────────────────────────────────

/// Parsed events.jsonl rows belonging to one session.
fn session_events<'a>(
    content: &'a str,
    session_id: &'a str,
) -> impl Iterator<Item = serde_json::Value> + 'a {
    content
        .lines()
        .filter(move |l| l.contains(session_id))
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(move |v| v["session"].as_str() == Some(session_id))
}

/// Latest session (by session_start timestamp) within an optional project scope.
fn most_recent_session(content: &str, scope: Option<&str>) -> Option<String> {
    let mut best: Option<(String, String)> = None; // (ts, session)
    for line in content.lines() {
        if !line.contains("session_start") {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if v["event"].as_str() != Some("session_start") {
            continue;
        }
        let sid = match v["session"].as_str() {
            Some(s) if !s.is_empty() => s,
            _ => continue,
        };
        if let Some(sc) = scope {
            if !v["project"].as_str().unwrap_or("").contains(sc) {
                continue;
            }
        }
        let ts = v["ts"].as_str().unwrap_or("").to_string();
        if best.as_ref().map(|(b, _)| ts > *b).unwrap_or(true) {
            best = Some((ts, sid.to_string()));
        }
    }
    best.map(|(_, s)| s)
}

/// Map a near-miss timestamp to the epoch of the frame it falls within —
/// the last frame whose timestamp is at or before it.
fn epoch_for_ts(frames: &[rethink::Frame], ts: &str) -> u64 {
    let mut epoch = frames.first().map(|f| f.epoch).unwrap_or(0);
    for f in frames {
        if f.timestamp.as_str() <= ts {
            epoch = f.epoch;
        } else {
            break;
        }
    }
    epoch
}

/// Numeric fields in events.jsonl are stored as strings ("0.1693"); accept
/// either a stringified or native number.
fn field_f64(v: &serde_json::Value, key: &str) -> f64 {
    v[key]
        .as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| v[key].as_f64())
        .unwrap_or(0.0)
}
