//! Session state management — markers, epochs, token positions, scope detection.
//!
//! All session state lives under `sessions_root()/{session_id}/` as a directory
//! tree (`/tmp/.claude-sessions-{uid}/` on Unix, `%LOCALAPPDATA%/claude-ways/`
//! on Windows). Way IDs map directly to paths (no dash-encoding).
//! This module owns all reads and writes to session state.

use std::path::{Path, PathBuf};

mod engagement;

// Re-export the ADR-123 firing-dynamics engagement cluster so the rest
// of the crate keeps addressing these items as `session::REFIRE_FLOOR`,
// `session::way_fire_outcome`, etc. — no call-site churn from the
// structural split (issue #52).
pub use engagement::{
    first_suppression_in_window, lock_engagement, record_way_fire, way_fire_outcome,
    way_refire_threshold_k, EngagementLock, FireDecision, FireOutcome, REFIRE_FLOOR,
};

// ── Session directory ──────────────────────────────────────────

/// `$XDG_RUNTIME_DIR/claude-sessions`, or `None` when the variable is unset or
/// empty. Not the absolute-path guard of `paths::xdg_dir`: a Windows `C:\…`
/// value is a usable runtime dir here.
fn runtime_sessions_root(xdg: Option<String>) -> Option<String> {
    xdg.filter(|x| !x.is_empty()).map(|x| format!("{x}/claude-sessions"))
}

/// Per-user sessions root. The one copy of the rule: the binary exports it to
/// every macro and postcheck it runs as `WAYS_SESSIONS_ROOT`, and other
/// scripts read it from `ways sessions-root`. Resolution order:
///   1. `$XDG_RUNTIME_DIR/claude-sessions`            (Linux/systemd — already per-user)
///   2. Windows: `%LOCALAPPDATA%/claude-ways/sessions`  (per-user)
///   3. `/tmp/.claude-sessions-{uid}`                 (other Unix)
pub fn sessions_root() -> String {
    // 1. XDG_RUNTIME_DIR (already per-user, no UID needed) — wins on any platform.
    if let Some(root) = runtime_sessions_root(std::env::var("XDG_RUNTIME_DIR").ok()) {
        return root;
    }

    // 2. Windows: per-user LOCALAPPDATA base.
    #[cfg(windows)]
    {
        format!("{}/claude-ways/sessions", win_user_base())
    }

    // 3. Other Unix: /tmp with a UID namespace.
    #[cfg(not(windows))]
    {
        let uid = std::env::var("EUID")
            .or_else(|_| std::env::var("UID"))
            .unwrap_or_else(|_| {
                std::process::Command::new("id")
                    .arg("-u")
                    .output()
                    .ok()
                    .and_then(|o| String::from_utf8(o.stdout).ok())
                    .map(|s| s.trim().to_string())
                    .unwrap_or_else(|| "0".to_string())
            });
        format!("/tmp/.claude-sessions-{uid}")
    }
}

/// Per-user base directory for transient session state on Windows: `%LOCALAPPDATA%`,
/// falling back to the system temp dir if it is somehow unset.
#[cfg(windows)]
fn win_user_base() -> String {
    std::env::var("LOCALAPPDATA")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| std::env::temp_dir().to_string_lossy().into_owned())
}

/// Root directory for a session's state.
pub fn session_dir(session_id: &str) -> PathBuf {
    PathBuf::from(format!("{}/{session_id}", sessions_root()))
}

/// Whether `session_id` names one directory directly under the sessions root:
/// no separator, no leading dot, not empty. Every path that removes a
/// session's state checks it first, so an id from a hook payload or the
/// command line never reaches the root itself or a directory beside it.
pub fn is_plain_session_id(session_id: &str) -> bool {
    let ok = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    let mut chars = session_id.chars();
    match chars.next() {
        Some(c) if ok(c) => chars.all(|c| ok(c) || c == '.'),
        _ => false,
    }
}

/// Path to the per-session queued-message scan mark (ADR-161). Stores the newest
/// `queue-operation`/`enqueue` transcript timestamp already matched by the
/// PostToolUse queued-message lane, so each mid-turn operator message is scanned
/// at most once.
pub fn queued_scan_mark_path(session_id: &str) -> PathBuf {
    session_dir(session_id).join("queued-scan-mark")
}

/// Read the queued-message scan mark (an ISO-8601 timestamp), if one exists.
pub fn read_queued_scan_mark(session_id: &str) -> Option<String> {
    std::fs::read_to_string(queued_scan_mark_path(session_id))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Advance the queued-message scan mark to the newest consumed timestamp.
/// Best-effort: a write failure only risks re-scanning a message, never loss.
pub fn write_queued_scan_mark(session_id: &str, ts: &str) {
    let path = queued_scan_mark_path(session_id);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, ts);
}

/// The Stop hook's record of Claude's last response, read by the next
/// UserPromptSubmit for the embed lane (ADR-155 §3). Session state like the
/// rest, so `ways session reset` and the SessionStart clear remove it with the session.
pub fn response_context_path(session_id: &str) -> PathBuf {
    session_dir(session_id).join("response-context.json")
}

/// Ensure a path's parent directories exist.
fn ensure_parent(path: &Path) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
}

// ── Way markers ─────────────────────────────────────────────────

/// Check if a way has been shown for the current agent.
/// Subagent markers use agent_id — a subagent firing a way does NOT
/// prevent the main agent (or other subagents) from also getting it.
pub fn way_is_shown(way_id: &str, session_id: &str) -> bool {
    way_marker_path(way_id, session_id).exists()
}

/// Write the way marker with token position, scope, and agent_id.
pub fn stamp_way_marker(way_id: &str, session_id: &str, token_position: u64) {
    let path = way_marker_path(way_id, session_id);
    ensure_parent(&path);
    let scope = detect_scope(session_id);
    let _ = std::fs::write(&path, format!("{token_position}\t{scope}\t{}", current_agent()));
}

fn way_marker_path(way_id: &str, session_id: &str) -> PathBuf {
    session_dir(session_id)
        .join("ways")
        .join(way_id)
        .join(format!(".marker.{}", current_agent()))
}

// ── Agent identity (#815) ───────────────────────────────────────

/// The id the top-level agent's state is keyed by.
pub const MAIN_AGENT: &str = "main";

/// The agent this process serves. Claude Code reports a subagent's id only as
/// `agent_id` in the hook payload, and `ways hook` exports it as
/// `CLAUDE_AGENT_ID` for this reader and the macros it runs. Only an unset or
/// empty value is the main agent; any other value is keyed by [`agent_key`].
pub fn current_agent() -> String {
    std::env::var("CLAUDE_AGENT_ID")
        .ok()
        .filter(|a| !a.is_empty())
        .map_or_else(|| MAIN_AGENT.to_string(), |a| agent_key(&a))
}

