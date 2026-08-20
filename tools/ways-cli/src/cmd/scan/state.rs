//! State-trigger scan — evaluates `context-threshold`, `file-exists`,
//! and `session-start` triggers and emits matched bodies for the agent.
//!
//! Split from `mod.rs` so the scan module stays under the Code Quality
//! Way's Review-tier line budget. Behavior unchanged from the in-place
//! implementation.

use anyhow::Result;

use crate::session;

use super::candidates::collect_candidates;
use super::emit_hook_context;
use super::scoring::{capture_show_way, default_project};

pub fn state(
    session_id: &str,
    project: Option<&str>,
    transcript: Option<&str>,
    hook_event: &str,
) -> Result<()> {
    let project_dir = project
        .map(|s| s.to_string())
        .unwrap_or_else(default_project);

    let scope = session::detect_scope(session_id);
    let candidates = collect_candidates(&project_dir);

    let mut context = String::new();

    // Core re-injection safety net. The marker is cleared by `clear-markers.sh`
    // on the `startup`, `compact`, and `clear` SessionStart matchers, so a
    // missing marker is the one signal that core needs showing. (An earlier
    // transcript-size heuristic — "marker older than 30 s and under 5 KB of
    // transcript since the last summary" — re-showed core on the first prompt
    // of any session whose operator paused before typing, because a fresh
    // transcript is tiny. Removed.)
    if !session::core_is_shown(session_id) {
        let out = capture_show_core(session_id);
        if !out.is_empty() {
            context.push_str(&out);
            context.push_str("\n\n");
        }
    }

    // State trigger evaluation
    for way in &candidates {
        let trigger_type = match &way.trigger {
            Some(t) => t.as_str(),
            None => continue,
        };

        if !session::scope_matches(&way.scope, &scope) {
            continue;
        }

        let triggered = match trigger_type {
            "context-threshold" => {
                evaluate_context_threshold(way.threshold as u64, transcript)
            }
            "file-exists" => {
                if let Some(ref pattern) = way.trigger_path {
                    evaluate_file_exists(pattern, &project_dir)
                } else {
                    false
                }
            }
            "session-start" => true,
            _ => false,
        };

        if !triggered {
            continue;
        }

        // Marker-gated via show; refire cadence follows the way's own `refire:`
        // curve (ADR-126). The former `repeat: true` bypass — which dumped the
        // body every threshold crossing and consulted a `tasks-active` marker —
        // is gone: no way uses `repeat` since todos moved to its refire curve, so
        // the branch was dead. (The mark-tasks-active hook still writes the marker;
        // it is dormant, kept as the hook-point should per-way tasks-active
        // suppression be wanted again.)
        let out = capture_show_way(&way.id, session_id, "state", None, None, None);
        if !out.is_empty() {
            context.push_str(&out);
            context.push_str("\n\n");
        }
    }

    if !context.is_empty() {
        emit_hook_context(hook_event, context.trim_end());
    }

    Ok(())
}

fn evaluate_context_threshold(threshold_pct: u64, transcript: Option<&str>) -> bool {
    // Guard: a missing or 0 threshold on a context-threshold trigger is a bug
    // (would fire on every non-empty transcript). Caller should have set a
    // percentage in frontmatter. Refuse to fire rather than spam.
    if threshold_pct == 0 {
        return false;
    }

    let transcript = match transcript {
        Some(t) if std::path::Path::new(t).is_file() => t,
        _ => return false,
    };

    // Single source of truth with `ways context`: accurate API token counts ÷
    // model window — NOT a transcript-byte heuristic, which over-counts the
    // full transcript file (out-of-context tool output, persisted blobs, JSON
    // envelope) and fires thresholds far too early.
    matches!(
        crate::cmd::context::pct_used_from_transcript(transcript),
        Some(pct) if pct >= threshold_pct
    )
}

fn evaluate_file_exists(pattern: &str, project_dir: &str) -> bool {
    // Use glob matching for patterns like "*.md" or ".claude/todo-*.md"
    let full_pattern = format!("{project_dir}/{pattern}");
    glob::glob(&full_pattern)
        .map(|paths| paths.filter_map(|p| p.ok()).next().is_some())
        .unwrap_or(false)
}

fn capture_show_core(session_id: &str) -> String {
    crate::cmd::show::core(session_id).unwrap_or_default()
}
