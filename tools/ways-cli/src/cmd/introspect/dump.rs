//! The replay as JSON, and the session lists agents pick from.
//!
//! `session replay` draws a session's frames on the screen; `replay
//! --json` writes the same reconstructed timeline, with a session summary,
//! the relevance gate's work and the near-miss events the screen omits, as
//! one JSON document on stdout, for agents and scripts. It needs no
//! terminal, so it runs where the screen cannot (ADR-504 §10).

use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;

use super::agents::Agents;
use super::model::Suppression;
use super::{frames, scope, sessions, Frame};
use crate::session;

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
    gate: GateSummary,
    /// What the subagent switch held back (#786); new after `gate`, so
    /// the fields before it read as they did.
    suppressed: SuppressedSummary,
}

/// The `injection_suppressed` events of the session: each Task dispatch
/// and each agent the subagent switch kept ways from, and by which switch.
#[derive(Serialize, Default)]
struct SuppressedSummary {
    total: u64,
    dispatches: u64,
    agents: u64,
    by_switch: BTreeMap<String, u64>,
}

/// The relevance gate's work in this session (ADR-196): what it judged, what
/// it kept out, and when it could not answer.
#[derive(Serialize, Default)]
struct GateSummary {
    judged: u64,
    passed: u64,
    blocked: u64,
    would_block: u64,
    /// Ways past the profile's cap, passed without a verdict.
    unjudged: u64,
    fallbacks: BTreeMap<String, u64>,
    gate_ms_p50: Option<u64>,
    gate_ms_p95: Option<u64>,
    blocked_ways: BTreeMap<String, u64>,
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
    /// The suppressions in this frame (#786); left out when there are none.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    suppressed: Vec<Suppression>,
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
    /// `injected`, `blocked` (the judge kept it out) or `would_block`
    /// (shadow: injected, the judge would have kept it out).
    outcome: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    p_yes: Option<String>,
    /// On a way blocked with its ancestor, the ancestor whose P(yes) it shows.
    #[serde(skip_serializing_if = "Option::is_none")]
    ancestor: Option<String>,
    /// The agent it was injected into (#814): `main` or the subagent's id;
    /// left out on a blocked row whose verdict did not name one. A way
    /// several agents fired is an entry per agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    agent: Option<String>,
    /// The subagent's `subagent_type` from its transcript, or
    /// `workflow-subagent` for a workflow member; left out when unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_type: Option<String>,
}

#[derive(Serialize)]
struct NearMiss {
    epoch: u64,
    timestamp: String,
    way: String,
    prob_en: f64,
    prob_multi: f64,
    tau_s: f64,
    margin: f64,
}

// ── Entry point ───────────────────────────────────────────────

/// Emit a session's reconstructed timeline as a single pretty-printed JSON
/// document. With no `session`, dumps the most recent session in scope.
pub fn replay_json(session: Option<&str>, project: Option<&str>, all: bool, matched: bool) -> Result<()> {
    let content = ways_core::firing::load_events_text();
    if content.trim().is_empty() {
        println!("{{\"error\":\"no events recorded yet\"}}");
        return Ok(());
    }

    // Scope to the current project by default; emit a JSON error (not a bail) so
    // agent consumers get structured output even on the fail-loud path.
    let scope = match scope::resolve_project_scope(project, all) {
        Ok(s) => s,
        Err(e) => {
            println!("{{\"error\":{}}}", serde_json::to_string(&e.to_string())?);
            return Ok(());
        }
    };

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

    match build_dump(&content, &session_id, matched) {
        Some(dump) => println!("{}", serde_json::to_string_pretty(&dump)?),
        None => println!("{{\"error\":\"no events for session\",\"session\":\"{session_id}\"}}"),
    }
    Ok(())
}

