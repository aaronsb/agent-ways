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
    record_way_fire, way_fire_outcome, way_refire_threshold_k, FireOutcome, REFIRE_FLOOR,
};

// ── Session directory ──────────────────────────────────────────

/// Per-user sessions root.
///
/// Resolution order — **must stay identical to `hooks/ways/sessions-root.sh`**.
/// The binary and the shell hooks both compute this independently; if they
/// disagree they read and write session state in different directories and
/// coordination silently breaks:
///   1. `$XDG_RUNTIME_DIR/claude-sessions`            (Linux/systemd — already per-user)
///   2. Windows: `%LOCALAPPDATA%/claude-ways/sessions`  (per-user; LOCALAPPDATA is
///      exported in both native Windows and Git Bash, so the `.exe` and the shell
///      hooks resolve the identical path)
///   3. `/tmp/.claude-sessions-{uid}`                 (other Unix)
pub fn sessions_root() -> String {
    // 1. XDG_RUNTIME_DIR (already per-user, no UID needed) — wins on any platform.
    if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") {
        return format!("{xdg}/claude-sessions");
    }

    // 2. Windows: per-user LOCALAPPDATA base (matches sessions-root.sh).
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
/// falling back to the system temp dir if it is somehow unset. Centralized so
/// `sessions_root()` and `response_topics_path()` cannot drift on Windows.
#[cfg(windows)]
fn win_user_base() -> String {
    std::env::var("LOCALAPPDATA")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| std::env::temp_dir().to_string_lossy().into_owned())
}

/// Root directory for a session's state.
fn session_dir(session_id: &str) -> PathBuf {
    PathBuf::from(format!("{}/{session_id}", sessions_root()))
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

/// Path to the per-session response-topics state file written by the Stop
/// hook (`check-response.sh`) and consumed on the next turn by
/// `check-prompt.sh` to enrich prompt matching with topics from Claude's
/// last reply. Lives outside `sessions_root()` because it predates that
/// hierarchy. Canonical here so reset, the writer, and the consumer all
/// resolve through one source of truth and cannot drift.
pub fn response_topics_path(session_id: &str) -> PathBuf {
    // The shell hooks never hardcode this — they read it back from
    // `ways response-topics-path`, so only the binary's view must be correct
    // per platform. On Windows, `/tmp` is not a real path for the native `.exe`,
    // so anchor to the per-user LOCALAPPDATA base (which always exists).
    #[cfg(windows)]
    {
        PathBuf::from(format!(
            "{}/claude-response-topics-{session_id}",
            win_user_base()
        ))
    }
    #[cfg(not(windows))]
    {
        PathBuf::from(format!("/tmp/claude-response-topics-{session_id}"))
    }
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

/// Read the scope that fired a way (for display in ways list).
/// Returns (scope, agent_id).
pub fn way_fired_scope(way_id: &str, session_id: &str) -> Option<(String, String)> {
    // Try scoped marker first
    let path = way_marker_path(way_id, session_id);
    if path.exists() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            let parts: Vec<&str> = content.split('\t').collect();
            let scope = parts.get(1).unwrap_or(&"agent").to_string();
            let agent_id = parts.get(2).unwrap_or(&"main").to_string();
            return Some((scope, agent_id));
        }
    }
    // Backward compat: old-style unscoped marker
    let old = session_dir(session_id).join("ways").join(way_id).join(".marker");
    if old.exists() {
        return Some(("agent".to_string(), "main".to_string()));
    }
    None
}

/// List all scopes that fired a way (for display — shows all agents that got it).
pub fn way_fired_scopes(way_id: &str, session_id: &str) -> Vec<(String, String)> {
    let base = session_dir(session_id).join("ways").join(way_id);
    let mut results = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&base) {
        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(".marker") {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    let parts: Vec<&str> = content.split('\t').collect();
                    let scope = parts.get(1).unwrap_or(&"agent").to_string();
                    let agent_id = parts.get(2).unwrap_or(&"main").to_string();
                    results.push((scope, agent_id));
                }
            }
        }
    }
    if results.is_empty() {
        // Backward compat
        if base.join(".marker").exists() {
            results.push(("agent".to_string(), "main".to_string()));
        }
    }
    results
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

// ── Token position (ADR-104 re-disclosure) ──────────────────────

/// Read the token position from the most recent transcript.
pub fn get_token_position(_session_id: &str) -> u64 {
    let project_dir = std::env::var("CLAUDE_PROJECT_DIR")
        .unwrap_or_else(|_| std::env::var("PWD").unwrap_or_else(|_| ".".to_string()));
    let project_slug = project_dir.replace(['/', '.'], "-");
    let conv_dir = home_dir().join(format!(".claude/projects/{project_slug}"));

    let transcript = find_newest_jsonl(&conv_dir);
    let transcript = match transcript {
        Some(t) => t,
        None => return 0,
    };

    let content = match std::fs::read_to_string(&transcript) {
        Ok(c) => c,
        Err(_) => return 0,
    };

    let mut max_tokens: u64 = 0;
    for line in content.lines().rev() {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
            if val.get("type").and_then(|t| t.as_str()) == Some("assistant") {
                if let Some(usage) = val.get("message").and_then(|m| m.get("usage")) {
                    let cache_read = usage["cache_read_input_tokens"].as_u64().unwrap_or(0);
                    let cache_create = usage["cache_creation_input_tokens"].as_u64().unwrap_or(0);
                    let input = usage["input_tokens"].as_u64().unwrap_or(0);
                    let total = cache_read + cache_create + input;
                    if total > max_tokens {
                        max_tokens = total;
                    }
                    break;
                }
            }
        }
    }
    max_tokens
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
    let project_slug = project.replace(['/', '.'], "-");
    let transcript = home_dir()
        .join(format!(".claude/projects/{project_slug}/{session_id}.jsonl"));
    context_window_from_transcript(&transcript)
}

