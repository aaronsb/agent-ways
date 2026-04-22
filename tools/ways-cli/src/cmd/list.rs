//! List ways triggered in the current session with epoch and disclosure state.
//! Shows conversation-ordered progression of way firings, epoch distances,
//! check fire counts, and predicted next-allowed-fire epochs.

use anyhow::Result;
use serde_json::json;
use std::collections::HashMap;

use crate::cmd::context;
use crate::cmd::render::{self, WayRow};
use crate::session;

/// A fired way with all its session state.
struct FiredWay {
    id: String,
    epoch_at_fire: u64,
    token_pos: u64,
    trigger: String,
    depth: u64,
    check_fires: u64,
    #[allow(dead_code)]
    parent: String,
    agent_id: String,
    /// Per-way re-fire distance in thousands of tokens, derived from
    /// the way's frontmatter `curve:` block via
    /// [`sensor_trait::Curve::refire_delta`]. Falls back to 25% of the
    /// context window for ways whose frontmatter is missing or cannot
    /// be parsed (shouldn't happen post-C2 but keeps the renderer
    /// robust).
    refire_threshold_k: u64,
}

impl WayRow for FiredWay {
    fn id(&self) -> &str { &self.id }
    fn epoch_fired(&self) -> u64 { self.epoch_at_fire }
    fn token_pos(&self) -> u64 { self.token_pos }
    fn trigger(&self) -> &str { &self.trigger }
    fn check_fires(&self) -> u64 { self.check_fires }
    fn depth(&self) -> u64 { self.depth }
    fn agent_id(&self) -> &str { &self.agent_id }
    fn refire_threshold_k(&self) -> u64 { self.refire_threshold_k }
}

pub fn run(session: Option<&str>, sort: &str, json_out: bool) -> Result<()> {
    // Auto-detect session if not provided
    let session_id = match session {
        Some(s) => s.to_string(),
        None => match detect_session() {
            Some(s) => s,
            None => {
                println!("No session markers found. Ways will appear after the first hook fires.");
                return Ok(());
            }
        },
    };

    let current_epoch = session::get_epoch(&session_id);

    // Use accurate context data from transcript when available
    let (current_tokens_k, context_window_k, context_window) = match context::get_context(None) {
        Ok(ctx) => (ctx.tokens_used / 1000, ctx.tokens_total / 1000, ctx.tokens_total),
        Err(_) => {
            // Fallback to session markers
            let tok = session::get_token_position(&session_id) / 1000;
            let window_k = if tok > 200 { 1000 } else { 200 };
            (tok, window_k, window_k * 1000)
        }
    };
    // Fallback refire threshold for ways with missing/unparsable curves —
    // matches the pre-ADR-123 visual baseline of 25% of the context window.
    let fallback_refire_k = context_window_k * 25 / 100;
    let project_dir = std::env::var("CLAUDE_PROJECT_DIR")
        .unwrap_or_else(|_| std::env::var("PWD").unwrap_or_else(|_| ".".to_string()));

    // Collect metrics from JSONL (has trigger, depth, parent)
    let metrics = load_metrics(&session_id);

    // Collect all fired ways from markers
    let mut ways = collect_fired_ways(
        &session_id,
        &metrics,
        &project_dir,
        fallback_refire_k,
        context_window,
    );

    if ways.is_empty() {
        println!("No ways triggered yet this session.");
        return Ok(());
    }

    // Sort
    match sort {
        "name" => ways.sort_by(|a, b| a.id.cmp(&b.id)),
        "distance" => ways.sort_by(|a, b| {
            let da = current_epoch.saturating_sub(a.epoch_at_fire);
            let db = current_epoch.saturating_sub(b.epoch_at_fire);
            db.cmp(&da) // highest distance first
        }),
        _ => ways.sort_by_key(|w| w.epoch_at_fire), // epoch = conversation order
    }

    if json_out {
        print_json(&ways, current_epoch, current_tokens_k, context_window_k);
        return Ok(());
    }

    // Render to buffer then print
    let mut out = String::new();
    use std::fmt::Write;

    // Header
    let short_id = &session_id[..session_id.len().min(12)];
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "\x1b[1mSession\x1b[0m {short_id}...  \x1b[2mepoch {current_epoch} · {context_window_k}K ctx · {} ways fired\x1b[0m",
        ways.len()
    );
    let _ = writeln!(out);

    let bar_positions = render::compute_bar_positions(&ways, context_window_k);
    let unique_pos = render::unique_positions(&bar_positions);

    render::write_table_header(&mut out);

    for (i, w) in ways.iter().enumerate() {
        render::write_way_row(
            &mut out, w, current_epoch, current_tokens_k,
            &bar_positions, &unique_pos, i, "", "",
        );
    }

    // Token timeline with re-disclosure markers
    if current_tokens_k > 0 {
        let _ = writeln!(out);
        render::write_token_timeline(
            &mut out, &ways, &unique_pos,
            current_tokens_k, context_window_k,
        );
    }

    let _ = writeln!(out);
    print!("{out}");
    Ok(())
}

