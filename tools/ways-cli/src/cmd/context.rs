//! Context window usage — accurate token counts from transcript API data.
//!
//! Replaces: scripts/context-usage.sh
//! Reads the active transcript's API usage data for real token counts,
//! detects the model and context window size, provides JSON and human output.

use anyhow::Result;
use serde_json::json;
use std::path::{Path, PathBuf};
use ways_core::context_window::{self, WindowSource};

pub struct ContextInfo {
    pub tokens_used: u64,
    pub tokens_total: u64,
    pub tokens_remaining: u64,
    pub pct_used: u64,
    pub pct_remaining: u64,
    pub model: String,
    pub method: String,
    pub session: String,
    /// How `tokens_total` was arrived at (ADR-166). Carried so a defaulted window
    /// is never mistaken for a detected one — the failure that let a 1M Fable
    /// session report 106% of a 200k window.
    pub window_source: WindowSource,
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

/// Accurate context-fill percentage (0–100) from a transcript file path.
///
/// Single source of truth shared with the `context-threshold` trigger in
/// `scan/state.rs`: both read the same gauge — real API token counts
/// (`read_token_usage`) divided by the model window (`context_window::resolve`,
/// ADR-166) — never a transcript-byte heuristic. The transcript *file* is far
/// larger than the live context (it holds full tool output, persisted-output
/// blobs that aren't in context, and JSON envelope overhead), so byte-size badly
/// over-counts and fires thresholds early.
pub fn pct_used_from_transcript(transcript: &str) -> Option<u64> {
    let content = std::fs::read_to_string(transcript).ok()?;
    let window = resolve_window(&content).tokens;
    if window == 0 {
        return None;
    }
    let (tokens_used, _method) = read_token_usage(&content);
    Some(tokens_used * 100 / window)
}

fn get_context_inner(project_dir: Option<&str>, session_id: Option<&str>) -> Result<ContextInfo> {
    let projects_root = home_dir().join(".claude/projects");
    let env_session_id = std::env::var("CLAUDE_CODE_SESSION_ID").ok();
    let transcript = resolve_transcript(
        project_dir,
        session_id,
        env_session_id.as_deref(),
        &projects_root,
    )?;

    let session = transcript
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

    let content = std::fs::read_to_string(&transcript)?;

    // Detect model from last assistant message, and resolve its window (ADR-166).
    let model = detect_model(&content);
    let window = resolve_window(&content);
    let window_tokens = window.tokens;

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
        window_source: window.source,
    })
}

/// Resolve which transcript file to read from the caller's inputs and the
/// ambient session environment. Kept pure w.r.t. globals — the env session id
/// and projects root are passed in — so the precedence below is unit-testable.
///
/// Precedence:
///   1. an explicit `session_id` (e.g. `get_context_for_session`);
///   2. otherwise, when no explicit `project_dir` was given, the *current*
///      session id from the environment (`CLAUDE_CODE_SESSION_ID`). This is
///      cwd-independent: it is what lets `ways context` report the live session
///      even when the shell cwd has drifted from the project — the failure the
///      `wrap` / `context-status` skills hit when they run the gauge from
///      wherever the agent's shell happens to sit;
///   3. finally, the `project_dir` / `CLAUDE_PROJECT_DIR` / cwd slug plus the
///      newest transcript in that project (the original heuristic, preserved).
fn resolve_transcript(
    project_dir: Option<&str>,
    session_id: Option<&str>,
    env_session_id: Option<&str>,
    projects_root: &Path,
) -> Result<PathBuf> {
    if let Some(sid) = session_id {
        return find_transcript_by_session_in(projects_root, sid)
            .ok_or_else(|| anyhow::anyhow!("No transcript found for session: {sid}"));
    }

    // No explicit --project: trust the environment's session id first. Only
    // fall through to the cwd heuristic if it is absent or its transcript is
    // missing, so behaviour outside a live session is unchanged.
    if project_dir.is_none() {
        if let Some(sid) = env_session_id.filter(|s| !s.is_empty()) {
            if let Some(transcript) = find_transcript_by_session_in(projects_root, sid) {
                return Ok(transcript);
            }
        }
    }

    let project = project_dir
        .map(|s| s.to_string())
        .or_else(|| std::env::var("CLAUDE_PROJECT_DIR").ok())
        .or_else(detect_project_dir)
        .unwrap_or_else(|| ".".to_string());

    let project_slug = project.replace(['/', '.'], "-");
    let conv_dir = projects_root.join(project_slug);

    find_newest_transcript(&conv_dir)
        .ok_or_else(|| anyhow::anyhow!("No active transcript found for project: {project}"))
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
            "window_source": ctx.window_source.as_str(),
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

    println!("  {bar_color}{bar}\x1b[0m {}%", ctx.pct_used);
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

/// The most recent *real* assistant model in the transcript.
///
/// Sentinel turns (`<synthetic>`, written for interrupts and API errors) are
/// skipped, not returned: an interrupt does not change which model the session is
/// running, and treating the sentinel as the model would resolve a live 1M session
/// to the 200K default. Nine transcripts in local history end on one.
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
                    .filter(|m| !context_window::is_sentinel(m))
                {
                    return model.to_string();
                }
            }
        }
    }
    UNKNOWN_MODEL.to_string()
}

/// Sentinel `detect_model` returns when the transcript holds no assistant turn
/// yet — the launch race. It is the *absence* of a model, not a model id.
const UNKNOWN_MODEL: &str = "unknown";

