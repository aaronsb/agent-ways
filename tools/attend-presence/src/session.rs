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
#[cfg(test)]
use std::fs;
use std::path::{Path, PathBuf};

/// The canonical identity tuple. `session_id` and `origin_path` are
/// the stable key downstream state must use; display naming (nickname
/// + Greek ordinal) is presentation layered on top and never a key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionIdentity {
    /// Claude Code session UID, or `pid-<pid>` when unresolved.
    pub session_id: String,
    /// The session's ROOT path — the record's cwd normalized through
    /// [`normalize_origin`] (managed-worktree hops stripped), or the
    /// process cwd when unresolved. One session record = one persona
    /// at one root (issue #394).
    pub origin_path: String,
    /// True iff `session_id` came from a real session record.
    pub session_resolved: bool,
    /// True iff `origin_path` came from that record's `cwd` field.
    /// Distinct from `session_resolved` so "no session at all" and
    /// "session record without a cwd" stay distinguishable.
    pub origin_resolved: bool,
    /// The Claude Code process whose session record matched, when one did.
    pub claude_pid: Option<u32>,
}

impl SessionIdentity {
    /// Fully resolved: both halves of the tuple came from a session
    /// record. Callers that gate behavior (instance registration,
    /// whoami's fallback warning) branch on this.
    pub fn resolved(&self) -> bool {
        self.session_resolved && self.origin_resolved
    }

    /// The key of the Claude Code process this session runs in: its pid and
    /// start time, which together outlive a session-id change (`/clear`)
    /// and are never reused for another process.
    pub fn claude_key(&self) -> Option<String> {
        let pid = self.claude_pid?;
        Some(format!("{pid}-{}", crate::process::start_time(pid)?))
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
    match find_session_in(dir, pid) {
        Some((sid, claude_pid)) => {
            let origin = origin_path_in(dir, &sid);
            let origin_resolved = origin.is_some();
            SessionIdentity {
                origin_path: origin.unwrap_or_else(process_cwd),
                session_id: sid,
                session_resolved: true,
                origin_resolved,
                claude_pid: Some(claude_pid),
            }
        }
        None => SessionIdentity {
            session_id: format!("pid-{pid}"),
            origin_path: process_cwd(),
            session_resolved: false,
            origin_resolved: false,
            claude_pid: None,
        },
    }
}

/// Find the Claude Code session owning `own_pid` by climbing its
/// process ancestry against the session records' pids.
pub fn find_own_session_id(own_pid: u32) -> Option<String> {
    find_session_id_in(&sessions_dir(), own_pid)
}

/// Test-seam counterpart to [`find_own_session_id`].
pub fn find_session_id_in(dir: &Path, own_pid: u32) -> Option<String> {
    find_session_in(dir, own_pid).map(|(sid, _)| sid)
}

/// The session owning `own_pid` and the ancestor pid whose record matched:
/// the Claude Code process itself.
pub fn find_own_session(own_pid: u32) -> Option<(String, u32)> {
    find_session_in(&sessions_dir(), own_pid)
}

/// Test-seam counterpart to [`find_own_session`].
pub fn find_session_in(dir: &Path, own_pid: u32) -> Option<(String, u32)> {
    let pid_to_session: HashMap<u32, String> = claude_sessions::read_session_records(dir)
        .into_iter()
        .map(|r| (r.pid, r.session_id))
        .collect();
    if pid_to_session.is_empty() {
        return None;
    }

    let mut pid = own_pid;
    for _ in 0..15 {
        if let Some(sid) = pid_to_session.get(&pid) {
            return Some((sid.clone(), pid));
        }
        if pid <= 1 {
            break;
        }
        match crate::process::parent_pid(pid) {
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
    claude_sessions::read_session_records(dir)
        .into_iter()
        .find(|r| r.session_id == session_id)
        .and_then(|r| r.cwd)
        .map(|c| normalize_origin(&c))
}

/// A session's ROOT path — the identity anchor (ADR-171, issue #394).
/// The session record's `cwd` field follows the session into managed
/// worktrees (`<root>/.claude/worktrees/<name>`), but a hop must not
/// change the session's persona: the phantom-agent bug was precisely
/// a hop minting sibling roster entries. Strip the managed-worktree
/// suffix; every other cwd passes through unchanged.
pub fn normalize_origin(cwd: &str) -> String {
    match cwd.find("/.claude/worktrees/") {
        // idx == 0 would leave an empty root — a pathological cwd like
        // "/.claude/worktrees/x" is passed through rather than emptied.
        Some(idx) if idx > 0 => cwd[..idx].to_string(),
        _ => cwd.to_string(),
    }
}

fn sessions_dir() -> PathBuf {
    claude_sessions::ClaudeDir::user().sessions_dir()
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

    #[test]
    fn find_session_in_returns_the_matched_ancestor_pid() {
        let dir = tempdir_like();
        let parent = crate::process::parent_pid(std::process::id()).expect("a parent pid");
        write_session(&dir, "sess-parent", parent, "/via/hop");
        assert_eq!(find_session_in(&dir, std::process::id()), Some(("sess-parent".to_string(), parent)));
        fs::remove_dir_all(&dir).ok();
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

    #[test]
    fn pretty_printed_record_resolves() {
        // Records are read with serde_json (claude-sessions), so whitespace
        // and key order do not matter.
        let dir = tempdir_like();
        let pid = std::process::id();
        let body = format!("{{\n  \"cwd\": \"/p\",\n  \"pid\": {pid},\n  \"sessionId\": \"sess-x\"\n}}");
        std::fs::write(dir.join("x.json"), body).unwrap();
        let id = identity_in(&dir, pid);
        assert_eq!(id.session_id, "sess-x");
        assert_eq!(id.origin_path, "/p");
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
}

#[cfg(test)]
mod origin_tests {
    use super::*;

    #[test]
    fn normalize_strips_managed_worktree_suffix() {
        assert_eq!(
            normalize_origin("/home/a/proj/.claude/worktrees/feature-x"),
            "/home/a/proj"
        );
        assert_eq!(
            normalize_origin("/home/a/proj/.claude/worktrees/x/nested/dir"),
            "/home/a/proj"
        );
    }

    #[test]
    fn normalize_passes_ordinary_paths_through() {
        assert_eq!(normalize_origin("/home/a/proj"), "/home/a/proj");
        assert_eq!(normalize_origin("/home/a/.claude"), "/home/a/.claude");
        // Pathological root-level worktree path stays untouched rather
        // than normalizing to the empty string.
        assert_eq!(
            normalize_origin("/.claude/worktrees/x"),
            "/.claude/worktrees/x"
        );
    }

    /// Issue #394 regression: a session record whose cwd sits inside a
    /// managed worktree must resolve to the project ROOT — a hop never
    /// changes the persona.
    #[test]
    fn origin_path_resolves_worktree_cwd_to_root() {
        let dir = std::env::temp_dir().join(format!(
            "attend-session-394-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("1234.json"),
            r#"{"pid":1234,"sessionId":"uuid-1","cwd":"/home/a/proj/.claude/worktrees/adr-x"}"#,
        )
        .unwrap();
        assert_eq!(
            origin_path_in(&dir, "uuid-1").as_deref(),
            Some("/home/a/proj")
        );
        fs::remove_dir_all(&dir).ok();
    }
}
