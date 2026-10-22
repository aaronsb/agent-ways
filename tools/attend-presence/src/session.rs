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
    /// resolved process's own cwd when unresolved (empty when unreadable). One session record = one persona
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

    /// The key of the Claude Code process this session runs in:
    /// `<pid>-<start time>@<host>`. Pid and start time together outlive a
    /// session-id change (`/clear`) and are never reused for another
    /// process; the host scopes them, so a cache shared between hosts
    /// never has one host judge another's pid.
    pub fn claude_key(&self) -> Option<String> {
        let pid = self.claude_pid?;
        Some(format!("{pid}-{}@{}", crate::process::start_time(pid)?, crate::process::host_name()))
    }
}

/// The key of the Claude Code process session `session_id` runs in on this
/// host, from its session record, as [`SessionIdentity::claude_key`] gives
/// it; `None` when no record names it, or its pid is no longer held by the
/// record's process ([`crate::record_process_runs`]) or by a Claude Code
/// process at all. For a caller that acts for another session, such as an
/// add (ADR-404 D1), which records the key as the session's own.
pub fn claude_key_of(session_id: &str) -> Option<String> {
    key_of(session_id, crate::process::is_claude)
}

/// The key of the process session `session_id`'s record names on this
/// host, whatever program it runs; `None` when no record names it or its
/// pid is no longer held by the record's process. A session finds its
/// earlier id by the process its own record names, Claude Code or not
/// ([`SessionIdentity::claude_key`]), so a caller that finds another
/// session's earlier id ([`crate::enrollment::previous_id_in`]) looks it up
/// by this key.
pub fn process_key_of(session_id: &str) -> Option<String> {
    key_of(session_id, |_| true)
}

/// The key of the process a session record naming `session_id` holds, when
/// that process still runs and `runs_claude` holds for its pid.
fn key_of(session_id: &str, runs_claude: impl Fn(u32) -> bool) -> Option<String> {
    sessions_dirs()
        .iter()
        .flat_map(|d| claude_sessions::read_session_records(d))
        .filter(|r| r.session_id == session_id && crate::record_process_runs(r.pid, r.started_at_ms) && runs_claude(r.pid))
        .find_map(|r| Some(format!("{}-{}@{}", r.pid, crate::process::start_time(r.pid)?, crate::process::host_name())))
}

/// Resolve the identity of the current process. Memoized for the
/// process lifetime — the tuple is stable by definition (a process
/// cannot change its owning session), and one-shot commands like
/// `attend message send` would otherwise repeat the sessions-dir walk and
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
    identity_in_dirs(&sessions_dirs(), pid, crate::process::is_claude)
}

/// The identity of `pid` when a Claude Code session record owns it, or
/// `None` without reading anything more. `attend peer status` names each run by
/// this, and a run with no record costs no working-directory lookup.
pub fn session_identity_for_pid(pid: u32) -> Option<SessionIdentity> {
    session_identity_in_dirs(&sessions_dirs(), pid, crate::process::is_claude)
}

/// The working directory of `pid`, the fallback origin. It is `pid`'s
/// own, never the caller's: `attend peer status` resolves other processes, and
/// its own cwd says nothing about theirs. Empty when it cannot be read.
fn process_cwd(pid: u32) -> String {
    if pid == std::process::id() {
        std::env::current_dir().map(|p| p.to_string_lossy().to_string()).unwrap_or_default()
    } else {
        crate::process::cwd(pid).unwrap_or_default()
    }
}

/// Core resolution against an arbitrary sessions directory, so tests
/// can drive it without touching `$HOME`. The ancestry walk still
/// uses the real process table — tests pass their own pid with a
/// session record naming it directly.
pub fn identity_in(dir: &Path, pid: u32) -> SessionIdentity {
    identity_in_dirs(&[dir.to_path_buf()], pid, |_| false)
}

/// [`identity_in`] over several sessions directories, the first naming a
/// pid winning, with the ancestry walk stopping at a Claude Code process
/// `is_claude` recognizes (see [`find_session_in_dirs`]).
pub fn identity_in_dirs(dirs: &[PathBuf], pid: u32, is_claude: impl Fn(u32) -> bool) -> SessionIdentity {
    session_identity_in_dirs(dirs, pid, is_claude).unwrap_or_else(|| SessionIdentity {
        session_id: format!("pid-{pid}"),
        origin_path: process_cwd(pid),
        session_resolved: false,
        origin_resolved: false,
        claude_pid: None,
    })
}