/// Longest agent id used as a path component as it stands. Claude Code's ids
/// are 17 to 30 characters.
const MAX_AGENT_ID: usize = 64;

/// The key a reported agent id is stored under. A plain id of at most
/// [`MAX_AGENT_ID`] characters is its own key. Any other id (one with a
/// separator or a leading dot, an over-long one, or the literal `main`) is
/// keyed by `h` and the 16-hex FNV-1a hash of its bytes. Hashing rather than
/// falling back to `main` keeps such an agent's state its own, and keeps the
/// key short and safe as a path component. The mapping is idempotent.
pub fn agent_key(raw: &str) -> String {
    if raw != MAIN_AGENT && raw.len() <= MAX_AGENT_ID && is_plain_session_id(raw) {
        raw.to_string()
    } else {
        format!("h{:016x}", agent_identity::identity::fnv1a_64(raw.as_bytes()))
    }
}

/// The directory holding the current agent's firing state: engagement, way
/// tokens, way epochs, the epoch counter, and check fires. Subagent hooks
/// report the parent's session id, so without this every agent in a session
/// would share them.
pub fn agent_state_dir(session_id: &str) -> PathBuf {
    agent_state_dir_for(session_id, &current_agent())
}

/// [`agent_state_dir`] for a named agent. The main agent keeps the session
/// root, so state written before per-agent keying reads as the main agent's;
/// a subagent's state lives under `agents/<agent_id>/`.
pub fn agent_state_dir_for(session_id: &str, agent: &str) -> PathBuf {
    let dir = session_dir(session_id);
    if agent == MAIN_AGENT {
        dir
    } else {
        dir.join("agents").join(agent)
    }
}

/// The agents with firing state in a session: `main` first, then each
/// subagent under `agents/`, by id.
pub fn agents_in(session_id: &str) -> Vec<String> {
    let mut subagents: Vec<String> = std::fs::read_dir(session_dir(session_id).join("agents"))
        .map(|d| {
            d.flatten()
                .filter(|e| e.path().is_dir())
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    subagents.sort();
    std::iter::once(MAIN_AGENT.to_string()).chain(subagents).collect()
}

/// One agent's recorded state, for readers outside its hooks (`ways session
/// ways`): its epoch counter, a way's token position and check fires, and its
/// current token position.
pub struct AgentState<'a> {
    pub session_id: &'a str,
    pub agent: &'a str,
}

impl AgentState<'_> {
    fn value(&self, parts: &[&str]) -> u64 {
        let mut path = agent_state_dir_for(self.session_id, self.agent);
        for p in parts {
            path.push(p);
        }
        read_u64_path(&path)
    }

    pub fn epoch(&self) -> u64 {
        self.value(&["epoch"])
    }

    pub fn way_tokens(&self, way_id: &str) -> u64 {
        self.value(&["way-tokens", way_id, ".value"])
    }

    pub fn check_fires(&self, way_id: &str) -> u64 {
        self.value(&["check-fires", way_id, ".value"])
    }

    /// The agent's token position now, from its own transcript; 0 when none.
    pub fn token_position(&self) -> u64 {
        transcript_in(
            &ways_core::paths::claude_dir(),
            None,
            &crate::util::project_dir(),
            self.session_id,
            self.agent,
        )
        .map_or(0, |t| token_position_of(&t))
    }
}

// ── Epochs ──────────────────────────────────────────────────────

/// Read the current epoch for a session.
pub fn get_epoch(session_id: &str) -> u64 {
    let path = agent_state_dir(session_id).join("epoch");
    read_u64_path(&path)
}

/// Bump the epoch counter, returning the new value.
pub fn bump_epoch(session_id: &str) -> u64 {
    let path = agent_state_dir(session_id).join("epoch");
    ensure_parent(&path);
    let next = read_u64_path(&path) + 1;
    let _ = std::fs::write(&path, next.to_string());
    next
}

/// Stamp when a way was last shown (epoch).
pub fn stamp_way_epoch(way_id: &str, session_id: &str, epoch: u64) {
    let path = agent_state_dir(session_id).join("way-epochs").join(way_id).join(".value");
    ensure_parent(&path);
    let _ = std::fs::write(&path, epoch.to_string());
}

/// Get the epoch when a way was last shown.
pub fn get_way_epoch(way_id: &str, session_id: &str) -> u64 {
    let path = agent_state_dir(session_id).join("way-epochs").join(way_id).join(".value");
    read_u64_path(&path)
}

/// Get epoch distance since a way last fired.
pub fn epoch_distance(way_id: &str, session_id: &str) -> u64 {
    let current = get_epoch(session_id);
    let way_ep = get_way_epoch(way_id, session_id);
    current.saturating_sub(way_ep)
}

// ── Token position (ADR-123/126 re-disclosure) ──────────────────────

/// The current agent's own transcript, the one file its token position,
/// refire window and fire's model are read from. For the main agent: the
/// hook's `transcript_path` when it names this session, else the session-id
/// lookup. For a subagent: its `agent-<id>.jsonl` under the session's
/// `subagents/` directory. The parent's transcript never stands in for a
/// subagent's.
pub fn current_transcript(session_id: &str) -> Option<PathBuf> {
    transcript_in(
        &ways_core::paths::claude_dir(),
        crate::cmd::show::firing_transcript(),
        &crate::util::project_dir(),
        session_id,
        &current_agent(),
    )
}

/// [`current_transcript`] against an explicit config dir, hook transcript
/// and agent, for tests.
pub(crate) fn transcript_in(
    claude: &claude_sessions::ClaudeDir,
    hook_transcript: Option<&str>,
    project_dir: &str,
    session_id: &str,
    agent: &str,
) -> Option<PathBuf> {
    if agent == MAIN_AGENT {
        session_transcript(claude, hook_transcript, project_dir, session_id)
    } else {
        subagent_transcript(claude, hook_transcript, project_dir, session_id, agent)
    }
}

/// The current agent's token position, read from [`current_transcript`]; 0
/// when there is none.
pub fn get_token_position(session_id: &str) -> u64 {
    current_transcript(session_id).map_or(0, |t| token_position_of(&t))
}

/// The newest turn that reports usage; a zero-usage synthetic turn does not
/// reset the position.
fn token_position_of(transcript: &Path) -> u64 {
    std::fs::read_to_string(transcript)
        .ok()
        .and_then(|c| claude_sessions::usage::last_context_tokens(&c))
        .unwrap_or(0)
}

