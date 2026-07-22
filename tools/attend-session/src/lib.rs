//! Canonical session self-identity for the attend mesh (issue #378).
//!
//! One derivation of "who am I on the bus", consumed by everything
//! that needs a stable key: the instance registry, the heartbeat,
//! focus-group member ids, signal wire identity, and (planned) the
//! per-session consumption checkpoint. The invariant this crate
//! guards:
//!
//! > A claude's identity is `(sessionId ∩ origin_path)` — the session
//! > UID from Claude Code's session record, paired with the *session
//! > record's* cwd, never the process cwd.
//!
//! Why not process cwd: a shell `cd` that leaks into an `attend run`
//! launch (or any subcommand) would otherwise put the session on the
//! bus as a different persona than its project — the "multiple
//! personalities" failure #378 documents. The session record is the
//! stable half of the tuple; process cwd is only ever a fallback for
//! processes that genuinely have no Claude session (a human's shell).
//!
//! ## Resolution
//!
//! 1. Walk `~/.claude/sessions/*.json` into a pid → sessionId map.
//! 2. Climb our own pid's ancestry (≤15 hops) until a mapped pid is
//!    found — that session is ours.
//! 3. Read that session record's `cwd` as the origin path.
//!
//! Fallbacks are explicit and flagged (`resolved: false`): no session
//! record → `pid-<pid>` + process cwd. Callers that require a real
//! session (registry, groups) can branch on `resolved`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The canonical identity tuple. `session_id` and `origin_path` are
/// the stable key downstream state must use; display naming (nickname
/// + Greek ordinal) is presentation layered on top and never a key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionIdentity {
    /// Claude Code session UID, or `pid-<pid>` when unresolved.
    pub session_id: String,
    /// The session record's cwd, or the process cwd when unresolved.
    pub origin_path: String,
    /// True iff `session_id` came from a real session record.
    pub session_resolved: bool,
    /// True iff `origin_path` came from that record's `cwd` field.
    /// Distinct from `session_resolved` so "no session at all" and
    /// "session record without a cwd" stay distinguishable.
    pub origin_resolved: bool,
}

impl SessionIdentity {
    /// Fully resolved: both halves of the tuple came from a session
    /// record. Callers that gate behavior (instance registration,
    /// whoami's fallback warning) branch on this.
    pub fn resolved(&self) -> bool {
        self.session_resolved && self.origin_resolved
    }
}

/// Resolve the identity of the current process. Memoized for the
/// process lifetime — the tuple is stable by definition (a process
/// cannot change its owning session), and one-shot commands like
/// `attend send` would otherwise repeat the sessions-dir walk and
/// `ps` ancestry climb several times per invocation.
pub fn identity() -> SessionIdentity {
    use std::sync::OnceLock;
    static IDENT: OnceLock<SessionIdentity> = OnceLock::new();
    IDENT
        .get_or_init(|| identity_for_pid(std::process::id()))
        .clone()
}

/// Resolve the identity of an arbitrary pid (test seam + tooling).
pub fn identity_for_pid(pid: u32) -> SessionIdentity {
    identity_in(&sessions_dir(), pid)
}

/// Core resolution against an arbitrary sessions directory, so tests
/// can drive it without touching `$HOME`. The ancestry walk still
/// uses the real process table — tests pass their own pid with a
/// session record naming it directly.
pub fn identity_in(dir: &Path, pid: u32) -> SessionIdentity {
    let process_cwd = || {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default()
    };
    match find_session_id_in(dir, pid) {
        Some(sid) => {
            let origin = origin_path_in(dir, &sid);
            let origin_resolved = origin.is_some();
            SessionIdentity {
                origin_path: origin.unwrap_or_else(process_cwd),
                session_id: sid,
                session_resolved: true,
                origin_resolved,
            }
        }
        None => SessionIdentity {
            session_id: format!("pid-{pid}"),
            origin_path: process_cwd(),
            session_resolved: false,
            origin_resolved: false,
        },
    }
}

/// Is `ancestor` in `pid`'s process ancestry (inclusive)? The
/// canonical home for ancestry checks — sensor-peers' own-session
/// detection delegates here so hop limits and parent-pid resolution
/// cannot drift between crates.
pub fn pid_has_ancestor(pid: u32, ancestor: u32) -> bool {
    let mut cur = pid;
    for _ in 0..15 {
        if cur == ancestor {
            return true;
        }
        if cur <= 1 {
            break;
        }
        match get_parent_pid(cur) {
            Some(ppid) if ppid != cur => cur = ppid,
            _ => break,
        }
    }
    false
}