/// [`session_identity_for_pid`] against arbitrary sessions directories.
/// The process's cwd is read only when the record has none.
fn session_identity_in_dirs(dirs: &[PathBuf], pid: u32, is_claude: impl Fn(u32) -> bool) -> Option<SessionIdentity> {
    let (sid, claude_pid) = find_session_in_dirs(dirs, pid, is_claude)?;
    let origin = dirs.iter().find_map(|d| origin_path_in(d, &sid));
    let origin_resolved = origin.is_some();
    Some(SessionIdentity {
        origin_path: origin.unwrap_or_else(|| process_cwd(pid)),
        session_id: sid,
        session_resolved: true,
        origin_resolved,
        claude_pid: Some(claude_pid),
    })
}

/// Find the Claude Code session owning `own_pid` by climbing its
/// process ancestry against the session records' pids.
pub fn find_own_session_id(own_pid: u32) -> Option<String> {
    find_session_in_dirs(&sessions_dirs(), own_pid, crate::process::is_claude).map(|(sid, _)| sid)
}

/// Test-seam counterpart to [`find_own_session_id`].
pub fn find_session_id_in(dir: &Path, own_pid: u32) -> Option<String> {
    find_session_in(dir, own_pid).map(|(sid, _)| sid)
}

/// The session owning `own_pid` and the ancestor pid whose record matched:
/// the Claude Code process itself.
pub fn find_own_session(own_pid: u32) -> Option<(String, u32)> {
    find_session_in_dirs(&sessions_dirs(), own_pid, crate::process::is_claude)
}

/// Test-seam counterpart to [`find_own_session`], over one directory and
/// with no Claude Code process recognized on the way.
pub fn find_session_in(dir: &Path, own_pid: u32) -> Option<(String, u32)> {
    find_session_in_dirs(&[dir.to_path_buf()], own_pid, |_| false)
}