#[cfg(test)]
fn token_position_in(
    claude: &claude_sessions::ClaudeDir,
    hook_transcript: Option<&str>,
    project_dir: &str,
    session_id: &str,
    agent: &str,
) -> u64 {
    transcript_in(claude, hook_transcript, project_dir, session_id, agent).map_or(0, |t| token_position_of(&t))
}

/// The session's own transcript: the hook's when its stem is the session id,
/// else the session-id lookup.
fn session_transcript(
    claude: &claude_sessions::ClaudeDir,
    hook_transcript: Option<&str>,
    project_dir: &str,
    session_id: &str,
) -> Option<PathBuf> {
    hook_transcript
        .map(PathBuf::from)
        .filter(|t| t.file_stem().is_some_and(|s| s == session_id) && t.is_file())
        .or_else(|| claude.find_transcript(Some(project_dir), session_id))
}

/// A subagent's transcript. Claude Code writes a Task subagent's to
/// `<session>/subagents/agent-<id>.jsonl` and a workflow agent's to
/// `<session>/subagents/workflows/<run>/agent-<id>.jsonl`, beside the
/// session's own `<session>.jsonl`. Claude Code's hooks reference says a
/// hook's `transcript_path` always names the main session's transcript, so
/// the first branch below does not fire today; it keeps the lookup right if
/// a hook ever names the subagent's own file.
fn subagent_transcript(
    claude: &claude_sessions::ClaudeDir,
    hook_transcript: Option<&str>,
    project_dir: &str,
    session_id: &str,
    agent: &str,
) -> Option<PathBuf> {
    let file = format!("agent-{agent}.jsonl");
    if let Some(t) = hook_transcript
        .map(PathBuf::from)
        .filter(|t| t.file_name().is_some_and(|n| n == file.as_str()) && t.is_file())
    {
        return Some(t);
    }
    let subagents = session_transcript(claude, hook_transcript, project_dir, session_id)?
        .with_extension("")
        .join("subagents");
    let direct = subagents.join(&file);
    if direct.is_file() {
        return Some(direct);
    }
    std::fs::read_dir(subagents.join("workflows"))
        .ok()?
        .flatten()
        .map(|run| run.path().join(&file))
        .find(|p| p.is_file())
}

/// Stamp the token position when a way was last shown.
pub fn stamp_way_tokens(way_id: &str, session_id: &str, position: u64) {
    let path = agent_state_dir(session_id).join("way-tokens").join(way_id).join(".value");
    ensure_parent(&path);
    let _ = std::fs::write(&path, position.to_string());
}

// ADR-123 engine integration lives in `session::engagement` — see the
// re-exports at the top of this file. Added in issue #52 when session.rs
// crossed the 800-line priority threshold.

/// Detect context window for a specific session by project path and session ID.
pub fn detect_context_window_for(project: &str, session_id: &str) -> u64 {
    // No transcript reads as empty: no model, so the default window.
    let transcript = ways_core::paths::claude_dir()
        .find_transcript(Some(project), session_id)
        .unwrap_or_default();
    context_window_from_transcript(&transcript)
}

/// Scan a transcript to detect the model, and resolve its context window through
/// the one resolver (ADR-166).
///
/// Uses `resolve_for_foreign_session`, **not** the env-honoring `resolve`: every
/// caller here (`detect_context_window_for`) is a replay tool — `introspect`
/// replay, live and dump — re-evaluating a *recorded* session identified by
/// project + id, not the operator's live session. The window of a recorded session
/// is a property of the model it ran, so the operator's `CLAUDE_CONTEXT_WINDOW`
/// (which states *their current* session's window) must not rescale it — the same
/// override-leak the peer sensor avoids. The live gauge (`cmd::context`) keeps
/// honoring the override, because there the operator's window is the right answer.
///
/// This is load-bearing beyond reporting: ADR-126 scales a way's refire half-life
/// as a fraction of the window, so a wrong answer here rescales the disclosure
/// curve for every way in the replayed session.
fn context_window_from_transcript(transcript: &std::path::Path) -> u64 {
    let content = std::fs::read_to_string(transcript).unwrap_or_default();
    ways_core::context_window::resolve_for_foreign_session(
        model_from_transcript(&content).as_deref(),
    )
    .tokens
}

/// The model id from the transcript's most recent *real* assistant turn. `None`
/// before the first assistant turn is written — the launch race a monitor hits
/// when it starts seconds into a session. Live views must therefore re-resolve on
/// refresh rather than cache a startup answer taken before the model had spoken.
///
/// Sentinel turns (`<synthetic>`, written for interrupts and API errors) are
/// skipped rather than returned: an interrupted turn does not change which model
/// the session is running, and treating the sentinel as the model would resolve a
/// live 1M session to the 200K default.
fn model_from_transcript(content: &str) -> Option<String> {
    claude_sessions::usage::last_model(content)
}

#[cfg(test)]
mod context_window_tests {
    use super::*;

    #[test]
    fn detects_opus_1m_but_falls_back_before_first_model_turn() {
        let dir = std::env::temp_dir();
        let opus = dir.join(format!("ways-ctx-opus-{}.jsonl", std::process::id()));
        let early = dir.join(format!("ways-ctx-early-{}.jsonl", std::process::id()));

        // A transcript with an opus-4 assistant turn resolves to the 1M window.
        // This path resolves for a foreign/recorded session, so it never reads
        // CLAUDE_CONTEXT_WINDOW — the assertion is env-independent by construction.
        std::fs::write(
            &opus,
            "{\"type\":\"user\"}\n{\"type\":\"assistant\",\"message\":{\"model\":\"claude-opus-4-8\"}}\n",
        )
        .unwrap();
        assert_eq!(context_window_from_transcript(&opus), 1_000_000);

        // The launch race: the monitor can start seconds into a session, before the
        // first assistant turn is written. With no model to read, detection returns
        // the 200K default — NOT 1M — which is why the live view must re-detect on
        // refresh rather than cache this startup value for the whole session. Again
        // env-independent: the foreign-session resolver does not consult the override.
        std::fs::write(&early, "{\"type\":\"user\"}\n").unwrap();
        assert_eq!(context_window_from_transcript(&early), 200_000);

        let _ = std::fs::remove_file(&opus);
        let _ = std::fs::remove_file(&early);
    }

    #[test]
    fn walks_back_past_a_synthetic_newest_turn() {
        // Regression for the sentinel-skip: a real opus turn followed by a
        // `<synthetic>` interrupt must still resolve to opus's 1M window, not the
        // 200K default the sentinel alone would yield.
        let dir = std::env::temp_dir();
        let f = dir.join(format!("ways-ctx-synth-{}.jsonl", std::process::id()));
        std::fs::write(
            &f,
            "{\"type\":\"assistant\",\"message\":{\"model\":\"claude-opus-4-8\"}}\n\
             {\"type\":\"assistant\",\"message\":{\"model\":\"<synthetic>\"}}\n",
        )
        .unwrap();
        assert_eq!(
            context_window_from_transcript(&f),
            1_000_000,
            "a synthetic newest turn must not mask the real model behind it"
        );
        let _ = std::fs::remove_file(&f);
    }
}