/// Find the Claude Code session owning `own_pid` by climbing its
/// process ancestry against the session records' pids.
pub fn find_own_session_id(own_pid: u32) -> Option<String> {
    find_session_id_in(&sessions_dir(), own_pid)
}

/// Test-seam counterpart to [`find_own_session_id`].
pub fn find_session_id_in(dir: &Path, own_pid: u32) -> Option<String> {
    let mut pid_to_session: HashMap<u32, String> = HashMap::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Ok(content) = fs::read_to_string(entry.path()) {
                if let (Some(pid), Some(sid)) = (
                    extract_json_u64(&content, "pid"),
                    extract_json_string(&content, "sessionId"),
                ) {
                    pid_to_session.insert(pid as u32, sid);
                }
            }
        }
    }
    if pid_to_session.is_empty() {
        return None;
    }

    let mut pid = own_pid;
    for _ in 0..15 {
        if let Some(sid) = pid_to_session.get(&pid) {
            return Some(sid.clone());
        }
        if pid <= 1 {
            break;
        }
        match get_parent_pid(pid) {
            Some(ppid) if ppid != pid => pid = ppid,
            _ => break,
        }
    }
    None
}

/// The origin path recorded for `session_id`, from its session record.
pub fn origin_path(session_id: &str) -> Option<String> {
    origin_path_in(&sessions_dir(), session_id)
}

/// Test-seam counterpart to [`origin_path`].
pub fn origin_path_in(dir: &Path, session_id: &str) -> Option<String> {
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let Ok(content) = fs::read_to_string(entry.path()) else {
            continue;
        };
        if extract_json_string(&content, "sessionId").as_deref() == Some(session_id) {
            return extract_json_string(&content, "cwd");
        }
    }
    None
}

fn sessions_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".claude").join("sessions")
}

/// Return the parent PID of `pid`, or `None` if it cannot be
/// determined. Byte-compatible with the sensor-peers implementation
/// this crate canonicalizes.
#[cfg(not(windows))]
fn get_parent_pid(pid: u32) -> Option<u32> {
    let output = Command::new("ps")
        .args(["--no-headers", "-p", &pid.to_string(), "-o", "ppid"])
        .output()
        .ok()?;
    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout);
        s.trim().parse::<u32>().ok().filter(|&p| p > 0)
    } else {
        None
    }
}

#[cfg(windows)]
fn get_parent_pid(pid: u32) -> Option<u32> {
    let script = format!(
        "(Get-CimInstance Win32_Process -Filter 'ProcessId={}').ParentProcessId",
        pid
    );
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .ok()?;
    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout);
        s.trim().parse::<u32>().ok().filter(|&p| p > 0)
    } else {
        None
    }
}

