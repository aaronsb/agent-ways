//! `ways introspect` — the user/agent-facing surface over the ways-core
//! `SessionIntrospection` model (ADR-154). Modes: `replay` (interactive),
//! `list`, `dump` (agent-facing JSON). `live` and the why-fired drill-down
//! follow in later increments.
//!
//! `replay`/`list` currently delegate to the proven `rethink` pipeline
//! (`build_frames` → TUI); re-pointing them at the `SessionIntrospection` model
//! is deferred until the drill-down needs it, since it requires reconciling that
//! model's fire-centric clustering with `build_frames`' full-event-stream
//! clustering (ADR-154 §1 — not a drop-in swap). `dump` already reads the model.

use anyhow::Result;

use crate::cmd::{rethink, rethink_dump};
use crate::session;

/// `ways introspect replay` — interactive replay of a session's way firings.
pub fn replay(
    session: Option<&str>,
    project: Option<&str>,
    all: bool,
    speed: Option<u64>,
) -> Result<()> {
    rethink::run(session, project, speed, false, all)
}

/// `ways introspect live` — monitor the current session's way firings, following
/// the newest frame as ways fire. The "current" session is the most recent one in
/// scope (the one actively writing events); `--session` overrides it. Scoping
/// mirrors `replay`: defaults to the current project, `--project` for a specific
/// one, and fails loud rather than silently globalizing when detection fails.
pub fn live(session: Option<&str>, project: Option<&str>) -> Result<()> {
    let content = ways_core::firing::load_events_text();
    if content.trim().is_empty() {
        println!("No events recorded yet.");
        return Ok(());
    }

    // Resolve which session to monitor:
    // - explicit `--session` wins;
    // - `--project` scopes to the latest session recorded under that project;
    // - otherwise the *most recently active* session anywhere — the one still
    //   appending events. We deliberately don't scope the default to a detected
    //   project: a long-running session's recorded project can be stale/wrong (the
    //   boundary hook logs `CLAUDE_PROJECT_DIR:-$PWD`), and "live" means "what's
    //   happening now," which is an activity signal, not a project one.
    let session_id = match (session, project) {
        (Some(s), _) => s.to_string(),
        (None, Some(p)) => match rethink_dump::most_recent_session(&content, Some(p)) {
            Some(s) => s,
            None => {
                println!("No sessions found for project {p}.");
                return Ok(());
            }
        },
        (None, None) => match rethink_dump::most_recent_active_session(&content) {
            Some(s) => s,
            None => {
                println!("No sessions found to monitor.");
                return Ok(());
            }
        },
    };

    // The project shown (and used for the transcript lookup) is where you launched
    // the monitor — CLAUDE_PROJECT_DIR, or the current directory — NOT the session's
    // recorded project, which the boundary hook may have mislabeled. For a live view,
    // "the project" is where you're working now.
    let launch_project = project.map(str::to_string).or_else(|| {
        std::env::var("CLAUDE_PROJECT_DIR").ok().or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
        })
    });

    rethink::run_live(&session_id, launch_project.as_deref(), None)
}

/// `ways introspect list` — enumerate candidate sessions in scope, as a table or
/// (`--json`) machine-listable data for an agent to pick from before dumping.
pub fn list(project: Option<&str>, all: bool, json: bool) -> Result<()> {
    if json {
        rethink_dump::run_list_json(project, all)
    } else {
        rethink::run(None, project, None, true, all)
    }
}

/// `ways introspect dump` — emit a session's reconstructed introspection (turns,
/// fired ways, criteria, keyed transcript join, matched spans) as JSON, so an
/// agent can investigate *which ways fired, on which turn, and why* without a TUI.
///
/// Scoping mirrors `rethink`: default the current project, `--project` for a
/// specific one, `--all` across every project (which only affects session
/// picking). With no `--session`, the most recent session in scope is dumped.
pub fn dump(session: Option<&str>, project: Option<&str>, all: bool) -> Result<()> {
    let content = ways_core::firing::load_events_text();
    if content.trim().is_empty() {
        println!("{{\"error\":\"no events recorded yet\"}}");
        return Ok(());
    }

    // Fail-loud scope resolution, as JSON (agent-facing).
    let scope = match rethink::resolve_project_scope(project, all) {
        Ok(s) => s,
        Err(e) => {
            println!("{{\"error\":{}}}", serde_json::to_string(&e.to_string())?);
            return Ok(());
        }
    };

    let session_id = match session {
        Some(s) => s.to_string(),
        None => match rethink_dump::most_recent_session(&content, scope.as_deref()) {
            Some(s) => s,
            None => {
                println!("{{\"error\":\"no sessions found in scope\"}}");
                return Ok(());
            }
        }
    };

    // Project path drives the criteria corpus and the transcript slug. Prefer the
    // caller's scope (explicit `--project` or the detected current project) — it's
    // authoritative — over the session's first recorded `session_start` project,
    // which can be a stray subagent/hook cwd. The transcript reader scans by
    // session id if the slug misses, so a wrong path here still resolves the join.
    let project_path = scope
        .clone()
        .or_else(|| rethink::find_session_project(&content, &session_id))
        .unwrap_or_default();
    let window_k = session::detect_context_window_for(&project_path, &session_id) / 1000;

    let model = ways_core::introspection::SessionIntrospection::from_session(
        &session_id,
        &project_path,
        window_k,
    );
    println!("{}", serde_json::to_string_pretty(&model)?);
    Ok(())
}

