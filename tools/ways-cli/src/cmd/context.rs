//! Context window usage — accurate token counts from transcript API data.
//!
//! Replaces: scripts/context-usage.sh
//! Reads the active transcript's API usage data for real token counts,
//! detects the model and context window size, provides JSON and human output.

use anyhow::Result;
use serde_json::json;
use std::path::{Path, PathBuf};

pub struct ContextInfo {
    pub tokens_used: u64,
    pub tokens_total: u64,
    pub tokens_remaining: u64,
    pub pct_used: u64,
    pub pct_remaining: u64,
    pub model: String,
    pub method: String,
    pub session: String,
}

/// Get context info for the current session. Used by `ways context` and `ways list`.
///
/// When `session_id` is provided, the transcript is located by scanning
/// `~/.claude/projects/*/<session_id>.jsonl` — this is robust against
/// cwd/project mismatches (e.g. a session rooted in `~/.claude` while the
/// shell cwd is elsewhere). Falls back to `project_dir` + newest-transcript
/// lookup when no session id is given.
pub fn get_context(project_dir: Option<&str>) -> Result<ContextInfo> {
    get_context_inner(project_dir, None)
}

/// Like `get_context`, but pinned to a known session id. Locates the
/// transcript by session id across all project dirs rather than guessing
/// the project from cwd.
pub fn get_context_for_session(session_id: &str) -> Result<ContextInfo> {
    get_context_inner(None, Some(session_id))
}

fn get_context_inner(project_dir: Option<&str>, session_id: Option<&str>) -> Result<ContextInfo> {
    let transcript = if let Some(sid) = session_id {
        find_transcript_by_session(sid).ok_or_else(|| {
            anyhow::anyhow!("No transcript found for session: {sid}")
        })?
    } else {
        let project = project_dir
            .map(|s| s.to_string())
            .or_else(|| std::env::var("CLAUDE_PROJECT_DIR").ok())
            .or_else(detect_project_dir)
            .unwrap_or_else(|| ".".to_string());

        let project_slug = project.replace(['/', '.'], "-");
        let conv_dir = home_dir().join(format!(".claude/projects/{project_slug}"));

        find_newest_transcript(&conv_dir)
            .ok_or_else(|| anyhow::anyhow!("No active transcript found for project: {project}"))?
    };

    let session = transcript
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

    let content = std::fs::read_to_string(&transcript)?;

    // Detect model from last assistant message
    let model = detect_model(&content);
    let window_tokens = model_to_window(&model);

    // Get token count from API usage data
    let (tokens_used, method) = read_token_usage(&content);

    let tokens_remaining = window_tokens.saturating_sub(tokens_used);
    let pct_used = if window_tokens > 0 {
        tokens_used * 100 / window_tokens
    } else {
        0
    };
    let pct_remaining = 100u64.saturating_sub(pct_used);

    Ok(ContextInfo {
        tokens_used,
        tokens_total: window_tokens,
        tokens_remaining,
        pct_used,
        pct_remaining,
        model,
        method,
        session,
    })
}