/// Scan a transcript to detect the model, and resolve its context window through
/// the one resolver (ADR-166) — the same call `cmd::context` makes, so the gauge
/// and every fire-evaluation path (show, list, rethink) agree on the window.
///
/// This is load-bearing beyond reporting: ADR-126 scales a way's refire half-life
/// as a fraction of the window, so a wrong answer here rescales the disclosure
/// curve for every way in the session.
fn context_window_from_transcript(transcript: &std::path::Path) -> u64 {
    let content = std::fs::read_to_string(transcript).unwrap_or_default();
    ways_core::context_window::resolve(model_from_transcript(&content).as_deref()).tokens
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
    for line in content.lines().rev() {
        // An unparseable line is skipped, not fatal — transcripts carry many
        // shapes, and only assistant turns name a model.
        let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if val.get("type").and_then(|t| t.as_str()) == Some("assistant") {
            if let Some(model) = val
                .get("message")
                .and_then(|m| m.get("model"))
                .and_then(|m| m.as_str())
                .filter(|m| !ways_core::context_window::is_sentinel(m))
            {
                return Some(model.to_string());
            }
        }
    }
    None
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
        std::fs::write(
            &opus,
            "{\"type\":\"user\"}\n{\"type\":\"assistant\",\"message\":{\"model\":\"claude-opus-4-8\"}}\n",
        )
        .unwrap();
        assert_eq!(context_window_from_transcript(&opus), 1_000_000);

        // The launch race: the monitor can start seconds into a session, before the
        // first assistant turn is written. With no model to read, detection returns
        // the fallback — NOT 1M — which is why the live view must re-detect on refresh
        // rather than cache this startup value for the whole session.
        std::fs::write(&early, "{\"type\":\"user\"}\n").unwrap();
        assert_ne!(
            context_window_from_transcript(&early),
            1_000_000,
            "no assistant turn yet → cannot detect the 1M window"
        );
        // Absent a CLAUDE_CONTEXT_WINDOW override, the fallback is the 200K default.
        if std::env::var("CLAUDE_CONTEXT_WINDOW").is_err() {
            assert_eq!(context_window_from_transcript(&early), 200_000);
        }

        let _ = std::fs::remove_file(&opus);
        let _ = std::fs::remove_file(&early);
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
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let _ = std::fs::write(&path, ts.to_string());
}

pub fn core_is_shown(session_id: &str) -> bool {
    session_dir(session_id).join("core").exists()
}

/// Read the timestamp from the core marker.
pub fn core_marker_ts(session_id: &str) -> Option<u64> {
    let path = session_dir(session_id).join("core");
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

/// Remove the core marker (for re-injection after context clear).
pub fn clear_core(session_id: &str) {
    let path = session_dir(session_id).join("core");
    let _ = std::fs::remove_file(&path);
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
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| {
                use std::io::Write;
                writeln!(f, "{}", line)
            });
    }
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

/// Log an event to the telemetry log ($XDG_STATE/agent-ways/events.jsonl, with a
/// legacy ~/.claude/stats fallback for un-migrated installs — see paths::events_log).
pub fn log_event(fields: &[(&str, &str)]) {
    let events_file = crate::paths::events_log();
    if let Some(stats_dir) = events_file.parent() {
        let _ = std::fs::create_dir_all(stats_dir);
    }

    let ts = chrono_utc_now();
    let mut obj = serde_json::Map::new();
    obj.insert("ts".to_string(), serde_json::Value::String(ts));
    for (k, v) in fields {
        obj.insert(k.to_string(), serde_json::Value::String(v.to_string()));
    }

    if let Ok(line) = serde_json::to_string(&serde_json::Value::Object(obj)) {
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&events_file)
            .and_then(|mut f| {
                use std::io::Write;
                writeln!(f, "{}", line)
            });
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

/// Monotonic suffix so repeated compactions in one process never collide on the
/// temp name (paired with the pid for cross-process uniqueness).
static COMPACT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

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

    // Unique temp: pid (cross-process) + sequence (intra-process) so two
    // concurrent compactions never write the same file and publish a torn tail.
    let seq = COMPACT_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("jsonl.compact.{}.{seq}.tmp", std::process::id()));
    let write_then_rename = || -> std::io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&data[start..])?;
        f.sync_all()?; // durable before publish — no zero/partial file on crash
        std::fs::rename(&tmp, path)
    };
    let res = write_then_rename();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp); // never leave a stray temp behind
    }
    res
}

/// UTC timestamp without chrono dependency.
fn chrono_utc_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days_since_epoch = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;
    let (year, month, day) = crate::util::days_to_ymd(days_since_epoch);
    format!(
        "{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z"
    )
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

    let global_dir = home_dir().join(format!(".claude/hooks/ways/{way_id}"));
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

    let global_dir = home_dir().join(format!(".claude/hooks/ways/{way_id}"));
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
            if crate::util::has_frontmatter(&content) {
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
    let root = PathBuf::from(sessions_root());
    if !root.is_dir() {
        return Vec::new();
    }
    let mut sessions = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&root) {
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

fn find_newest_jsonl(dir: &Path) -> Option<PathBuf> {
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

use crate::util::home_dir;

// Tests for the ADR-123 engagement cluster (classify_outcome,
// load_engagement_for_tick, FirstFire → ReFire → Suppressed) live in
// `session::engagement`'s own test module — they moved with the code
// they cover (issue #52).

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
