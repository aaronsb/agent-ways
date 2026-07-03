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