/// The session owning `own_pid`: climb its ancestry and take the first pid
/// a record in `dirs` names (an earlier directory wins for one pid).
///
/// The walk stops at the first Claude Code process, `is_claude`, whether or
/// not a record names it. That process is the session `own_pid` runs in;
/// when no record names it (its config directory is one this process cannot
/// see), the session is unresolved rather than taken for an outer session
/// further up, which would send as that session and read its messages.
pub fn find_session_in_dirs(dirs: &[PathBuf], own_pid: u32, is_claude: impl Fn(u32) -> bool) -> Option<(String, u32)> {
    let mut pid_to_session: HashMap<u32, String> = HashMap::new();
    for dir in dirs {
        for r in claude_sessions::read_session_records(dir) {
            pid_to_session.entry(r.pid).or_insert(r.session_id);
        }
    }
    if pid_to_session.is_empty() {
        return None;
    }

    let mut pid = own_pid;
    for _ in 0..15 {
        if let Some(sid) = pid_to_session.get(&pid) {
            return Some((sid.clone(), pid));
        }
        if is_claude(pid) || pid <= 1 {
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
    sessions_dirs().iter().find_map(|d| origin_path_in(d, session_id))
}

/// Test-seam counterpart to [`origin_path`].
pub fn origin_path_in(dir: &Path, session_id: &str) -> Option<String> {
    claude_sessions::read_session_records(dir)
        .into_iter()
        .find(|r| r.session_id == session_id)
        .and_then(|r| r.cwd)
        .map(|c| normalize_origin(&c))
}

/// When `session_id`'s Claude Code process started, from its session
/// record's `startedAt`; `None` when no record names it or the record has no
/// start time. Mail addressed to a project before this was addressed to the
/// sessions live there then, not to this one (ADR-401, ADR-404 D3b).
pub fn started_at(session_id: &str) -> Option<std::time::SystemTime> {
    sessions_dirs().iter().find_map(|d| started_at_in(d, session_id))
}

/// Test-seam counterpart to [`started_at`].
pub fn started_at_in(dir: &Path, session_id: &str) -> Option<std::time::SystemTime> {
    claude_sessions::read_session_records(dir)
        .into_iter()
        .filter(|r| r.session_id == session_id)
        .find_map(|r| r.started_at_ms)
        .map(|ms| std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms))
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

/// Whether [`normalize_origin`] leaves `path` as it is: no managed-worktree
/// segment in it. A fixture that stands up session projects checks its roots
/// with this, since a project under a worktree resolves to the checkout's
/// root, and every such project to the same one (#902).
pub fn origin_kept(path: &Path) -> bool {
    let s = path.to_string_lossy();
    normalize_origin(&s) == s
}

/// The temp directory for fixtures that stand up session projects: the
/// system one, or `/tmp` when the system one sits under a managed worktree
/// (a TMPDIR inside an agent's checkout). The strip applies to every session
/// record's cwd, a peer's as much as this session's, so the fixture moves
/// rather than the rule (#902).
pub fn fixture_temp_dir() -> PathBuf {
    let tmp = std::env::temp_dir();
    if origin_kept(&tmp) {
        tmp
    } else {
        PathBuf::from("/tmp")
    }
}

/// The sessions directories a session record can be in: the config
/// directory this process runs under (`CLAUDE_CONFIG_DIR`, which Claude Code
/// passes to what it runs), then `~/.claude`.
pub fn sessions_dirs() -> Vec<PathBuf> {
    sessions_dirs_for(std::env::var_os("CLAUDE_CONFIG_DIR").as_deref().map(Path::new), &claude_sessions::home_dir())
}

/// [`sessions_dirs`] for a given `CLAUDE_CONFIG_DIR` and home.
pub fn sessions_dirs_for(config_dir: Option<&Path>, home: &Path) -> Vec<PathBuf> {
    let default = claude_sessions::ClaudeDir::at(home.join(".claude")).sessions_dir();
    let mut out = Vec::new();
    if let Some(d) = config_dir.filter(|d| !d.as_os_str().is_empty()) {
        out.push(claude_sessions::ClaudeDir::at(d).sessions_dir());
    }
    if !out.contains(&default) {
        out.push(default);
    }
    out
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

    #[test]
    fn the_config_dir_is_read_before_home() {
        let home = Path::new("/home/u");
        assert_eq!(sessions_dirs_for(None, home), vec![PathBuf::from("/home/u/.claude/sessions")]);
        assert_eq!(
            sessions_dirs_for(Some(Path::new("/t/claude-b")), home),
            vec![PathBuf::from("/t/claude-b/sessions"), PathBuf::from("/home/u/.claude/sessions")]
        );
        assert_eq!(sessions_dirs_for(Some(Path::new("/home/u/.claude")), home), vec![PathBuf::from("/home/u/.claude/sessions")]);
        assert_eq!(sessions_dirs_for(Some(Path::new("")), home), vec![PathBuf::from("/home/u/.claude/sessions")]);
    }

    /// A session under a non-default config directory resolves to itself,
    /// and its record wins over one in `~/.claude` naming the same pid.
    #[test]
    fn a_session_under_a_non_default_config_dir_resolves_to_itself() {
        let (target, home) = (tempdir_like(), tempdir_like());
        let pid = std::process::id();
        write_session(&target, "sess-target", pid, "/work/b");
        let only_home = identity_in_dirs(std::slice::from_ref(&home), pid, |_| false);
        assert!(!only_home.resolved(), "~/.claude alone does not know it");
        let id = identity_in_dirs(&[target.clone(), home.clone()], pid, |_| false);
        assert_eq!((id.session_id.as_str(), id.origin_path.as_str()), ("sess-target", "/work/b"));
        write_session(&home, "sess-home", pid, "/work/a");
        assert_eq!(identity_in_dirs(&[target.clone(), home.clone()], pid, |_| false).session_id, "sess-target");
        fs::remove_dir_all(&target).ok();
        fs::remove_dir_all(&home).ok();
    }

    /// A session under a non-default config directory, started from inside
    /// a default-target session: the inner Claude Code process has no record
    /// this process can see, and the walk stops there instead of taking the
    /// outer session's identity.
    #[cfg(unix)]
    #[test]
    fn a_nested_session_does_not_take_the_outer_identity() {
        let home = tempdir_like();
        let own = std::process::id();
        let outer = std::os::unix::process::parent_id();
        write_session(&home, "sess-outer", outer, "/work/outer");
        // This process stands for the inner Claude Code process.
        let inner = |pid: u32| pid == own;
        assert_eq!(find_session_in_dirs(std::slice::from_ref(&home), own, inner), None);
        assert!(!identity_in_dirs(std::slice::from_ref(&home), own, inner).resolved());
        // The control: with no Claude Code process on the way, the walk
        // climbs to the outer record.
        assert_eq!(find_session_in_dirs(std::slice::from_ref(&home), own, |_| false), Some(("sess-outer".to_string(), outer)));
        fs::remove_dir_all(&home).ok();
    }

    /// A node or bun hook under a `.claude/hooks/` folder that runs attend is
    /// not the Claude Code program: the walk climbs past it to the session.
    #[cfg(unix)]
    #[test]
    fn a_node_hook_under_a_claude_folder_resolves_through_to_the_session() {
        let home = tempdir_like();
        let own = std::process::id();
        let session = std::os::unix::process::parent_id();
        write_session(&home, "sess-outer", session, "/work/proj");
        for hook in [["node", "/home/u/.claude/hooks/hook.js"], ["bun", "/home/u/proj/.claude/statusline.ts"]] {
            let argv: Vec<String> = hook.iter().map(|s| s.to_string()).collect();
            // This process stands for the hook; its parent for the session.
            let is_claude = |pid: u32| pid == own && crate::process::is_claude_argv(&argv);
            assert_eq!(find_session_in_dirs(std::slice::from_ref(&home), own, is_claude), Some(("sess-outer".to_string(), session)), "{argv:?}");
        }
        fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn claude_code_is_told_from_what_mentions_it() {
        let v = |args: &[&str]| crate::process::is_claude_argv(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        // Claude Code itself.
        assert!(v(&["claude", "-p", "hi"]));
        assert!(v(&["/home/u/.local/share/claude/versions/2.1.296"]));
        assert!(v(&["node", "/usr/bin/claude"]), "npm-global");
        assert!(v(&["node", "/home/u/.npm-global/bin/claude", "--resume"]), "npm-global under the home");
        assert!(v(&["node", "--no-warnings", "/usr/lib/node_modules/@anthropic-ai/claude-code/cli.js"]));
        assert!(v(&["node", "/app/node_modules/@anthropic-ai/claude-agent-sdk/cli.js"]), "the Agent SDK's CLI");
        assert!(v(&["C:\\Users\\u\\AppData\\Roaming\\npm\\claude.exe"]));
        // What only mentions it.
        assert!(!v(&["node", "/home/u/.claude/hooks/hook.js"]));
        assert!(!v(&["bun", "/home/u/proj/.claude/statusline.ts"]));
        assert!(!v(&["node", "/app/node_modules/@anthropic-ai/claude-agent-sdk/index.js"]), "an SDK host, not its CLI");
        assert!(!v(&["bash", "-c", "source /home/u/.claude/shell-snapshots/s.sh && attend"]));
        assert!(!v(&["timeout", "240", "claude", "-p"]));
        assert!(!v(&["/Applications/Claude.app/Contents/MacOS/Claude"]), "the desktop app");
        assert!(!v(&[]));
        // A path under a `.claude/` folder, whatever the program.
        assert!(!v(&["/usr/bin/bash", "/home/u/.claude/hooks/x.sh"]), "a hook");
        assert!(!v(&["/home/u/.claude/bin/ways-mcp"]), "a binary installed there");
        assert!(
            !v(&["/home/u/proj/.claude/worktrees/agent-1/tools/target/debug/deps/attend_presence-0123abcd", "--test-threads", "4"]),
            "a test binary in a worktree there"
        );
        assert!(!v(&["node", "/home/u/proj/.claude/worktrees/agent-1/scripts/run.js"]), "a script in a worktree there");
        assert!(!v(&["sh", "-c", "sleep 60", "claude"]), "a shell whose $0 is claude");
    }

    /// This process's own command line, read live: a test binary, wherever
    /// it lives (a worktree under `.claude/` included), is no Claude Code
    /// process.
    #[test]
    fn this_test_binary_is_not_claude_code() {
        let pid = std::process::id();
        assert!(!crate::process::is_claude(pid), "{:?}", crate::process::argv(pid));
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

    /// A record without a cwd for another process: the origin is that
    /// process's own working directory, never the resolver's (`attend
    /// status` resolving a peer's run).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_foreign_pid_without_a_record_cwd_takes_its_own_cwd() {
        let dir = tempdir_like();
        let work = dir.join("elsewhere");
        fs::create_dir_all(&work).unwrap();
        let mut child = std::process::Command::new("sleep").arg("5").current_dir(&work).spawn().unwrap();
        let pid = child.id();
        std::fs::write(dir.join("sess-peer.json"), format!(r#"{{"sessionId":"sess-peer","pid":{pid}}}"#)).unwrap();
        let id = identity_in(&dir, pid);
        child.kill().ok();
        child.wait().ok();
        assert!(id.session_resolved && !id.origin_resolved);
        assert_eq!(id.origin_path, work.canonicalize().unwrap().to_string_lossy());
        assert_ne!(id.origin_path, std::env::current_dir().unwrap().to_string_lossy());
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

    #[test]
    fn a_fixture_temp_dir_is_never_under_a_managed_worktree() {
        assert!(!origin_kept(Path::new("/home/a/proj/.claude/worktrees/agent-1/tmp")));
        assert!(origin_kept(Path::new("/tmp")));
        assert!(origin_kept(&fixture_temp_dir()));
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
