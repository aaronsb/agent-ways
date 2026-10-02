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
    /// The transcript file the figures were read from.
    pub transcript: String,
    /// The last assistant usage entries, oldest first (ADR-182). The keepwarm
    /// sensor reads the idle clock and the cache verdict from these.
    pub usage_tail: Vec<UsageEntry>,
}

/// One assistant message's API usage, as the transcript records it.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct UsageEntry {
    /// ISO-8601 timestamp as written in the transcript.
    pub at: String,
    /// The same instant as Unix seconds, 0 when the timestamp did not parse.
    pub at_epoch: u64,
    pub model: String,
    pub input: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
    pub output: u64,
    /// Cache tier the write landed on: "1h", "5m", or "" when nothing was written.
    pub tier: String,
}

/// How many assistant usage entries `usage_tail` carries. A ten-step turn
/// inside one sensor poll interval still fits with room to spare.
pub const USAGE_TAIL_LEN: usize = 32;

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

/// Like `get_context`, but reads one named transcript file. This is the path
/// a hook payload hands over as `transcript_path`: authoritative for the
/// invoking agent (a subagent's own transcript, not its parent's) and free of
/// the directory walk the session-id lookup pays.
pub fn get_context_for_transcript(transcript: &str) -> Result<ContextInfo> {
    let path = PathBuf::from(transcript);
    if !path.is_file() {
        anyhow::bail!("No transcript at: {transcript}");
    }
    context_from_transcript(path)
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
    let projects_root = projects_root();
    let env_session_id = std::env::var("CLAUDE_CODE_SESSION_ID").ok();
    let transcript = resolve_transcript(
        project_dir,
        session_id,
        env_session_id.as_deref(),
        &projects_root,
    )?;
    context_from_transcript(transcript)
}

/// Read one transcript file into a `ContextInfo`: model, window, usage.
fn context_from_transcript(transcript: PathBuf) -> Result<ContextInfo> {
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
    let pct_used = (tokens_used * 100).checked_div(window_tokens).unwrap_or(0);
    let pct_remaining = 100u64.saturating_sub(pct_used);

    let usage_tail = read_usage_tail(&content, USAGE_TAIL_LEN);

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
        transcript: transcript.to_string_lossy().to_string(),
        usage_tail,
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
        return claude_sessions::find_transcript_in(projects_root, None, sid)
            .ok_or_else(|| anyhow::anyhow!("No transcript found for session: {sid}"));
    }

    // No explicit --project: trust the environment's session id first. Only
    // fall through to the cwd heuristic if it is absent or its transcript is
    // missing, so behaviour outside a live session is unchanged.
    if project_dir.is_none() {
        if let Some(sid) = env_session_id.filter(|s| !s.is_empty()) {
            if let Some(transcript) = claude_sessions::find_transcript_in(projects_root, None, sid) {
                return Ok(transcript);
            }
        }
    }

    let project = project_dir
        .map(|s| s.to_string())
        .or_else(crate::util::project_root)
        .unwrap_or_else(|| ".".to_string());

    claude_sessions::find_project_dir_in(projects_root, &project)
        .and_then(|dir| claude_sessions::newest_transcript(&dir))
        .ok_or_else(|| anyhow::anyhow!("No active transcript found for project: {project}"))
}

