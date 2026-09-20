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
    query: Option<&str>,
) -> Result<()> {
    // The same transcript the context-threshold arm reads also carries the
    // model id every fire on this lane is stamped with.
    crate::cmd::show::set_firing_transcript(transcript);
    // A UserPromptSubmit that carries a harness envelope rather than an
    // operator turn does not advance the session's guidance. Measured over a
    // month of transcripts, 154 of 470 Prose Check fires landed on Monitor
    // and task notifications where no human was reading the reply. Only the
    // context-threshold arm is gated: the core safety net and session-start
    // ways (the teams way reaches a teammate on its first prompt, which may
    // be harness-wrapped) still run.
    let envelope_turn = is_envelope_turn(hook_event, query);

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
                !envelope_turn && evaluate_context_threshold(way.threshold as u64, transcript)
            }
            "file-exists" => {
                if let Some(ref pattern) = way.trigger_path {
                    evaluate_file_exists(pattern, &project_dir)
                } else {
                    false
                }
            }
            // Once per session: the state hook also runs on every prompt for
            // the two conditional triggers above, and a session-start way
            // must not ride that cadence on its refire curve. The marker is
            // cleared on startup, compact, and clear, so the way still shows
            // again after a compaction. Gated here on the marker rather than
            // on the SessionStart event because the teammate scope marker is
            // written after session start, and the teams way must still
            // reach a teammate on its first prompt.
            "session-start" => !session::way_is_shown(&way.id, session_id),
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

/// True when the invoking prompt is a harness envelope on a UserPromptSubmit,
/// so context-threshold ways should not ride it.
fn is_envelope_turn(hook_event: &str, query: Option<&str>) -> bool {
    hook_event == "UserPromptSubmit" && query.is_some_and(super::is_system_envelope)
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
    // Use glob matching for patterns like "*.md" or "docs/architecture/*.md"
    let full_pattern = format!("{project_dir}/{pattern}");
    glob::glob(&full_pattern)
        .map(|paths| paths.filter_map(|p| p.ok()).next().is_some())
        .unwrap_or(false)
}

fn capture_show_core(session_id: &str) -> String {
    crate::cmd::show::core(session_id).unwrap_or_default()
}

#[cfg(test)]
mod envelope_gate_tests {
    use super::is_envelope_turn;

    #[test]
    fn notification_turns_do_not_carry_threshold_ways() {
        assert!(is_envelope_turn("UserPromptSubmit", Some("<task-notification> <task-id>x</task-id>")));
        assert!(is_envelope_turn("UserPromptSubmit", Some("  [attend] 2 peer message(s)")));
        assert!(is_envelope_turn("UserPromptSubmit", Some("base directory for this skill: /x")));
    }

    #[test]
    fn operator_turns_and_other_events_are_untouched() {
        assert!(!is_envelope_turn("UserPromptSubmit", Some("let's wrap up")));
        assert!(!is_envelope_turn("UserPromptSubmit", None));
        assert!(!is_envelope_turn("SessionStart", Some("<task-notification>")));
    }
}