pub fn run(project: Option<&str>, json_out: bool) -> Result<()> {
    let ctx = get_context(project)?;

    if json_out {
        let output = json!({
            "tokens_used": ctx.tokens_used,
            "tokens_remaining": ctx.tokens_remaining,
            "tokens_total": ctx.tokens_total,
            "pct_used": ctx.pct_used,
            "pct_remaining": ctx.pct_remaining,
            "model": ctx.model,
            "method": ctx.method,
            "session": ctx.session,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    let used_k = ctx.tokens_used / 1000;
    let total_k = ctx.tokens_total / 1000;
    let remaining_k = ctx.tokens_remaining / 1000;

    println!();

    // Token bar
    let bar_width = 60;
    let filled = (ctx.pct_used as usize * bar_width / 100).min(bar_width);

    let bar_color = if ctx.pct_used < 50 {
        "\x1b[0;32m" // green
    } else if ctx.pct_used < 75 {
        "\x1b[1;33m" // yellow
    } else {
        "\x1b[0;31m" // red
    };

    // Token-usage bar. The old "25% re-disclosure marker" was dropped
    // when ADR-123 moved firing dynamics onto per-way curves — no
    // single tick on a global context bar captures per-way behavior.
    // Use `ways list` to see per-way re-fire points.
    let mut bar = String::new();
    for i in 0..bar_width {
        if i < filled {
            bar.push('█');
        } else {
            bar.push('░');
        }
    }

    println!(
        "  {bar_color}{bar}\x1b[0m {}%",
        ctx.pct_used
    );
    println!();
    println!(
        "  \x1b[1m{used_k}K\x1b[0m / {total_k}K tokens used  \x1b[2m({remaining_k}K remaining)\x1b[0m"
    );
    println!(
        "  \x1b[2mModel: {}  Method: {}\x1b[0m",
        ctx.model, ctx.method
    );
    println!();

    Ok(())
}

// ── Internals ──────────────────────────────────────────────────

fn detect_model(content: &str) -> String {
    // Scan from the end for the most recent assistant message with a model field
    for line in content.lines().rev() {
        if !line.contains("\"assistant\"") {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
            if val.get("type").and_then(|t| t.as_str()) == Some("assistant") {
                if let Some(model) = val
                    .get("message")
                    .and_then(|m| m.get("model"))
                    .and_then(|m| m.as_str())
                {
                    return model.to_string();
                }
            }
        }
    }
    "unknown".to_string()
}

fn model_to_window(model: &str) -> u64 {
    if model.contains("opus-4") {
        1_000_000
    } else if model.contains("sonnet") || model.contains("haiku") {
        200_000
    } else {
        // Check env override
        if let Ok(val) = std::env::var("CLAUDE_CONTEXT_WINDOW") {
            if let Ok(n) = val.parse::<u64>() {
                return n;
            }
        }
        200_000 // safe default
    }
}

fn read_token_usage(content: &str) -> (u64, String) {
    // Find the highest token count from assistant messages with usage data
    // cache_read reflects actual context size sent to API
    let mut max_tokens: u64 = 0;

    for line in content.lines().rev() {
        if !line.contains("cache_read_input_tokens") {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
            if val.get("type").and_then(|t| t.as_str()) == Some("assistant") {
                if let Some(usage) = val.get("message").and_then(|m| m.get("usage")) {
                    let cache_read = usage["cache_read_input_tokens"].as_u64().unwrap_or(0);
                    let cache_create = usage["cache_creation_input_tokens"].as_u64().unwrap_or(0);
                    let input = usage["input_tokens"].as_u64().unwrap_or(0);
                    let total = cache_read + cache_create + input;
                    if total > max_tokens {
                        max_tokens = total;
                        // Most recent is most accurate — don't keep scanning
                        return (max_tokens, "api".to_string());
                    }
                }
            }
        }
    }

    if max_tokens > 0 {
        return (max_tokens, "api".to_string());
    }

    // Fallback: estimate from transcript bytes
    let file_size = content.len() as u64;

    // Find last summary position
    let mut last_summary_end: u64 = 0;
    let mut pos: u64 = 0;
    for line in content.lines() {
        if line.contains("\"type\":\"summary\"") {
            last_summary_end = pos + line.len() as u64 + 1;
        }
        pos += line.len() as u64 + 1;
    }

    let active_bytes = file_size.saturating_sub(last_summary_end);
    // Conservative: ~6.3 transcript JSON bytes per token
    let estimated = active_bytes * 10 / 63;
    (estimated, "bytes".to_string())
}

/// Find a transcript by session id, searching every project dir under
/// `~/.claude/projects/`. Session ids are globally unique, so we don't
/// need to know which project the session is rooted in.
fn find_transcript_by_session(session_id: &str) -> Option<PathBuf> {
    let projects_root = home_dir().join(".claude/projects");
    let filename = format!("{session_id}.jsonl");
    for entry in std::fs::read_dir(&projects_root).ok()? {
        let entry = entry.ok()?;
        let candidate = entry.path().join(&filename);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn find_newest_transcript(dir: &Path) -> Option<PathBuf> {
    if !dir.is_dir() {
        return None;
    }
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        if path.to_str().is_some_and(|s| s.contains(".tmp")) {
            continue;
        }
        if let Ok(meta) = entry.metadata() {
            let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
            if newest.as_ref().is_none_or(|(t, _)| mtime > *t) {
                newest = Some((mtime, path));
            }
        }
    }
    newest.map(|(_, p)| p)
}

use crate::util::{detect_project_dir, home_dir};