// ── Check fire count ────────────────────────────────────────────

/// Get and increment fire count for a check.
pub fn bump_check_fires(way_id: &str, session_id: &str) -> u64 {
    let path = agent_state_dir(session_id).join("check-fires").join(way_id).join(".value");
    ensure_parent(&path);
    let count = read_u64_path(&path) + 1;
    let _ = std::fs::write(&path, count.to_string());
    count
}

/// Get current fire count without incrementing.
pub fn get_check_fires(way_id: &str, session_id: &str) -> u64 {
    let path = agent_state_dir(session_id).join("check-fires").join(way_id).join(".value");
    read_u64_path(&path)
}

// ── Core marker ─────────────────────────────────────────────────

pub fn stamp_core(session_id: &str) {
    let path = session_dir(session_id).join("core");
    ensure_parent(&path);
    let _ = std::fs::write(&path, agent_fmt::when::now_secs().to_string());
}

pub fn core_is_shown(session_id: &str) -> bool {
    session_dir(session_id).join("core").exists()
}

// ── Scope detection ─────────────────────────────────────────────

/// Detect execution scope: "agent" or "teammate".
pub fn detect_scope(session_id: &str) -> String {
    let path = agent_state_dir(session_id).join("teammate");
    if path.exists() {
        "teammate".to_string()
    } else {
        "agent".to_string()
    }
}

/// Read team name from teammate marker.
pub fn detect_team(session_id: &str) -> Option<String> {
    let path = agent_state_dir(session_id).join("teammate");
    std::fs::read_to_string(&path).ok().map(|s| s.trim().to_string())
}

// ── Subagent switch (#768) ──────────────────────────────────────

/// The marker that switches ways off for one session's subagents and
/// teammates, keyed by the parent's session id, which subagent hooks report.
/// It lives under the durable state root, outside the session directory that
/// compaction and `ways session reset` clear, so a long workflow keeps it.
fn subagents_off_marker(session_id: &str) -> PathBuf {
    crate::paths::state_root().join("subagent-switch").join(session_id)
}

/// Whether this session switched ways off for its subagents and teammates.
pub fn subagents_off(session_id: &str) -> bool {
    is_plain_session_id(session_id) && subagents_off_marker(session_id).exists()
}

/// Switch ways off (`false`) or back on (`true`) for one session's subagents
/// and teammates. Holds until switched back.
pub fn set_subagents(session_id: &str, on: bool) -> std::io::Result<()> {
    let marker = subagents_off_marker(session_id);
    if on {
        match std::fs::remove_file(&marker) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    } else {
        ensure_parent(&marker);
        std::fs::write(&marker, "")
    }
}

