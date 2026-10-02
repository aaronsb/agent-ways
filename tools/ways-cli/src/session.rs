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

/// Check if a way has been shown this session.
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
    let agent_id = current_agent_id().unwrap_or_else(|| "main".to_string());
    let _ = std::fs::write(&path, format!("{token_position}\t{scope}\t{agent_id}"));
}

fn way_marker_path(way_id: &str, session_id: &str) -> PathBuf {
    let agent_id = current_agent_id().unwrap_or_else(|| "main".to_string());
    session_dir(session_id)
        .join("ways")
        .join(way_id)
        .join(format!(".marker.{agent_id}"))
}

/// Read CLAUDE_AGENT_ID from the environment (set by Claude Code for subagents).
fn current_agent_id() -> Option<String> {
    std::env::var("CLAUDE_AGENT_ID").ok().filter(|s| !s.is_empty())
}

// ── Epochs ──────────────────────────────────────────────────────

/// Read the current epoch for a session.
pub fn get_epoch(session_id: &str) -> u64 {
    let path = session_dir(session_id).join("epoch");
    read_u64_path(&path)
}

/// Bump the epoch counter, returning the new value.
pub fn bump_epoch(session_id: &str) -> u64 {
    let path = session_dir(session_id).join("epoch");
    ensure_parent(&path);
    let next = read_u64_path(&path) + 1;
    let _ = std::fs::write(&path, next.to_string());
    next
}

/// Stamp when a way was last shown (epoch).
pub fn stamp_way_epoch(way_id: &str, session_id: &str, epoch: u64) {
    let path = session_dir(session_id).join("way-epochs").join(way_id).join(".value");
    ensure_parent(&path);
    let _ = std::fs::write(&path, epoch.to_string());
}

/// Get the epoch when a way was last shown.
pub fn get_way_epoch(way_id: &str, session_id: &str) -> u64 {
    let path = session_dir(session_id).join("way-epochs").join(way_id).join(".value");
    read_u64_path(&path)
}

/// Get epoch distance since a way last fired.
pub fn epoch_distance(way_id: &str, session_id: &str) -> u64 {
    let current = get_epoch(session_id);
    let way_ep = get_way_epoch(way_id, session_id);
    current.saturating_sub(way_ep)
}

// ── Token position (ADR-123/126 re-disclosure) ──────────────────────

/// Read this session's token position from its transcript: the hook's own
/// `transcript_path` when it names this session, else the session-id lookup.
pub fn get_token_position(session_id: &str) -> u64 {
    let project_dir = crate::util::project_dir();
    token_position_in(
        &ways_core::paths::claude_dir(),
        crate::cmd::show::firing_transcript(),
        &project_dir,
        session_id,
    )
}

/// [`get_token_position`] against an explicit config dir and hook
/// transcript, for tests.
fn token_position_in(
    claude: &claude_sessions::ClaudeDir,
    hook_transcript: Option<&str>,
    project_dir: &str,
    session_id: &str,
) -> u64 {
    let transcript = hook_transcript
        .map(PathBuf::from)
        .filter(|t| t.file_stem().is_some_and(|s| s == session_id) && t.is_file())
        .or_else(|| claude.find_transcript(Some(project_dir), session_id));
    let transcript = match transcript {
        Some(t) => t,
        None => return 0,
    };

    // The newest turn that reports usage; a zero-usage synthetic turn does not
    // reset the position.
    std::fs::read_to_string(&transcript)
        .ok()
        .and_then(|c| claude_sessions::usage::last_context_tokens(&c))
        .unwrap_or(0)
}

/// Read the token position when a way was last shown.
pub fn get_token_position_for_way(way_id: &str, session_id: &str) -> u64 {
    let path = session_dir(session_id).join("way-tokens").join(way_id).join(".value");
    read_u64_path(&path)
}

/// Stamp the token position when a way was last shown.
pub fn stamp_way_tokens(way_id: &str, session_id: &str, position: u64) {
    let path = session_dir(session_id).join("way-tokens").join(way_id).join(".value");
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
    let path = session_dir(session_id).join("check-fires").join(way_id).join(".value");
    ensure_parent(&path);
    let count = read_u64_path(&path) + 1;
    let _ = std::fs::write(&path, count.to_string());
    count
}

/// Get current fire count without incrementing.
pub fn get_check_fires(way_id: &str, session_id: &str) -> u64 {
    let path = session_dir(session_id).join("check-fires").join(way_id).join(".value");
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
    let path = session_dir(session_id).join("teammate");
    if path.exists() {
        "teammate".to_string()
    } else {
        "agent".to_string()
    }
}