/// `ways session list --json`: enumerate candidate sessions in scope as
/// structured data, so an agent can pick one before dumping it (ADR-154 §4).
/// Newest first. `scope` is null when `--all` was passed.
pub fn run_list_json(project: Option<&str>, all: bool) -> Result<()> {
    let content = ways_core::firing::load_events_text();
    let scope = match scope::resolve_project_scope(project, all) {
        Ok(s) => s,
        Err(e) => {
            println!("{{\"error\":{}}}", serde_json::to_string(&e.to_string())?);
            return Ok(());
        }
    };
    let mut sessions = sessions::gather_sessions(&content, scope.as_deref());
    sessions.sort_by(|a, b| b.ts.cmp(&a.ts)); // newest first
    // `last_write` and `live`: one stat of each transcript, as the screen's
    // first pass makes (#780).
    sessions::find_transcripts(&mut sessions, &ways_core::paths::claude_dir());
    sessions::mark_live(&mut sessions, &super::live::stat_file, (super::live::system_clock())());
    let out = serde_json::json!({
        "scope": scope,
        "count": sessions.len(),
        "sessions": sessions,
    });
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

// ── Assembly ──────────────────────────────────────────────────

/// `matched` keeps the ways the relevance judge blocked; without it a
/// frame lists what reached the session.
fn build_dump(content: &str, session_id: &str, matched: bool) -> Option<SessionDump> {
    let project =
        frames::find_session_project(content, session_id).unwrap_or_else(|| "unknown".to_string());
    let events = frames::load_session_events(content, session_id);
    if events.is_empty() {
        return None;
    }

    let context_window = session::detect_context_window_for(&project, session_id);
    let frames = frames::reconstruct_frames(&events, &project, session_id, context_window, matched);

    let near_misses = build_near_misses(content, session_id, &frames);
    let summary = build_summary(content, session_id, &frames, near_misses.len());
    let transcript = ways_core::paths::claude_dir().find_transcript(Some(&project), session_id);
    let agents = Agents::read(&events, transcript.as_deref());
    let dump_frames = frames.iter().map(|f| to_dump_frame(f, &agents)).collect();

    Some(SessionDump {
        session: session_id.to_string(),
        project,
        context_window_k: context_window / 1000,
        summary,
        frames: dump_frames,
        near_misses,
    })
}

fn to_dump_frame(f: &Frame, agents: &Agents) -> DumpFrame {
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
            outcome: w.outcome.as_str(),
            p_yes: Some(w.p_yes.clone()).filter(|p| !p.is_empty()),
            ancestor: Some(w.ancestor.clone()).filter(|a| !a.is_empty()),
            agent: Some(w.agent.clone()).filter(|a| !a.is_empty()),
            agent_type: agents.agent_type(&w.agent).map(str::to_string),
        })
        .collect();
    DumpFrame {
        epoch: f.epoch,
        timestamp: f.timestamp.clone(),
        elapsed_secs: f.elapsed_secs,
        token_position_k: f.token_position_k,
        active_ways,
        new_events: f.new_events.clone(),
        suppressed: f.suppressed.clone(),
    }
}

fn build_summary(
    content: &str,
    session_id: &str,
    frames: &[Frame],
    near_miss_count: usize,
) -> Summary {
    let mut total_fires = 0u64;
    let mut redisclosures = 0u64;
    let mut checks_fired = 0u64;
    let mut triggers: BTreeMap<String, u64> = BTreeMap::new();
    let mut way_fires: BTreeMap<String, u64> = BTreeMap::new();
    let mut gate = GateSummary::default();
    let mut gate_ms: Vec<u64> = Vec::new();
    let mut suppressed = SuppressedSummary::default();

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
            Some("way_judged") => {
                gate.judged += 1;
                match v["verdict"].as_str() {
                    Some("block") => {
                        gate.blocked += 1;
                        if let Some(w) = v["way"].as_str() {
                            *gate.blocked_ways.entry(w.to_string()).or_insert(0) += 1;
                        }
                    }
                    Some("would_block") => gate.would_block += 1,
                    _ => gate.passed += 1,
                }
                if let Some(ms) = v["gate_ms"].as_str().and_then(|s| s.parse().ok()) {
                    gate_ms.push(ms);
                }
            }
            Some("injection_suppressed") => {
                suppressed.total += 1;
                if v["lane"].as_str() == Some("task") {
                    suppressed.dispatches += 1;
                } else {
                    suppressed.agents += 1;
                }
                *suppressed.by_switch.entry(v["switch"].as_str().unwrap_or("unknown").to_string()).or_insert(0) += 1;
            }
            Some("gate_capped") => {
                gate.unjudged += v["unjudged"].as_str().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            }
            Some("gate_fallback") => {
                let reason = v["reason"].as_str().unwrap_or("unknown");
                let key = reason.split(':').next().unwrap_or(reason).to_string();
                *gate.fallbacks.entry(key).or_insert(0) += 1;
                if let Some(ms) = v["gate_ms"].as_str().and_then(|s| s.parse().ok()) {
                    gate_ms.push(ms);
                }
            }
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
        gate: {
            gate_ms.sort_unstable();
            let pct = |q: f64| (!gate_ms.is_empty()).then(|| gate_ms[((gate_ms.len() - 1) as f64 * q).round() as usize]);
            GateSummary { gate_ms_p50: pct(0.5), gate_ms_p95: pct(0.95), ..gate }
        },
        suppressed,
    }
}