/// Minimal JSON field extractor — the session file schema is flat and
/// stable, so a parser dependency would be pure ceremony. Unlike the
/// historical copies this canonicalizes, it tolerates whitespace after
/// the colon (`"key": "value"`): as the now-single point of failure
/// for identity, it must not silently break if Claude Code ever
/// pretty-prints the record.
fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{}\":", key);
    let start = json.find(&pattern)? + pattern.len();
    let rest = json[start..].trim_start().strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn extract_json_u64(json: &str, key: &str) -> Option<u64> {
    let pattern = format!("\"{}\":", key);
    let start = json.find(&pattern)? + pattern.len();
    let rest = json[start..].trim_start();
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tempdir_like() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "attend-session-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn write_session(dir: &Path, sid: &str, pid: u32, cwd: &str) {
        let body = format!(r#"{{"sessionId":"{sid}","cwd":"{cwd}","pid":{pid},"model":"x"}}"#);
        let mut f = fs::File::create(dir.join(format!("{sid}.json"))).unwrap();
        f.write_all(body.as_bytes()).unwrap();
    }

    #[test]
    fn identity_resolves_own_pid_via_direct_record() {
        // Our own test process pid mapped directly — no ancestry hops
        // needed, so the walk terminates on the first lookup.
        let dir = tempdir_like();
        let pid = std::process::id();
        write_session(&dir, "sess-me", pid, "/home/me/proj");
        let id = identity_in(&dir, pid);
        assert!(id.resolved());
        assert!(id.session_resolved && id.origin_resolved);
        assert_eq!(id.session_id, "sess-me");
        assert_eq!(id.origin_path, "/home/me/proj");
    }

    #[cfg(unix)]
    #[test]
    fn identity_resolves_via_ancestry_hop() {
        // The record names our *parent* pid — resolution must climb
        // one ancestry hop to find it, exercising the walk rather
        // than the direct-map shortcut.
        let dir = tempdir_like();
        let parent = std::os::unix::process::parent_id();
        write_session(&dir, "sess-parent", parent, "/via/hop");
        let id = identity_in(&dir, std::process::id());
        assert!(id.resolved());
        assert_eq!(id.session_id, "sess-parent");
        assert_eq!(id.origin_path, "/via/hop");
    }

    #[test]
    fn session_without_cwd_is_partially_resolved() {
        // A record that names us but carries no cwd: the session half
        // resolves, the origin half falls back — and the two flags
        // keep the cases distinguishable.
        let dir = tempdir_like();
        let pid = std::process::id();
        let body = format!(r#"{{"sessionId":"sess-nocwd","pid":{pid}}}"#);
        std::fs::write(dir.join("sess-nocwd.json"), body).unwrap();
        let id = identity_in(&dir, pid);
        assert!(id.session_resolved);
        assert!(!id.origin_resolved);
        assert!(!id.resolved());
        assert_eq!(id.session_id, "sess-nocwd");
    }

    #[cfg(unix)]
    #[test]
    fn pid_has_ancestor_finds_parent_and_self() {
        let me = std::process::id();
        assert!(pid_has_ancestor(me, me));
        assert!(pid_has_ancestor(me, std::os::unix::process::parent_id()));
        // A pid that cannot be in our ancestry (pid 0 never is).
        assert!(!pid_has_ancestor(me, 0));
    }

    #[test]
    fn extractors_tolerate_whitespace_after_colon() {
        let json = r#"{ "sessionId": "sess-x", "pid": 42, "cwd": "/p" }"#;
        assert_eq!(extract_json_string(json, "sessionId").as_deref(), Some("sess-x"));
        assert_eq!(extract_json_string(json, "cwd").as_deref(), Some("/p"));
        assert_eq!(extract_json_u64(json, "pid"), Some(42));
    }

    #[test]
    fn origin_path_comes_from_record_not_process_cwd() {
        // The #378 invariant: even though this test process's cwd is
        // wherever cargo put it, the identity's origin_path is the
        // session record's cwd.
        let dir = tempdir_like();
        let pid = std::process::id();
        write_session(&dir, "sess-me", pid, "/canonical/origin");
        let id = identity_in(&dir, pid);
        let actual_cwd = std::env::current_dir().unwrap().to_string_lossy().to_string();
        assert_eq!(id.origin_path, "/canonical/origin");
        assert_ne!(id.origin_path, actual_cwd);
    }

    #[test]
    fn unresolved_identity_falls_back_flagged() {
        let dir = tempdir_like(); // empty sessions dir
        let id = identity_in(&dir, std::process::id());
        assert!(!id.resolved());
        assert!(!id.session_resolved && !id.origin_resolved);
        assert_eq!(id.session_id, format!("pid-{}", std::process::id()));
        assert!(!id.origin_path.is_empty());
    }

    #[test]
    fn find_session_id_none_when_dir_missing() {
        let dir = tempdir_like().join("nope");
        assert_eq!(find_session_id_in(&dir, std::process::id()), None);
    }

    #[test]
    fn origin_path_matches_by_session_id() {
        let dir = tempdir_like();
        write_session(&dir, "sess-a", 1111, "/proj/a");
        write_session(&dir, "sess-b", 2222, "/proj/b");
        assert_eq!(origin_path_in(&dir, "sess-b").as_deref(), Some("/proj/b"));
        assert_eq!(origin_path_in(&dir, "sess-zz"), None);
    }

    #[test]
    fn extract_json_u64_parses_pid() {
        assert_eq!(extract_json_u64(r#"{"pid":12345,"x":"y"}"#, "pid"), Some(12345));
        assert_eq!(extract_json_u64(r#"{"pid": 99}"#, "pid"), Some(99));
        assert_eq!(extract_json_u64(r#"{"nope":1}"#, "pid"), None);
    }
}
