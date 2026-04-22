//! Session state management — markers, epochs, token positions, scope detection.
//!
//! All session state lives in /tmp/.claude-sessions-{uid}/{session_id}/ as a
//! directory tree. Way IDs map directly to paths (no dash-encoding).
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

/// Per-user sessions root: /tmp/.claude-sessions-{uid}
///
/// Uses XDG_RUNTIME_DIR (per-user on systemd) if available, otherwise
/// falls back to /tmp/.claude-sessions-{uid} using $EUID or `id -u`.
pub fn sessions_root() -> String {
    // Prefer XDG_RUNTIME_DIR (already per-user, no UID needed)
    if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") {
        return format!("{xdg}/claude-sessions");
    }
    // Fall back to /tmp with UID namespace
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

/// Root directory for a session's state.
fn session_dir(session_id: &str) -> PathBuf {
    PathBuf::from(format!("{}/{session_id}", sessions_root()))
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

/// Scan a transcript to detect model and return context window size in tokens.
/// Honors `CLAUDE_CONTEXT_WINDOW` as an override — the same contract
/// `cmd::context::get_context` uses, so window fallbacks stay consistent
/// across all fire-evaluation paths (show, list, rethink).
fn context_window_from_transcript(transcript: &std::path::Path) -> u64 {
    let fallback = || {
        std::env::var("CLAUDE_CONTEXT_WINDOW")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(200_000)
    };

    let content = match std::fs::read_to_string(transcript) {
        Ok(c) => c,
        Err(_) => return fallback(),
    };

    for line in content.lines().rev() {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
            if val.get("type").and_then(|t| t.as_str()) == Some("assistant") {
                if let Some(model) = val.get("message").and_then(|m| m.get("model")).and_then(|m| m.as_str()) {
                    if model.contains("opus-4") {
                        return 1_000_000;
                    }
                }
                break;
            }
        }
    }
    fallback()
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

/// Log an event to ~/.claude/stats/events.jsonl.
pub fn log_event(fields: &[(&str, &str)]) {
    let stats_dir = home_dir().join(".claude/stats");
    let _ = std::fs::create_dir_all(&stats_dir);
    let events_file = stats_dir.join("events.jsonl");

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
    let (year, month, day) = days_to_ymd(days_since_epoch);
    format!(
        "{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z"
    )
}

/// Public wrapper for governance module.
pub fn days_to_ymd_pub(days: u64) -> (u64, u64, u64) {
    days_to_ymd(days)
}

fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

// ── Domain disable check ────────────────────────────────────────

/// Check if a domain is disabled.
/// config::global() — future migration: ctx.config.disabled_domains
pub fn domain_disabled(domain: &str) -> bool {
    crate::config::global().disabled_domains.iter().any(|d| d == domain)
}


// ── Way file resolution ─────────────────────────────────────────

/// Resolve a way ID to its file path. Project-local takes precedence.
/// Returns (path, is_project_local).
pub fn resolve_way_file(way_id: &str, project_dir: &str) -> Option<(PathBuf, bool)> {
    let local_dir = PathBuf::from(project_dir).join(format!(".claude/ways/{way_id}"));
    if let Some(f) = find_way_in_dir(&local_dir) {
        return Some((f, true));
    }

    let global_dir = home_dir().join(format!(".claude/hooks/ways/{way_id}"));
    if let Some(f) = find_way_in_dir(&global_dir) {
        return Some((f, false));
    }

    None
}

/// Resolve a way ID to its check file path.
pub fn resolve_check_file(way_id: &str, project_dir: &str) -> Option<(PathBuf, bool)> {
    let local_dir = PathBuf::from(project_dir).join(format!(".claude/ways/{way_id}"));
    if let Some(f) = find_check_in_dir(&local_dir) {
        return Some((f, true));
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
            if content.starts_with("---\n") {
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