pub fn run(project: Option<&str>, session: Option<&str>, json_out: bool) -> Result<()> {
    let ctx = match session {
        Some(sid) => get_context_for_session(sid)?,
        None => get_context(project)?,
    };

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
            "transcript": ctx.transcript,
            "last_assistant_at": ctx.usage_tail.last().map(|u| u.at.clone()),
            "last_assistant_at_epoch": ctx.usage_tail.last().map(|u| u.at_epoch),
            "usage_tail": ctx.usage_tail,
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

    let bar_style = if ctx.pct_used < 50 {
        Style::new().role(Role::Ok)
    } else if ctx.pct_used < 75 {
        Style::new().role(Role::Warn).bold()
    } else {
        Style::new().role(Role::Err)
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

    println!("  {} {}%", paint(bar_style, &bar), ctx.pct_used);
    println!();
    println!(
        "  {} / {total_k}K tokens used  {}",
        paint(Style::new().bold(), format!("{used_k}K")),
        paint(Role::Muted, format!("({remaining_k}K remaining)"))
    );
    println!(
        "  {}",
        paint(Role::Muted, format!("Model: {}  Method: {}", ctx.model, ctx.method))
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
    claude_sessions::usage::last_model(content).unwrap_or_else(|| UNKNOWN_MODEL.to_string())
}

/// Sentinel `detect_model` returns when the transcript holds no assistant turn
/// yet — the launch race. It is the *absence* of a model, not a model id.
pub(crate) const UNKNOWN_MODEL: &str = "unknown";

/// Resolve the window for a transcript's detected model through the one resolver
/// (ADR-166). The `"unknown"` sentinel is an absent model, not a model named
/// "unknown", so it is passed as `None`.
fn resolve_window(content: &str) -> context_window::ContextWindow {
    let model = detect_model(content);
    let known = (model != UNKNOWN_MODEL).then_some(model.as_str());
    context_window::resolve(known)
}

fn read_token_usage(content: &str) -> (u64, String) {
    // The newest turn's API usage: cache reads reflect the context sent.
    if let Some(tokens) = claude_sessions::usage::last_context_tokens(content) {
        return (tokens, "api".to_string());
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

/// The last `n` assistant messages that carry usage, oldest first. Sentinel
/// turns (`<synthetic>`) carry no usage and are skipped by the usage check.
///
/// Claude Code writes one transcript line per content block, and every line
/// of one response carries the same `message.id` and the same usage. One
/// entry per id, keyed on the last line written, so its timestamp is the
/// latest one for that response.
fn read_usage_tail(content: &str, n: usize) -> Vec<UsageEntry> {
    let mut tail: Vec<UsageEntry> = Vec::new();
    let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    for line in content.lines().rev() {
        if tail.len() >= n {
            break;
        }
        if !line.contains("cache_read_input_tokens") {
            continue;
        }
        let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        if val.get("type").and_then(|t| t.as_str()) != Some("assistant") {
            continue;
        }
        let Some(message) = val.get("message") else { continue };
        let Some(usage) = message.get("usage") else { continue };
        if let Some(id) = message.get("id").and_then(|i| i.as_str()) {
            if !seen_ids.insert(id.to_string()) {
                continue;
            }
        }
        let at = val.get("timestamp").and_then(|t| t.as_str()).unwrap_or("").to_string();
        let creation = usage.get("cache_creation");
        let tier_tokens = |key: &str| {
            creation.and_then(|c| c.get(key)).and_then(|v| v.as_u64()).unwrap_or(0)
        };
        let tier = if tier_tokens("ephemeral_1h_input_tokens") > 0 {
            "1h"
        } else if tier_tokens("ephemeral_5m_input_tokens") > 0 {
            "5m"
        } else {
            ""
        };
        tail.push(UsageEntry {
            at_epoch: agent_fmt::when::parse_utc_iso(&at).unwrap_or(0),
            at,
            model: message.get("model").and_then(|m| m.as_str()).unwrap_or("").to_string(),
            input: usage["input_tokens"].as_u64().unwrap_or(0),
            cache_read: usage["cache_read_input_tokens"].as_u64().unwrap_or(0),
            cache_creation: usage["cache_creation_input_tokens"].as_u64().unwrap_or(0),
            output: usage["output_tokens"].as_u64().unwrap_or(0),
            tier: tier.to_string(),
        });
    }
    tail.reverse();
    tail
}

/// The root every session transcript lives under, one directory per project.
pub(crate) fn projects_root() -> PathBuf {
    ways_core::paths::transcripts_root()
}

use agent_theme::{paint, Role, Style};

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
    fn explicit_long_project_finds_its_truncated_dir() {
        // Claude Code cuts a slug over 200 characters and appends a hash; the
        // project branch looked for the uncut slug and found nothing.
        let project = format!("/srv/{}", "deep_dir/".repeat(30));
        let project = project.trim_end_matches('/');
        let name = claude_sessions::project_slug(project);
        // The hash Claude Code 2.1.287 computes for this path, under node.
        assert!(name.ends_with("-deep-d-8gbmig"), "{name}");
        let root = temp_projects_root(line!(), &[(name.as_str(), "sid-long")]);
        let got = resolve_transcript(Some(project), None, None, &root).unwrap();
        assert_eq!(got, root.join(&name).join("sid-long.jsonl"));
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

    #[test]
    fn usage_tail_is_oldest_first_and_skips_non_assistant_lines() {
        let a = r#"{"type":"assistant","timestamp":"2026-09-17T20:00:00Z","message":{"model":"claude-fable-5-1","usage":{"input_tokens":10,"cache_creation_input_tokens":90000,"cache_read_input_tokens":0,"output_tokens":5,"cache_creation":{"ephemeral_1h_input_tokens":90000,"ephemeral_5m_input_tokens":0}}}}"#;
        let user = r#"{"type":"user","timestamp":"2026-09-17T20:01:00Z","message":{"content":"cache_read_input_tokens in a prompt"}}"#;
        let b = r#"{"type":"assistant","timestamp":"2026-09-17T20:02:00Z","message":{"model":"claude-fable-5-1","usage":{"input_tokens":3,"cache_creation_input_tokens":120,"cache_read_input_tokens":90000,"output_tokens":2}}}"#;
        let content = format!("{a}\n{user}\n{b}\n");
        let tail = read_usage_tail(&content, 32);
        assert_eq!(tail.len(), 2);
        assert_eq!(tail[0].cache_creation, 90000);
        assert_eq!(tail[0].tier, "1h");
        assert_eq!(tail[0].at_epoch, 1_789_675_200);
        assert_eq!(tail[1].cache_read, 90000);
        assert_eq!(tail[1].tier, "");
        assert_eq!(read_usage_tail(&content, 1)[0].at, "2026-09-17T20:02:00Z");
    }

    #[test]
    fn usage_tail_collapses_the_lines_of_one_response() {
        // One response, three content blocks, three lines with the same id and usage.
        let line = |ts: &str| format!(r#"{{"type":"assistant","timestamp":"{ts}","message":{{"id":"msg_1","model":"claude-fable-5-1","usage":{{"input_tokens":3,"cache_creation_input_tokens":24000,"cache_read_input_tokens":23805,"output_tokens":2}}}}}}"#);
        let content = format!("{}\n{}\n{}\n", line("2026-09-17T20:02:00Z"), line("2026-09-17T20:02:01Z"), line("2026-09-17T20:02:02Z"));
        let tail = read_usage_tail(&content, 32);
        assert_eq!(tail.len(), 1);
        assert_eq!(tail[0].at, "2026-09-17T20:02:02Z");
    }
}