/// Read team name from teammate marker.
pub fn detect_team(session_id: &str) -> Option<String> {
    let path = session_dir(session_id).join("teammate");
    std::fs::read_to_string(&path).ok().map(|s| s.trim().to_string())
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

/// Log an event to the telemetry log ($XDG_STATE/agent-ways/events.jsonl — see paths::events_log).
pub fn log_event(fields: &[(&str, &str)]) {
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
    if let Ok(meta) = std::fs::metadata(&events_file) {
        if meta.len() > MAX_EVENTS_BYTES {
            let _ = compact_log_tail(&events_file, KEEP_EVENTS_BYTES);
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
/// On any failure the original file is left intact and the temp is removed.
fn compact_log_tail(path: &std::path::Path, keep_bytes: u64) -> std::io::Result<()> {
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

    // The shared writer's temp is unique per process and call, so two
    // concurrent compactions never write the same file and publish a torn tail.
    agent_settings::writer::write_atomic(path, &data[start..])
}

// ── Domain disable check ────────────────────────────────────────

/// Check if a domain is disabled.
/// config::global() — future migration: ctx.config.disabled_domains
pub fn domain_disabled(domain: &str) -> bool {
    crate::config::global().disabled_domains.iter().any(|d| d == domain)
}

/// Check if a specific way is disabled in the current project (ADR-131).
/// Project-scope only — sourced exclusively from `{project}/.claude/ways.yaml`.
/// config::global() — future migration: ctx.config.disabled_ways
pub fn way_disabled(way_id: &str) -> bool {
    crate::config::global().disabled_ways().iter().any(|w| w == way_id)
}


// ── Way file resolution ─────────────────────────────────────────

/// Resolve a way ID to its file path. Precedence: project > user > core (ADR-143).
/// Returns (path, is_project_local). User and core both report `false` (non-project),
/// but the user root is checked first so a user way shadows a same-named core way.
pub fn resolve_way_file(way_id: &str, project_dir: &str) -> Option<(PathBuf, bool)> {
    let local_dir = PathBuf::from(project_dir).join(format!(".claude/ways/{way_id}"));
    if let Some(f) = find_way_in_dir(&local_dir) {
        return Some((f, true));
    }

    let user_dir = crate::paths::user_ways_root().join(way_id);
    if let Some(f) = find_way_in_dir(&user_dir) {
        return Some((f, false));
    }

    let global_dir = crate::paths::projected_ways_root().join(way_id);
    if let Some(f) = find_way_in_dir(&global_dir) {
        return Some((f, false));
    }

    None
}

/// Resolve a way ID to its check file path. Precedence: project > user > core.
pub fn resolve_check_file(way_id: &str, project_dir: &str) -> Option<(PathBuf, bool)> {
    let local_dir = PathBuf::from(project_dir).join(format!(".claude/ways/{way_id}"));
    if let Some(f) = find_check_in_dir(&local_dir) {
        return Some((f, true));
    }

    let user_dir = crate::paths::user_ways_root().join(way_id);
    if let Some(f) = find_check_in_dir(&user_dir) {
        return Some((f, false));
    }

    let global_dir = crate::paths::projected_ways_root().join(way_id);
    if let Some(f) = find_check_in_dir(&global_dir) {
        return Some((f, false));
    }

    None
}

fn find_way_in_dir(dir: &Path) -> Option<PathBuf> {
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
pub fn list_way_epochs(session_id: &str) -> Vec<(String, u64)> {
    let epochs_dir = session_dir(session_id).join("way-epochs");
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
            assert_eq!(token_position_in(&claude, None, project, sid), 42000, "{project}");
        }
        std::fs::remove_dir_all(&root).ok();
    }
}

#[cfg(test)]
mod compaction_tests {
    use super::*;

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
        compact_log_tail(&path, 2000).unwrap();

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
    fn compact_log_tail_noop_when_under_keep() {
        let path = std::env::temp_dir().join(format!("ways-evt-small-{}.jsonl", std::process::id()));
        let content = "{\"n\":1}\n{\"n\":2}\n";
        std::fs::write(&path, content).unwrap();
        compact_log_tail(&path, 1024 * 1024).unwrap();
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
        compact_log_tail(&path, 1000).unwrap();
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
        compact_log_tail(&path, 1500).unwrap();
        let once = std::fs::read_to_string(&path).unwrap();
        compact_log_tail(&path, 1500).unwrap();
        let twice = std::fs::read_to_string(&path).unwrap();
        assert_eq!(once, twice, "second compaction is a no-op");
        let _ = std::fs::remove_file(&path);
    }
}