fn build_near_misses(content: &str, session_id: &str, frames: &[Frame]) -> Vec<NearMiss> {
    session_events(content, session_id)
        .filter(|v| v["event"].as_str() == Some("way_nearmiss"))
        .map(|v| {
            let ts = v["ts"].as_str().unwrap_or("").to_string();
            NearMiss {
                epoch: epoch_for_ts(frames, &ts),
                timestamp: ts,
                way: v["way"].as_str().unwrap_or("").to_string(),
                prob_en: field_f64(&v, "prob_en"),
                prob_multi: field_f64(&v, "prob_multi"),
                tau_s: field_f64(&v, "tau_s"),
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

/// The session with the most recent event of *any* kind — "what's live right now".
/// Unlike [`most_recent_session`], this keys off the latest activity, not the latest
/// `session_start`, and ignores the recorded project: a long-running session's
/// `session_start` is old, and its project may be mis-recorded (the boundary hook
/// logs `CLAUDE_PROJECT_DIR:-$PWD`, which isn't always the real project). For a live
/// monitor, the currently-active session is the one still appending events.
pub(crate) fn most_recent_active_session(content: &str) -> Option<String> {
    let mut best: Option<(String, String)> = None; // (ts, session)
    for line in content.lines() {
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let sid = match v["session"].as_str() {
            Some(s) if !s.is_empty() => s,
            _ => continue,
        };
        let ts = v["ts"].as_str().unwrap_or("");
        if best.as_ref().map(|(b, _)| ts > b.as_str()).unwrap_or(true) {
            best = Some((ts.to_string(), sid.to_string()));
        }
    }
    best.map(|(_, s)| s)
}

/// Latest session (by session_start timestamp) within an optional project scope.
pub(crate) fn most_recent_session(content: &str, scope: Option<&str>) -> Option<String> {
    // The newest session at the project itself wins; one in a worktree under
    // it is the default only when the project has none, so a workflow's
    // session never stands in for the operator's own.
    let mut best: Option<(String, String)> = None; // (ts, session)
    let mut under: Option<(String, String)> = None;
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
        let project = v["project"].as_str().unwrap_or("");
        let slot = match scope {
            Some(sc) if !scope::project_matches(project, sc) => continue,
            Some(sc) if !ways_core::util::in_project(sc, project) => &mut under,
            _ => &mut best,
        };
        let ts = v["ts"].as_str().unwrap_or("").to_string();
        if slot.as_ref().map(|(b, _)| ts > *b).unwrap_or(true) {
            *slot = Some((ts, sid.to_string()));
        }
    }
    best.or(under).map(|(_, s)| s)
}

/// Map a near-miss timestamp to the epoch of the frame it falls within —
/// the last frame whose timestamp is at or before it.
fn epoch_for_ts(frames: &[Frame], ts: &str) -> u64 {
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

#[cfg(test)]
mod tests {
    use super::build_dump;

    const LOG: &str = concat!(
        r#"{"event":"way_judged","session":"dump-test","ts":"2026-01-01T00:00:00Z","way":"d/a","p_yes":"0.100","threshold":"0.30","verdict":"would_block"}"#, "\n",
        r#"{"event":"way_fired","session":"dump-test","ts":"2026-01-01T00:00:00Z","way":"d/a","trigger":"keyword"}"#, "\n",
        r#"{"event":"way_judged","session":"dump-test","ts":"2026-01-01T00:00:00Z","way":"d/b","p_yes":"0.050","threshold":"0.30","verdict":"block"}"#, "\n",
        r#"{"event":"way_judged","session":"dump-test","ts":"2026-01-01T00:00:00Z","way":"d/b/c","p_yes":"0.050","threshold":"0.30","verdict":"block","reason":"ancestor","ancestor":"d/b"}"#, "\n",
    );

    /// The rows of the first frame as (id, outcome, p_yes, ancestor).
    fn rows(matched: bool) -> Vec<(String, String, String, String)> {
        let dump = serde_json::to_value(build_dump(LOG, "dump-test", matched).unwrap()).unwrap();
        let s = |v: &serde_json::Value| v.as_str().unwrap_or("").to_string();
        dump["frames"][0]["active_ways"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| (s(&w["id"]), s(&w["outcome"]), s(&w["p_yes"]), s(&w["ancestor"])))
            .collect()
    }

    /// `replay --json` marks each way with its outcome and P(yes);
    /// `--matched` adds the ways the judge blocked.
    #[test]
    fn replay_json_carries_outcomes_and_matched_adds_the_blocked() {
        let row = |id: &str, o: &str, p: &str, a: &str| (id.to_string(), o.to_string(), p.to_string(), a.to_string());
        assert_eq!(rows(false), [row("d/a", "would_block", "0.100", "")]);
        assert_eq!(
            rows(true),
            [row("d/a", "would_block", "0.100", ""), row("d/b", "blocked", "0.050", ""), row("d/b/c", "blocked", "0.050", "d/b")]
        );
    }
}