/// Remove session switches untouched for longer than `max_age`. Only
/// `ways session subagents on` removes one otherwise, so a session that ended
/// switched off, a mistyped `--session`, and the old id a `/clear` leaves
/// behind would each keep a file forever. Run at SessionStart.
pub fn prune_subagent_switches(max_age: std::time::Duration) {
    let Ok(entries) = std::fs::read_dir(crate::paths::state_root().join("subagent-switch")) else { return };
    for entry in entries.flatten() {
        let stale = entry.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > max_age);
        if stale {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Record a suppressed injection once per agent: a subagent's tool calls each
/// run the hooks, and one line says the whole agent ran without ways. Returns
/// whether this call is the first for the agent.
pub fn first_suppression_for(session_id: &str, agent: &str) -> bool {
    let marker = session_dir(session_id).join("suppressed").join(agent);
    ensure_parent(&marker);
    std::fs::OpenOptions::new().write(true).create_new(true).open(&marker).is_ok()
}

/// Check if a way's scope field matches the current scope.
pub fn scope_matches(scope_field: &str, current_scope: &str) -> bool {
    if scope_field.is_empty() {
        return current_scope == "agent";
    }
    scope_field.split(',').any(|s| s.trim() == current_scope)
}

// ── Metrics ─────────────────────────────────────────────────────

/// Append a tree disclosure metric.
pub fn append_metric(session_id: &str, metric: &serde_json::Value) {
    let path = session_dir(session_id).join("metrics.jsonl");
    ensure_parent(&path);
    if let Ok(line) = serde_json::to_string(metric) {
        append_jsonl_line(&path, &line);
    }
}

/// Append one JSONL record as a single `write` on an `O_APPEND` handle.
///
/// `writeln!` on a `File` issues two writes, the record and then the newline.
/// Parallel hooks append to the same logs, and two processes interleaving as
/// `recA recB \n \n` corrupt both lines, which readers then drop. That lost the
/// `way_fired` and `way_suppressed` rows of two racing PreToolUse hooks in CI
/// (#528). One buffer, one `write_all`, keeps each record whole.
fn append_jsonl_line(path: &std::path::Path, line: &str) {
    use std::io::Write;
    let mut buf = String::with_capacity(line.len() + 1);
    buf.push_str(line);
    buf.push('\n');
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| f.write_all(buf.as_bytes()));
}

// ── Event logging ───────────────────────────────────────────────

/// Size at which the event log is compacted (ADR-134 task E). The near-miss and
/// fire-score streams (tasks A/D) grow `events.jsonl` faster than fires alone,
/// so it needs a ceiling. ~32 MiB ≈ 125k events at the observed ~260 bytes/line
/// — over a year of history at current rates.
const MAX_EVENTS_BYTES: u64 = 32 * 1024 * 1024;
/// Target size kept after compaction (the most recent bytes). The gap to
/// MAX provides hysteresis: ~8 MiB (~30k events) of new logging between
/// compactions, so the rewrite is rare, not per-append.
const KEEP_EVENTS_BYTES: u64 = 24 * 1024 * 1024;
/// Compaction carries every `judge_call` line (#750), so the file settles at
/// KEEP + judge history. Once that history exceeds MAX - KEEP the file is over
/// MAX right after compacting; a rewrite that frees less than this is skipped so
/// appends do not re-read and rewrite the whole file each time. Non-judge lines
/// then accumulate until a compaction frees at least half the MAX-KEEP gap. The
/// file is bounded by KEEP + judge history + MAX-KEEP/2 + that accumulation.
const MIN_FREED_BYTES: u64 = (MAX_EVENTS_BYTES - KEEP_EVENTS_BYTES) / 2;

/// Log an event to the telemetry log ($XDG_STATE/agent-ways/events.jsonl — see paths::events_log).
pub fn log_event(fields: &[(&str, &str)]) {
    log_event_with(fields, &[]);
}

/// [`log_event`] with structured values beside the string fields, for an event
/// that carries a list (`scan_candidates`). Readers key on `event` and read the
/// fields they know, so an extra nested value costs them nothing.
pub fn log_event_with(fields: &[(&str, &str)], extra: &[(&str, serde_json::Value)]) {
    let events_file = crate::paths::events_log();
    if let Some(stats_dir) = events_file.parent() {
        let _ = std::fs::create_dir_all(stats_dir);
    }

    let ts = agent_fmt::when::now_utc_iso();
    let mut obj = serde_json::Map::new();
    obj.insert("ts".to_string(), serde_json::Value::String(ts));
    for (k, v) in fields {
        obj.insert(k.to_string(), serde_json::Value::String(v.to_string()));
    }
    for (k, v) in extra {
        obj.insert(k.to_string(), v.clone());
    }

    if let Ok(line) = serde_json::to_string(&serde_json::Value::Object(obj)) {
        append_jsonl_line(&events_file, &line);
    }

    // Amortized cap: only when the log crosses MAX do we rewrite it to the most
    // recent KEEP bytes. A single stat() per append; the O(n) rewrite happens
    // once per ~8 MiB of growth. Readers always see a complete file: a reader
    // that opened the pre-compaction inode keeps reading it intact (the rename
    // is atomic and unlinks the old name only after), and each compaction
    // publishes a whole file via its own private temp. Concurrent compactions
    // from parallel `ways` processes are last-writer-wins — that drops a bounded
    // window of events, acceptable for a telemetry log, but never tears a line.
    // Age rotation (ADR-701 §2) runs at most daily, gated by a marker file.
    rotate_if_due(&events_file, agent_fmt::when::now_secs(), crate::config::global().event_retention_days);

    if let Ok(meta) = std::fs::metadata(&events_file) {
        if meta.len() > MAX_EVENTS_BYTES {
            let _ = compact_log_tail(&events_file, KEEP_EVENTS_BYTES, MIN_FREED_BYTES);
        }
    }
}

/// Rewrite `path` in place to retain only its most recent `keep_bytes`, cut at a
/// line boundary so the first retained line is whole. The new contents are
/// written to a per-process, per-attempt temp, synced, then atomically renamed
/// over `path` — so a published `events.jsonl` is always a complete file even
/// under concurrent compaction (last rename wins; a bounded window of events may
/// be lost, but no line is ever torn). Oldest events are dropped — telemetry
/// tuning cares about recent behavior, and the cap holds a year-plus of history.
/// `judge_call` lines in the dropped head are carried ahead of the tail (#750).
/// On any failure the original file is left intact and the temp is removed.
fn compact_log_tail(path: &std::path::Path, keep_bytes: u64, min_freed: u64) -> std::io::Result<()> {
    let data = std::fs::read(path)?;
    let keep = keep_bytes as usize;
    if data.len() <= keep {
        return Ok(());
    }
    // Start `keep` bytes from the end, then advance past the next newline so we
    // never retain a partial leading line.
    let cut = data.len() - keep;
    let start = match data[cut..].iter().position(|&b| b == b'\n') {
        Some(off) => cut + off + 1,
        None => data.len(), // single huge line / no boundary: drop it all
    };

    // The judge's spend history must outlive the cut (#750): `ways agent cost`
    // reads `judge_call` lines from this file, so the ones in the dropped head
    // are carried ahead of the kept tail. A call is ~300 bytes, so a year of
    // heavy use adds a few MiB. Carried lines sit in the head region on the
    // next compaction and are carried again, never duplicated.
    let mut out: Vec<u8> = Vec::new();
    for line in data[..start].split(|&b| b == b'\n') {
        if is_judge_call(line) {
            out.extend_from_slice(line);
            out.push(b'\n');
        }
    }
    out.extend_from_slice(&data[start..]);
    if ((data.len() - out.len()) as u64) < min_freed {
        return Ok(()); // would not pay for the rewrite
    }

    // The shared writer's temp is unique per process and call, so two
    // concurrent compactions never write the same file and publish a torn tail.
    agent_settings::writer::write_atomic(path, &out)
}

/// Age rotation runs at most this often; the marker file's mtime is the clock.
const ROTATE_EVERY_SECS: u64 = 86_400;

/// Rotate the event log by age when a day has passed since the last rotation.
/// The marker is written first, so a parallel hook that stats it a moment later
/// sees the slot taken. Returns whether a rotation ran and dropped lines.
fn rotate_if_due(path: &std::path::Path, now: u64, retention_days: u32) -> bool {
    let marker = path.with_extension("rotated");
    // The marker holds the Unix second of the last rotation.
    let recorded = std::fs::read_to_string(&marker).ok().and_then(|s| s.trim().parse::<u64>().ok());
    if recorded.is_some_and(|t| now < t + ROTATE_EVERY_SECS) {
        return false;
    }
    if std::fs::write(&marker, now.to_string()).is_err() {
        return false;
    }
    rotate_log_by_age(path, now, retention_days).unwrap_or(false)
}

/// Drop event lines older than `retention_days` from `path` (ADR-701 §2).
///
/// Kept: lines newer than the cutoff, lines whose `ts` does not parse (never
/// guessed at), and every `judge_call`, as compaction keeps them (#750). The
/// survivors are written to a private temp and renamed over the log, so a
/// reader sees a whole file. Hooks append with `O_APPEND` and take no lock, so
/// bytes appended after the read are copied from the old file's tail onto the
/// new one just before the rename; the window left is the span between that
/// last length check and the rename, microseconds, and the same bounded loss
/// the size compaction accepts. Nothing newer than the cutoff is ever selected
/// for removal, so the current session's events are never the target.
/// Returns whether any line was dropped.
fn rotate_log_by_age(path: &std::path::Path, now: u64, retention_days: u32) -> std::io::Result<bool> {
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    let cutoff = now.saturating_sub(u64::from(retention_days.max(1)) * 86_400);
    let mut out: Vec<u8> = Vec::with_capacity(data.len());
    let mut dropped = false;
    for chunk in data.split_inclusive(|&b| b == b'\n') {
        let line = chunk.strip_suffix(b"\n").unwrap_or(chunk);
        if line.is_empty() || !is_older_than(line, cutoff) || is_judge_call(line) {
            out.extend_from_slice(chunk);
        } else {
            dropped = true;
        }
    }
    if !dropped {
        return Ok(false);
    }
    // Carry events appended since the read.
    if let Ok(now_len) = std::fs::metadata(path).map(|m| m.len()) {
        if now_len > data.len() as u64 {
            use std::io::{Read, Seek, SeekFrom};
            let mut f = std::fs::File::open(path)?;
            f.seek(SeekFrom::Start(data.len() as u64))?;
            f.read_to_end(&mut out)?;
        }
    }
    agent_settings::writer::write_atomic(path, &out)?;
    Ok(true)
}

/// Whether an event line's `ts` parses and is before `cutoff`.
fn is_older_than(line: &[u8], cutoff: u64) -> bool {
    serde_json::from_slice::<serde_json::Value>(line)
        .ok()
        .and_then(|v| v.get("ts").and_then(|t| t.as_str()).and_then(agent_fmt::when::parse_utc_iso))
        .is_some_and(|ts| ts < cutoff)
}

/// A whole `judge_call` event line (substring prefilter, then a real parse).
fn is_judge_call(line: &[u8]) -> bool {
    const NEEDLE: &[u8] = b"judge_call";
    line.windows(NEEDLE.len()).any(|w| w == NEEDLE)
        && serde_json::from_slice::<serde_json::Value>(line).is_ok_and(|v| v.get("event").and_then(|e| e.as_str()) == Some("judge_call"))
}

// ── Domain disable check ────────────────────────────────────────

/// Check if a domain is disabled.
/// config::global() — future migration: ctx.config.disabled_domains
pub fn domain_disabled(domain: &str) -> bool {
    crate::config::global().disabled_domains.iter().any(|d| d == domain)
}

/// Check if a specific way is disabled in the current project (ADR-131).
/// Project-scope only — sourced exclusively from `{project}/.claude/ways.yaml`.
/// A toggle may name the way or a `dir/*` prefix; the most specific wins (ADR-701 §1).
/// config::global() — future migration: ctx.config.disabled_ways
pub fn way_disabled(way_id: &str) -> bool {
    crate::config::global().way_disabled(way_id)
}


// ── Way file resolution ─────────────────────────────────────────

/// Resolve a way ID to its file path. Precedence: project > user > core (ADR-143).
/// Returns (path, is_project_local). User and core both report `false` (non-project),
/// but the user root is checked first so a user way shadows a same-named core way.
pub fn resolve_way_file(way_id: &str, project_dir: &str) -> Option<(PathBuf, bool)> {
    resolve_in_roots(way_id, project_dir, find_way_in_dir)
}

/// Resolve a way ID to its check file path. Precedence: project > user > core.
pub fn resolve_check_file(way_id: &str, project_dir: &str) -> Option<(PathBuf, bool)> {
    resolve_in_roots(way_id, project_dir, find_check_in_dir)
}

/// The first root, in `paths::ways_roots` order, whose `way_id` directory
/// `find` resolves; the flag says it was the project's own root.
fn resolve_in_roots(way_id: &str, project_dir: &str, find: fn(&Path) -> Option<PathBuf>) -> Option<(PathBuf, bool)> {
    let project_root = PathBuf::from(project_dir).join(".claude/ways");
    crate::paths::ways_roots(Some(Path::new(project_dir)))
        .into_iter()
        .find_map(|root| find(&root.join(way_id)).map(|f| (f, root == project_root)))
}

/// The way file in a way's directory: its first `.md` that opens with
/// frontmatter, whatever its name.
pub(crate) fn find_way_in_dir(dir: &Path) -> Option<PathBuf> {
    if !dir.is_dir() {
        return None;
    }
    for entry in std::fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let name = path.file_name()?.to_str()?;
        if name.contains(".check.") {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&path) {
            if crate::frontmatter::opens_with_fence(&content) {
                return Some(path);
            }
        }
    }
    None
}