// ── Data collection ────────────────────────────────────────────

fn collect_fired_ways(
    session_id: &str,
    metrics: &HashMap<String, MetricEntry>,
    project_dir: &str,
    fallback_refire_k: u64,
    context_window: u64,
) -> Vec<FiredWay> {
    let way_epochs = session::list_way_epochs(session_id);

    way_epochs
        .into_iter()
        .map(|(way_id, epoch_at_fire)| {
            let token_pos = session::get_token_position_for_way(&way_id, session_id);
            let check_fires = session::get_check_fires(&way_id, session_id);

            let (trigger, depth, parent, agent_id) = metrics
                .get(&way_id)
                .map(|m| (m.trigger.clone(), m.depth, m.parent.clone(), m.agent_id.clone()))
                .unwrap_or_else(|| ("unknown".to_string(), 0, "none".to_string(), "main".to_string()));

            let refire_threshold_k = session::way_refire_threshold_k(&way_id, project_dir, context_window)
                .unwrap_or(fallback_refire_k);

            FiredWay {
                id: way_id,
                epoch_at_fire,
                token_pos,
                trigger,
                depth,
                check_fires,
                parent,
                agent_id,
                refire_threshold_k,
            }
        })
        .collect()
}

struct MetricEntry {
    trigger: String,
    depth: u64,
    parent: String,
    agent_id: String,
}

fn load_metrics(session_id: &str) -> HashMap<String, MetricEntry> {
    let path = format!("{}/{session_id}/metrics.jsonl", crate::session::sessions_root());
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };

    let mut map = HashMap::new();
    for line in content.lines() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(way) = v["way"].as_str() {
                map.insert(
                    way.to_string(),
                    MetricEntry {
                        trigger: v["trigger"].as_str().unwrap_or("unknown").to_string(),
                        depth: v["depth"].as_u64().unwrap_or(0),
                        parent: v["parent"].as_str().unwrap_or("none").to_string(),
                        agent_id: v["agent_id"].as_str().unwrap_or("main").to_string(),
                    },
                );
            }
        }
    }
    map
}

fn detect_session() -> Option<String> {
    let project = std::env::var("CLAUDE_PROJECT_DIR")
        .ok()
        .or_else(detect_project_dir);

    if let Some(ref proj) = project {
        if let Some(sid) = latest_session_for_project(proj) {
            let dir = format!("{}/{sid}", crate::session::sessions_root());
            if std::path::Path::new(&dir).is_dir() {
                return Some(sid);
            }
        }
    }

    let sessions = session::list_sessions();
    if sessions.is_empty() {
        return None;
    }
    if sessions.len() == 1 {
        return Some(sessions.into_iter().next().unwrap());
    }

    let mut newest: Option<(std::time::SystemTime, String)> = None;
    for sid in sessions {
        let dir = format!("{}/{sid}", crate::session::sessions_root());
        if let Ok(meta) = std::fs::metadata(&dir) {
            let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
            if newest.as_ref().is_none_or(|(t, _)| mtime > *t) {
                newest = Some((mtime, sid));
            }
        }
    }
    newest.map(|(_, s)| s)
}

fn latest_session_for_project(project: &str) -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let path = format!("{home}/.claude/stats/events.jsonl");
    let content = std::fs::read_to_string(&path).ok()?;

    let mut latest: Option<String> = None;
    for line in content.lines() {
        if !line.contains("session_start") {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            if v["event"].as_str() == Some("session_start") {
                if let Some(p) = v["project"].as_str() {
                    if p == project {
                        if let Some(s) = v["session"].as_str() {
                            latest = Some(s.to_string());
                        }
                    }
                }
            }
        }
    }
    latest
}

use crate::util::detect_project_dir;

fn print_json(ways: &[FiredWay], current_epoch: u64, current_tokens_k: u64, context_window_k: u64) {
    let entries: Vec<serde_json::Value> = ways
        .iter()
        .map(|w| {
            let distance = current_epoch.saturating_sub(w.epoch_at_fire);
            let token_pos_k = w.token_pos / 1000;
            let token_distance_k = current_tokens_k.saturating_sub(token_pos_k);
            let token_pct = if w.refire_threshold_k > 0 {
                token_distance_k * 100 / w.refire_threshold_k
            } else {
                0
            };
            json!({
                "id": w.id,
                "epoch_at_fire": w.epoch_at_fire,
                "epoch_distance": distance,
                "token_pos_k": token_pos_k,
                "token_distance_k": token_distance_k,
                "refire_threshold_k": w.refire_threshold_k,
                "refire_pct": token_pct,
                "trigger": w.trigger,
                "depth": w.depth,
                "check_fires": w.check_fires,
                "parent": w.parent,
            })
        })
        .collect();

    let output = json!({
        "session": "current",
        "current_epoch": current_epoch,
        "current_tokens_k": current_tokens_k,
        "context_window_k": context_window_k,
        "ways_fired": entries.len(),
        "ways": entries,
    });

    println!("{}", serde_json::to_string_pretty(&output).unwrap_or_default());
}
