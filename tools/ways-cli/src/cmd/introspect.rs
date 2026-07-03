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