fn find_check_in_dir(dir: &Path) -> Option<PathBuf> {
    if !dir.is_dir() {
        return None;
    }
    for entry in std::fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.ends_with(".check.md") {
            return Some(path);
        }
    }
    None
}

// ── Session enumeration (for list/reset) ────────────────────────

/// List all session IDs that have state directories.
pub fn list_sessions() -> Vec<String> {
    list_sessions_in(Path::new(&sessions_root()))
}

/// List session IDs under an explicit state root. Split out from
/// [`list_sessions`] so callers that already resolved a root enumerate and
/// test against *that* root — enumerating the real `~/.claude` while checking
/// liveness against an injected one is internally inconsistent, and silently
/// makes every injected root look empty.
pub fn list_sessions_in(root: &Path) -> Vec<String> {
    if !root.is_dir() {
        return Vec::new();
    }
    let mut sessions = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    sessions.push(name.to_string());
                }
            }
        }
    }
    sessions.sort();
    sessions
}

/// List all way IDs that have fired in a session (from the ways/ subdirectory).
pub fn list_fired_ways(session_id: &str) -> Vec<String> {
    let ways_dir = session_dir(session_id).join("ways");
    collect_way_ids(&ways_dir, &ways_dir)
}

/// List all way IDs that have epoch stamps in a session.
pub fn list_way_epochs(session_id: &str, agent: &str) -> Vec<(String, u64)> {
    let epochs_dir = agent_state_dir_for(session_id, agent).join("way-epochs");
    let ids = collect_way_ids(&epochs_dir, &epochs_dir);
    ids.into_iter()
        .map(|id| {
            let epoch = read_u64_path(&epochs_dir.join(&id).join(".value"));
            (id, epoch)
        })
        .collect()
}

/// Recursively collect way IDs from a directory tree.
/// Way IDs are directories containing a .marker or .value sentinel file.
fn collect_way_ids(dir: &Path, base: &Path) -> Vec<String> {
    let mut ids = Vec::new();
    if !dir.is_dir() {
        return ids;
    }
    // Check if this directory itself is a way (has .marker.* or old .marker or .value)
    let has_marker = dir.join(".marker").exists()
        || std::fs::read_dir(dir)
            .ok()
            .map(|entries| {
                entries.filter_map(|e| e.ok()).any(|e| {
                    e.file_name().to_string_lossy().starts_with(".marker.")
                })
            })
            .unwrap_or(false);
    if has_marker || dir.join(".value").exists() {
        if let Ok(rel) = dir.strip_prefix(base) {
            let id = rel.display().to_string();
            if !id.is_empty() {
                ids.push(id);
            }
        }
    }
    // Recurse into subdirectories
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                ids.extend(collect_way_ids(&path, base));
            }
        }
    }
    ids.sort();
    ids
}

// ── Helpers ─────────────────────────────────────────────────────

fn read_u64_path(path: &Path) -> u64 {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}


