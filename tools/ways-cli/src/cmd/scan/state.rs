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

    // Core re-injection safety net
    if !session::core_is_shown(session_id) {
        let out = capture_show_core(session_id);
        if !out.is_empty() {
            context.push_str(&out);
            context.push_str("\n\n");
        }
    } else if let Some(tp) = transcript {
        // Check for stale core (context cleared under us)
        let ctx_size = transcript_size_since_summary(tp);
        if let Some(marker_ts) = session::core_marker_ts(session_id) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let age = now.saturating_sub(marker_ts);
            if ctx_size < 5000 && age > 30 {
                session::clear_core(session_id);
                let out = capture_show_core(session_id);
                if !out.is_empty() {
                    context.push_str(&out);
                    context.push_str("\n\n");
                }
            }
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

        // Handle repeating context-threshold ways
        if trigger_type == "context-threshold" && way.repeat {
            let tasks_marker = format!("{}/{session_id}/tasks-active", crate::session::sessions_root());
            if std::path::Path::new(&tasks_marker).exists() {
                continue; // tasks active, suppress repeat
            }
            // Repeating: output body directly (no marker gating)
            let content = std::fs::read_to_string(&way.path).unwrap_or_default();
            let body = strip_frontmatter(&content);
            if !body.is_empty() {
                context.push_str(&body);
                context.push_str("\n\n");
                session::log_event(&[
                    ("event", "way_fired"),
                    ("way", &way.id),
                    ("domain", way.id.split('/').next().unwrap_or(&way.id)),
                    ("trigger", "state"),
                    ("scope", &scope),
                    ("project", &project_dir),
                    ("session", session_id),
                ]);
            }
        } else {
            // Standard one-shot: marker-gated via show
            let out = capture_show_way(&way.id, session_id, "state");
            if !out.is_empty() {
                context.push_str(&out);
                context.push_str("\n\n");
            }
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

    // Detect model for context window size
    let window_chars: u64 = detect_window_chars(transcript);
    let limit = window_chars * threshold_pct / 100;
    let size = transcript_size_since_summary(transcript);

    size > limit
}

fn detect_window_chars(transcript: &str) -> u64 {
    let content = match std::fs::read_to_string(transcript) {
        Ok(c) => c,
        Err(_) => return 620_000,
    };
    for line in content.lines().rev() {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
            if val.get("type").and_then(|t| t.as_str()) == Some("assistant") {
                if let Some(model) = val.get("message").and_then(|m| m.get("model")).and_then(|m| m.as_str()) {
                    if model.contains("opus-4") {
                        return 3_800_000;
                    }
                }
                break;
            }
        }
    }
    620_000 // default: ~155K tokens × 4 chars/token
}

fn transcript_size_since_summary(transcript: &str) -> u64 {
    let file_size = match std::fs::metadata(transcript) {
        Ok(m) => m.len(),
        Err(_) => return 0,
    };

    // Check last 100KB for summary markers
    let content = match std::fs::read_to_string(transcript) {
        Ok(c) => c,
        Err(_) => return file_size,
    };

    // Find last summary line position
    let mut last_summary_pos = 0u64;
    let mut pos = 0u64;
    for line in content.lines() {
        if line.contains("\"type\":\"summary\"") {
            last_summary_pos = pos + line.len() as u64 + 1;
        }
        pos += line.len() as u64 + 1;
    }

    file_size.saturating_sub(last_summary_pos)
}

fn evaluate_file_exists(pattern: &str, project_dir: &str) -> bool {
    // Use glob matching for patterns like "*.md" or ".claude/todo-*.md"
    let full_pattern = format!("{project_dir}/{pattern}");
    glob::glob(&full_pattern)
        .map(|paths| paths.filter_map(|p| p.ok()).next().is_some())
        .unwrap_or(false)
}

fn strip_frontmatter(content: &str) -> String {
    let mut fm_count = 0;
    let mut body = Vec::new();
    for line in content.lines() {
        if line == "---" {
            fm_count += 1;
            continue;
        }
        if fm_count >= 2 {
            body.push(line);
        }
    }
    body.join("\n")
}

fn capture_show_core(session_id: &str) -> String {
    crate::cmd::show::core(session_id).unwrap_or_default()
}