/// `ways introspect fires` — the read-side precision instrument (task #2 of the
/// ADR-160 calibration work). Reads `way_fired`/`way_redisclosed` events straight
/// from events.jsonl — no `SessionIntrospection` reconstruction, no re-embedding —
/// and prints each *semantic* fire as `score · surface · way`, borderline (lowest
/// score) first, so a human can judge whether the matcher fired on text that
/// actually warranted the way. Pair with `--max-score` to isolate the suspect tail
/// that gate calibration (task #5) has to defend.
pub fn fires(
    session: Option<&str>,
    project: Option<&str>,
    all: bool,
    max_score: Option<f64>,
    limit: Option<usize>,
) -> Result<()> {
    let content = ways_core::firing::load_events_text();
    if content.trim().is_empty() {
        println!("No events recorded yet.");
        return Ok(());
    }

    let scope = rethink::resolve_project_scope(project, all)?;
    let session_id = match session {
        Some(s) => s.to_string(),
        None => match rethink_dump::most_recent_session(&content, scope.as_deref()) {
            Some(s) => s,
            None => {
                println!("No sessions found in scope.");
                return Ok(());
            }
        }
    };

    // Pull semantic fires for this session. A fire is semantic when its trigger
    // begins `semantic:` (`semantic:embedding:en|multi`); keyword/state fires have
    // no score or surface to eyeball, so they are out of scope for this view.
    let mut rows: Vec<(f64, String, String, bool)> = Vec::new();
    for line in content.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        if v.get("session").and_then(|s| s.as_str()) != Some(session_id.as_str()) {
            continue;
        }
        let event = v.get("event").and_then(|e| e.as_str()).unwrap_or("");
        let redisclosed = match event {
            "way_fired" => false,
            "way_redisclosed" => true,
            _ => continue,
        };
        let trigger = v.get("trigger").and_then(|t| t.as_str()).unwrap_or("");
        if !trigger.starts_with("semantic:") {
            continue;
        }
        // `fire_score` is written as a formatted string field (see show::way_scored).
        let Some(score) = v.get("fire_score").and_then(|s| s.as_str()).and_then(|s| s.parse::<f64>().ok())
        else {
            continue;
        };
        if let Some(cap) = max_score {
            if score > cap {
                continue;
            }
        }
        let way = v.get("way").and_then(|w| w.as_str()).unwrap_or("?").to_string();
        // `surface` only rides fires logged after the read-side instrument shipped;
        // older events legitimately lack it — show a placeholder rather than drop them.
        let surface = v
            .get("surface")
            .and_then(|s| s.as_str())
            .unwrap_or("—")
            .to_string();
        rows.push((score, way, surface, redisclosed));
    }

    if rows.is_empty() {
        println!(
            "No semantic fires for session {} (keyword/state fires carry no score/surface).",
            &session_id[..session_id.len().min(12)]
        );
        return Ok(());
    }

    // Borderline first: the lowest-scoring fires are the ones whose relevance is
    // most in question, so they lead the readout.
    rows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let total = rows.len();
    let shown = limit.unwrap_or(total).min(total);

    println!(
        "{} semantic fire{} · session {} · lowest score first{}",
        total,
        if total == 1 { "" } else { "s" },
        &session_id[..session_id.len().min(12)],
        max_score.map(|c| format!(" · ≤ {c:.2}")).unwrap_or_default(),
    );
    for (score, way, surface, redisclosed) in rows.into_iter().take(shown) {
        let mark = if redisclosed { "↻" } else { " " };
        println!("  {score:.3} {mark} {way:<44}  {surface}");
    }
    if shown < total {
        println!("  … {} more (raise --limit)", total - shown);
    }
    Ok(())
}