/// Resolve the window for a transcript's detected model through the one resolver
/// (ADR-166). The `"unknown"` sentinel is an absent model, not a model named
/// "unknown", so it is passed as `None`.
fn resolve_window(content: &str) -> context_window::ContextWindow {
    let model = detect_model(content);
    let known = (model != UNKNOWN_MODEL).then_some(model.as_str());
    context_window::resolve(known)
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

/// The root every session transcript lives under, one directory per project.
pub(crate) fn projects_root() -> PathBuf {
    home_dir().join(".claude/projects")
}

/// Find a transcript by session id, searching every project dir under
/// `~/.claude/projects/`. Session ids are globally unique, so we don't
/// need to know which project the session is rooted in.
pub(crate) fn find_transcript_by_session(session_id: &str) -> Option<PathBuf> {
    find_transcript_by_session_in(&projects_root(), session_id)
}

/// Search `projects_root/*/<session_id>.jsonl`. Split out from
/// `find_transcript_by_session` so the lookup is testable against a temp
/// projects root instead of the real `~/.claude/projects`.
pub(crate) fn find_transcript_by_session_in(
    projects_root: &Path,
    session_id: &str,
) -> Option<PathBuf> {
    let filename = format!("{session_id}.jsonl");
    for entry in std::fs::read_dir(projects_root).ok()? {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn pct_used_from_transcript_is_token_based_not_byte_based() {
        // A transcript whose FILE is tiny but whose API usage reports 500k
        // tokens on a 1M (opus) window must read as 50% — the regression guard
        // for the context-threshold byte-heuristic bug: the gauge is token
        // counts ÷ model window, never transcript file size.
        let path = std::env::temp_dir().join(format!("ways_pct_test_{}.jsonl", std::process::id()));
        let line = r#"{"type":"assistant","message":{"model":"claude-opus-4-8","usage":{"cache_read_input_tokens":500000,"cache_creation_input_tokens":0,"input_tokens":0}}}"#;
        {
            let mut f = std::fs::File::create(&path).unwrap();
            writeln!(f, "{line}").unwrap();
        }
        let pct = pct_used_from_transcript(path.to_str().unwrap());
        let _ = std::fs::remove_file(&path);
        assert_eq!(pct, Some(50));
    }

    /// Build a temp projects root with `<slug>/<sid>.jsonl` transcripts.
    /// Unique per call site via `line!()` so parallel tests don't collide;
    /// no env mutation, no `tempfile` dependency.
    fn temp_projects_root(unique: u32, transcripts: &[(&str, &str)]) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("ways_ctx_test_{}_{}", std::process::id(), unique));
        let _ = std::fs::remove_dir_all(&root);
        for (slug, sid) in transcripts {
            let dir = root.join(slug);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("{sid}.jsonl")), "{}\n").unwrap();
        }
        root
    }

    #[test]
    fn env_session_id_resolves_regardless_of_cwd() {
        // The bug: `ways context` from a drifted cwd found nothing. With the
        // session id from the environment, it locates the transcript by id in
        // any project dir — no --project, no matching cwd needed.
        let root = temp_projects_root(line!(), &[("-home-aaron-someproj", "sid-abc")]);
        let got = resolve_transcript(None, None, Some("sid-abc"), &root).unwrap();
        assert_eq!(got, root.join("-home-aaron-someproj/sid-abc.jsonl"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn explicit_session_id_takes_precedence_over_env() {
        let root = temp_projects_root(line!(), &[("-p", "explicit-sid"), ("-q", "env-sid")]);
        let got = resolve_transcript(None, Some("explicit-sid"), Some("env-sid"), &root).unwrap();
        assert_eq!(got, root.join("-p/explicit-sid.jsonl"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn explicit_project_ignores_env_session_id() {
        // A caller-supplied --project targets that project's newest transcript,
        // not whatever session the environment names.
        let root = temp_projects_root(
            line!(),
            &[
                ("-home-aaron-target", "proj-sid"),
                ("-elsewhere", "env-sid"),
            ],
        );
        let got =
            resolve_transcript(Some("/home/aaron/target"), None, Some("env-sid"), &root).unwrap();
        assert_eq!(got, root.join("-home-aaron-target/proj-sid.jsonl"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn find_transcript_by_session_in_returns_none_when_missing() {
        // Pins the not-found half of the id lookup the fall-through depends on.
        let root = temp_projects_root(line!(), &[("-someproj", "present-sid")]);
        assert!(find_transcript_by_session_in(&root, "absent-sid").is_none());
        assert!(find_transcript_by_session_in(&root, "present-sid").is_some());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn env_session_id_with_missing_transcript_falls_through() {
        // project_dir=None reaches the env branch; a non-empty env id whose
        // transcript doesn't exist must fall through to the project heuristic,
        // which errors here because the empty projects root has no match. (An
        // empty root guarantees the fallback errors regardless of the ambient
        // cwd/CLAUDE_PROJECT_DIR the heuristic reads.)
        let root = temp_projects_root(line!(), &[]);
        let err = resolve_transcript(None, None, Some("ghost-sid"), &root).unwrap_err();
        assert!(err.to_string().contains("No active transcript"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn empty_env_session_id_does_not_short_circuit() {
        // An empty CLAUDE_CODE_SESSION_ID with no --project must reach the
        // is_none() branch, be dropped by the non-empty filter, and fall
        // through to the heuristic (error against the empty projects root) —
        // never a spurious scan for a `.jsonl` file with an empty stem.
        let root = temp_projects_root(line!(), &[]);
        let err = resolve_transcript(None, None, Some(""), &root).unwrap_err();
        assert!(err.to_string().contains("No active transcript"));
        std::fs::remove_dir_all(&root).ok();
    }
}