// Tests for the ADR-123 engagement cluster (classify_outcome,
// load_engagement_for_tick, FirstFire → ReFire → Suppressed) live in
// `session::engagement`'s own test module — they moved with the code
// they cover (issue #52).

#[cfg(test)]
mod token_position_tests {
    use super::*;

    #[test]
    fn plain_session_ids_stay_under_the_root() {
        for ok in ["sess-a", "abcdef12-0000-0000-0000-000000000000", "a.b", "_x"] {
            assert!(is_plain_session_id(ok), "{ok}");
        }
        for bad in ["", ".", "..", "../victim", "a/b", ".hidden", "a\\b", "x y"] {
            assert!(!is_plain_session_id(bad), "{bad}");
        }
    }

    /// An empty XDG_RUNTIME_DIR is unset: it once resolved to
    /// `/claude-sessions`.
    #[test]
    fn empty_runtime_dir_is_unset() {
        assert_eq!(runtime_sessions_root(None), None);
        assert_eq!(runtime_sessions_root(Some(String::new())), None);
        assert_eq!(runtime_sessions_root(Some("/run/user/1".into())).as_deref(), Some("/run/user/1/claude-sessions"));
    }

    #[test]
    fn non_ascii_project_paths_find_their_transcript() {
        let root = std::env::temp_dir().join(format!("ways-tokpos-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let claude = claude_sessions::ClaudeDir::at(root.join(".claude"));
        let line = r#"{"type":"assistant","message":{"usage":{"input_tokens":1,"cache_read_input_tokens":41999,"cache_creation_input_tokens":0}}}"#;
        let synthetic = r#"{"type":"assistant","message":{"model":"<synthetic>","usage":{"input_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#;
        // Claude Code's names for these paths: `-srv----x` and `-srv----crab`.
        for (project, dir, sid) in [("/srv/项目 x", "-srv----x", "cjk"), ("/srv/🦀/crab", "-srv----crab", "crab")] {
            let d = root.join(".claude/projects").join(dir);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join(format!("{sid}.jsonl")), format!("{line}\n{synthetic}\n")).unwrap();
            assert_eq!(token_position_in(&claude, None, project, sid, MAIN_AGENT), 42000, "{project}");
        }
        std::fs::remove_dir_all(&root).ok();
    }

    /// A subagent reads its position from its own transcript, in either
    /// layout Claude Code writes, and never from the parent's.
    #[test]
    fn subagents_read_their_own_transcript() {
        let root = std::env::temp_dir().join(format!("ways-tokpos-sub-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let claude = claude_sessions::ClaudeDir::at(root.join(".claude"));
        let turn = |n: u64| {
            format!(r#"{{"type":"assistant","message":{{"usage":{{"input_tokens":{n},"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}}}"#)
                + "\n"
        };
        let project = "/srv/p";
        let dir = root.join(".claude/projects/-srv-p");
        let subagents = dir.join("sess/subagents");
        std::fs::create_dir_all(subagents.join("workflows/wf_1")).unwrap();
        std::fs::write(dir.join("sess.jsonl"), turn(90_000)).unwrap();
        std::fs::write(subagents.join("agent-atask.jsonl"), turn(30_000)).unwrap();
        std::fs::write(subagents.join("workflows/wf_1/agent-awf.jsonl"), turn(20_000)).unwrap();

        assert_eq!(token_position_in(&claude, None, project, "sess", MAIN_AGENT), 90_000);
        assert_eq!(token_position_in(&claude, None, project, "sess", "atask"), 30_000);
        assert_eq!(token_position_in(&claude, None, project, "sess", "awf"), 20_000);
        // The parent's transcript named by the hook still leads to the subagent's.
        let parent = dir.join("sess.jsonl");
        assert_eq!(token_position_in(&claude, parent.to_str(), project, "sess", "atask"), 30_000);
        // No transcript of its own: zero, not the parent's position.
        assert_eq!(token_position_in(&claude, None, project, "sess", "agone"), 0);
        std::fs::remove_dir_all(&root).ok();
    }

    /// The main agent's state stays at the session root, where state written
    /// before per-agent keying lives; a subagent's goes under `agents/<id>`.
    #[test]
    fn main_keeps_the_legacy_state_root() {
        assert_eq!(agent_state_dir_for("s", MAIN_AGENT), session_dir("s"));
        assert_eq!(agent_state_dir_for("s", "a1"), session_dir("s").join("agents").join("a1"));
    }
}

#[cfg(test)]
mod compaction_tests {
    use super::*;

    // ── ADR-701 §2: rotation by age ────────────────────────────────

    const NOW: u64 = 1_800_000_000;
    const DAY: u64 = 86_400;

    fn line(event: &str, age_days: u64, tag: &str) -> String {
        format!("{{\"ts\":\"{}\",\"event\":\"{event}\",\"tag\":\"{tag}\"}}\n", agent_fmt::when::utc_iso(NOW - age_days * DAY))
    }

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ways-rot-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("events.jsonl")
    }

    #[test]
    fn rotation_drops_lines_older_than_the_retention_and_keeps_the_rest() {
        let p = tmp("age");
        let mut body = String::new();
        body.push_str(&line("way_fired", 120, "old"));
        body.push_str(&line("judge_call", 120, "old-judge"));
        body.push_str("not json at all\n");
        body.push_str(&line("way_fired", 89, "recent"));
        body.push_str(&line("way_fired", 0, "today"));
        std::fs::write(&p, &body).unwrap();

        assert!(rotate_log_by_age(&p, NOW, 90).unwrap());
        let got = std::fs::read_to_string(&p).unwrap();
        assert!(!got.contains("\"old\""), "a line past retention is dropped");
        assert!(got.contains("old-judge"), "judge_call history outlives rotation (#750)");
        assert!(got.contains("not json at all"), "a line with no readable ts is kept");
        assert!(got.contains("recent") && got.contains("today"));
    }

    #[test]
    fn rotation_never_touches_a_log_with_nothing_old() {
        let p = tmp("noop");
        let body = format!("{}{}", line("way_fired", 1, "a"), line("way_fired", 0, "b"));
        std::fs::write(&p, &body).unwrap();
        assert!(!rotate_log_by_age(&p, NOW, 90).unwrap());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), body, "the current session's events are never lost");
    }

    #[test]
    fn rotation_on_a_missing_log_is_a_noop() {
        let p = tmp("missing");
        assert!(!rotate_log_by_age(&p, NOW, 90).unwrap());
    }

    #[test]
    fn rotation_runs_once_a_day() {
        let p = tmp("throttle");
        std::fs::write(&p, line("way_fired", 200, "old")).unwrap();
        assert!(rotate_if_due(&p, NOW, 90));
        // An old line appears again: the day's slot is spent.
        std::fs::write(&p, line("way_fired", 200, "old2")).unwrap();
        assert!(!rotate_if_due(&p, NOW + 60, 90));
        assert!(std::fs::read_to_string(&p).unwrap().contains("old2"));
        // A day later it is due again.
        assert!(rotate_if_due(&p, NOW + DAY + 60, 90));
        assert!(!std::fs::read_to_string(&p).unwrap().contains("old2"));
    }

    #[test]
    fn compact_log_tail_keeps_recent_whole_lines() {
        // A unique temp path (process id; no clock/random in this crate's tests).
        let path = std::env::temp_dir().join(format!("ways-evt-{}.jsonl", std::process::id()));
        // 1000 numbered JSON lines (~20 bytes each → ~20 KB).
        let mut content = String::new();
        for i in 0..1000 {
            content.push_str(&format!("{{\"n\":{i}}}\n"));
        }
        std::fs::write(&path, &content).unwrap();
        let before = std::fs::metadata(&path).unwrap().len();

        // Keep ~2 KB → far below the file size, so it must compact.
        compact_log_tail(&path, 2000, 0).unwrap();

        let after = std::fs::read_to_string(&path).unwrap();
        let after_len = after.len() as u64;
        assert!(after_len < before, "file should shrink");
        assert!(after_len <= 2000 + 32, "retained ~keep_bytes (+ one boundary line)");

        let lines: Vec<&str> = after.lines().collect();
        // Every retained line is whole, parseable JSON (no partial leading line).
        for l in &lines {
            serde_json::from_str::<serde_json::Value>(l).expect("retained line is valid JSON");
        }
        // The most recent line is preserved; the oldest are dropped.
        assert_eq!(*lines.last().unwrap(), "{\"n\":999}");
        assert!(!after.contains("{\"n\":0}"), "oldest events dropped");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn compaction_keeps_judge_calls_so_spend_still_counts_them() {
        let path = std::env::temp_dir().join(format!("ways-evt-judge-{}.jsonl", std::process::id()));
        let judge = |ts: &str| format!("{{\"event\":\"judge_call\",\"ts\":\"{ts}\",\"session\":\"s\",\"project\":\"/p\",\"input_tokens\":\"10\",\"output_tokens\":\"1\",\"cost_usd\":\"0.0100\",\"cost_source\":\"provider\"}}\n");
        let filler = |s: &mut String| {
            for i in 0..500 {
                s.push_str(&format!("{{\"event\":\"way_fired\",\"n\":\"{i}\"}}\n"));
            }
        };
        // Two old judge calls buried in filler that the cut drops.
        let mut content = judge("2026-01-05T10:00:00Z");
        filler(&mut content);
        content.push_str(&judge("2026-01-06T10:00:00Z"));
        filler(&mut content);
        content.push_str(&judge("2026-10-01T10:00:00Z"));
        std::fs::write(&path, &content).unwrap();

        compact_log_tail(&path, 2000, 0).unwrap();
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.len() < content.len(), "filler is cut");

        let calls = ways_agent_core::spend::parse_log(&after);
        assert_eq!(calls.len(), 3, "all three judge calls survive");
        assert_eq!(ways_agent_core::spend::covers_since(&calls).as_deref(), Some("2026-01-05T10:00:00Z"));

        // A second compaction carries them again without duplicating.
        compact_log_tail(&path, 2000, 0).unwrap();
        assert_eq!(ways_agent_core::spend::parse_log(&std::fs::read_to_string(&path).unwrap()).len(), 3);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn compaction_skips_a_rewrite_that_frees_too_little_and_keeps_judge_calls() {
        let path = std::env::temp_dir().join(format!("ways-evt-gap-{}.jsonl", std::process::id()));
        let judge = |i: usize| format!("{{\"event\":\"judge_call\",\"ts\":\"2026-01-05T10:00:{:02}Z\",\"cost_source\":\"unknown\"}}\n", i % 60);
        // Judge history (~4 KB) larger than the gap (keep 500, min_freed 1000).
        let mut content: String = (0..50).map(judge).collect();
        std::fs::write(&path, &content).unwrap();
        let (keep, min_freed) = (500, 1000);

        // Appending filler: the first compactions free < min_freed, so the file is left as is.
        for i in 0..10 {
            content.push_str(&format!("{{\"event\":\"way_fired\",\"n\":\"{i}\"}}\n"));
            std::fs::write(&path, &content).unwrap();
            let before = std::fs::read(&path).unwrap();
            compact_log_tail(&path, keep, min_freed).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), before, "no rewrite while it frees too little");
        }

        // Enough filler to free more than min_freed: it compacts, and judge calls survive.
        for i in 0..100 {
            content.push_str(&format!("{{\"event\":\"way_fired\",\"n\":\"{i}\"}}\n"));
        }
        std::fs::write(&path, &content).unwrap();
        compact_log_tail(&path, keep, min_freed).unwrap();
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.len() < content.len(), "compacted once it pays");
        assert_eq!(ways_agent_core::spend::parse_log(&after).len(), 50, "every judge call kept");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn compact_log_tail_noop_when_under_keep() {
        let path = std::env::temp_dir().join(format!("ways-evt-small-{}.jsonl", std::process::id()));
        let content = "{\"n\":1}\n{\"n\":2}\n";
        std::fs::write(&path, content).unwrap();
        compact_log_tail(&path, 1024 * 1024, 0).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn compact_log_tail_drops_oversized_unbroken_blob() {
        // A single line larger than keep_bytes with no newline has no boundary
        // to cut at — the intended behavior is to drop it (it is already corrupt
        // for a line-oriented log), leaving an empty file rather than a partial.
        let path = std::env::temp_dir().join(format!("ways-evt-blob-{}.jsonl", std::process::id()));
        std::fs::write(&path, "x".repeat(5000)).unwrap();
        compact_log_tail(&path, 1000, 0).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn compact_log_tail_is_idempotent() {
        // After one compaction the file is under keep, so a second is a no-op.
        let path = std::env::temp_dir().join(format!("ways-evt-idem-{}.jsonl", std::process::id()));
        let mut content = String::new();
        for i in 0..500 {
            content.push_str(&format!("{{\"n\":{i}}}\n"));
        }
        std::fs::write(&path, &content).unwrap();
        compact_log_tail(&path, 1500, 0).unwrap();
        let once = std::fs::read_to_string(&path).unwrap();
        compact_log_tail(&path, 1500, 0).unwrap();
        let twice = std::fs::read_to_string(&path).unwrap();
        assert_eq!(once, twice, "second compaction is a no-op");
        let _ = std::fs::remove_file(&path);
    }
}
