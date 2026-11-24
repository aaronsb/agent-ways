// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

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
///
/// A process a sensor command started never resolves, whatever records it
/// finds: [`started_by_sensor_command`] is checked first, over the whole
/// ancestry.
pub fn find_session_in_dirs(dirs: &[PathBuf], own_pid: u32, is_claude: impl Fn(u32) -> bool) -> Option<(String, u32)> {
    let mut pid_to_session: HashMap<u32, String> = HashMap::new();
    for dir in dirs {
        for r in claude_sessions::read_session_records(dir) {
            pid_to_session.entry(r.pid).or_insert(r.session_id);
        }
    }
    if pid_to_session.is_empty() || started_by_sensor_command(own_pid) {
        return None;
    }
    walk(own_pid, &pid_to_session, crate::process::parent_pid, is_claude)
}

/// Climb from `own_pid` by `parent`, at most 15 steps, and return the
/// first pid `records` names with its session. Stop with none at a Claude
/// Code process no record names, or at pid 1.
pub fn walk(own_pid: u32, records: &HashMap<u32, String>, parent: impl Fn(u32) -> Option<u32>, is_claude: impl Fn(u32) -> bool) -> Option<(String, u32)> {
    let mut pid = own_pid;
    for _ in 0..15 {
        if let Some(sid) = records.get(&pid) {
            return Some((sid.clone(), pid));
        }
        if is_claude(pid) || pid <= 1 {
            break;
        }
        match parent(pid) {
            Some(ppid) if ppid != pid => pid = ppid,
            _ => break,
        }
    }
    None
}

/// The most ancestors [`host_above`] reads.
pub const HOST_WALK_MAX: usize = 256;

/// Whether an ancestor of `own_pid`, up to pid 1, is one `is_host` names.
/// `own_pid` itself is not counted, so a host is not above itself. The walk
/// does not stop at Claude Code or at a session record, since either can be
/// imitated from below a host; it stops at pid 1, at a pid seen before, or
/// after [`HOST_WALK_MAX`] ancestors.
pub fn host_above(own_pid: u32, parent: impl Fn(u32) -> Option<u32>, is_host: impl Fn(u32) -> bool) -> bool {
    let mut seen = std::collections::HashSet::from([own_pid]);
    let mut pid = own_pid;
    for _ in 0..HOST_WALK_MAX {
        match parent(pid) {
            Some(ppid) if ppid > 0 && seen.insert(ppid) => pid = ppid,
            _ => return false,
        }
        if is_host(pid) {
            return true;
        }
        if pid <= 1 {
            return false;
        }
    }
    false
}

/// `path` with every symlink resolved, or `path` as given when it cannot
/// be resolved, such as a directory that is gone.
pub fn canonical_dir(path: &str) -> String {
    std::fs::canonicalize(path).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| path.to_string())
}

/// Whether two paths name one directory: equal as written, or equal once
/// symlinks are resolved. A session's recorded cwd and a path a caller
/// names can reach one directory by different routes, such as `/var` and
/// `/private/var` on macOS.
pub fn same_dir(a: &str, b: &str) -> bool {
    a == b || canonical_dir(a) == canonical_dir(b)
}

/// Why a process is taken to have been started for a sensor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SensorStart {
    /// An attend sensor host is among its ancestors.
    Host,
    /// Its environment carries the sensor marker `value`, which names the
    /// live attend `pid`.
    Marker { pid: u32, value: String },
}

/// Whether a sensor command started `pid`, and how attend knows: an
/// attend sensor host is among its ancestors, or, for this process, its
/// environment carries the sensor marker of a live attend
/// ([`live_sensor_marker`]). No legitimate caller has either, since the
/// session's own processes sit beside a host, not under one. Such a
/// process does not act as the session.
///
/// For this process the ancestry is read once and kept, as no new
/// ancestor can appear. The marker is read on every call, so a
/// long-running process stops refusing once the attend that set its
/// marker exits.
///
/// The two checks cover each other. A command that detaches, as `( … & )`
/// or `setsid -f` does, leaves the host's ancestry and keeps the marker;
/// a command that clears its environment and stays attached keeps the
/// ancestry. Neither covers a command that asks a process already running
/// outside the host to start attend for it: a service manager
/// (`systemd-run --user`, `launchctl`), a terminal multiplexer's server
/// that was already up (`tmux new-window`), `at` or cron, `ssh`, or
/// `sudo`, which resets the environment. Nor do they cover a command that
/// unsets the marker and detaches. The guard prevents accidents, and
/// trusting the sensor file is the control against intent.
///
/// A wrapper that runs `pid` with its arguments and stays its parent, as
/// `timeout 75 attend sensor host …` does, names a host in its arguments
/// without being one, and is not counted ([`launches_argv`]). `env`,
/// `nice`, `nohup` and `stdbuf` exec in place and leave no such parent.
pub fn sensor_start(pid: u32) -> Option<SensorStart> {
    static OWN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let ancestry = || {
        let own = crate::process::argv(pid);
        host_above(pid, crate::process::parent_pid, |p| is_sensor_host(p) && !launches(p, own.as_deref()))
    };
    match pid == std::process::id() {
        true if *OWN.get_or_init(ancestry) => Some(SensorStart::Host),
        true => live_sensor_marker(),
        false => ancestry().then_some(SensorStart::Host),
    }
}

/// Whether a sensor command started `pid` ([`sensor_start`]).
pub fn started_by_sensor_command(pid: u32) -> bool {
    sensor_start(pid).is_some()
}

/// The sensor marker: the variable attend sets in the environment of the
/// processes it starts for a sensor, naming itself ([`marker_value`]).
pub const SENSOR_HOST_VAR: &str = "ATTEND_SENSOR_HOST";

/// The marker naming `pid`: `<pid>:<start time>`, the start time as
/// [`crate::process::start_stamp`] reads it, so a later process that takes
/// the same pid does not match. Just `<pid>` where the start time cannot
/// be read.
pub fn marker_value(pid: u32) -> String {
    match crate::process::start_stamp(pid) {
        Some(start) => format!("{pid}:{start}"),
        None => pid.to_string(),
    }
}

/// Set the sensor marker on `cmd`, naming this process. attend marks every
/// process that runs a command a sensor file or a repository can choose: a
/// `command` sensor's child, an `mcp` sensor's server, and the `git` the
/// git sensor runs, which a repository's configuration, such as
/// `core.fsmonitor`, can make run other programs. The `ways` the context
/// and disclosure sensors run is marked too, for uniformity with the git
/// sensor beside them, though `ways context` runs nothing a repository
/// chooses. The processes sensor's `ps` and keepwarm's `ways` are fixed
/// programs and are not marked. Call it after the environment is cleared or
/// filtered and after the sensor file's own variables are added, so
/// neither removes or replaces it. A `tokio::process::Command` passes its
/// `as_std_mut()`.
pub fn mark_sensor_child(cmd: &mut std::process::Command) -> &mut std::process::Command {
    cmd.env(SENSOR_HOST_VAR, marker_value(std::process::id()))
}

/// The marker in this process's environment, when it names a live attend
/// ([`marker_host`]).
pub fn live_sensor_marker() -> Option<SensorStart> {
    let value = std::env::var_os(SENSOR_HOST_VAR)?;
    let pid = marker_host(&value, std::process::id(), runs_attend, crate::process::start_stamp)?;
    Some(SensorStart::Marker { pid, value: value.to_string_lossy().into_owned() })
}

/// The pid and start time a marker holds: `<pid>:<start>`, or `<pid>`
/// alone. `None` for any other text.
pub fn parse_marker(value: &str) -> Option<(u32, Option<u64>)> {
    match value.split_once(':') {
        Some((pid, start)) => Some((pid.parse().ok()?, Some(start.parse().ok()?))),
        None => Some((value.parse().ok()?, None)),
    }
}

/// The live attend the marker `value` names: a pid other than 1 and other
/// than `own` that `runs_attend` says runs attend and whose start time,
/// read by `start_of`, is the one the marker holds, when it holds one.
/// Any other marker is stale and names nothing: one naming a process that
/// has exited, a pid since taken by another process, attend included, or
/// a value that is not a marker. A marker that leaks, as into a daemon a
/// sensor command starts and the shells that daemon starts, therefore
/// refuses only while the attend that set it runs.
pub fn marker_host(value: &std::ffi::OsStr, own: u32, runs_attend: impl Fn(u32) -> bool, start_of: impl Fn(u32) -> Option<u64>) -> Option<u32> {
    let (pid, start) = parse_marker(value.to_str()?)?;
    let same_process = start.is_none() || start_of(pid) == start;
    (pid > 1 && pid != own && runs_attend(pid) && same_process).then_some(pid)
}

/// Whether `pid` runs attend: its executable file is named `attend`, or
/// its command line starts a sensor host ([`is_sensor_host`]), as a host
/// run through an interpreter has it. A process that has exited runs
/// nothing.
fn runs_attend(pid: u32) -> bool {
    crate::process::exe_name(pid).is_some_and(|n| is_attend(&n)) || is_sensor_host(pid)
}

/// Whether `pid` runs an attend sensor host: its command line starts one
/// ([`is_sensor_host_argv`]), or its executable file is named `attend` and
/// its arguments are `sensor host` (or a leftover `run`, [`host_args`]),
/// whatever it calls itself.
pub fn is_sensor_host(pid: u32) -> bool {
    let Some(args) = crate::process::argv(pid) else { return false };
    is_sensor_host_argv(&args) || (crate::process::exe_name(pid).is_some_and(|n| is_attend(&n)) && host_args(args.get(1..).unwrap_or_default()))
}

/// Whether `pid` launched the process whose argv is `own` through an argv
/// wrapper ([`launcher`]).
fn launches(pid: u32, own: Option<&[String]>) -> bool {
    launcher(crate::process::exe_name(pid).as_deref(), crate::process::argv(pid).as_deref(), own)
}

/// Whether a process whose executable file is named `exe` and whose argv
/// is `args` launched the process whose argv is `own` through an argv
/// wrapper: it is known not to run attend, and its arguments end with
/// `own`'s ([`launches_argv`]). A host runs attend, so a command a host
/// runs never takes the host for its launcher. An executable that cannot
/// be named, as when the process exited or the platform has no reader,
/// counts as attend, so an unreadable host is still a host.
fn launcher(exe: Option<&str>, args: Option<&[String]>, own: Option<&[String]>) -> bool {
    let (Some(exe), Some(args), Some(own)) = (exe, args, own) else { return false };
    !is_attend(exe) && launches_argv(args, own)
}

/// Whether the argv `args` runs the attend whose argv is `own`: it ends with
/// `own`'s arguments after its program, and the argument just before them
/// names a program `attend` and is not the first, as `timeout 75
/// /bin/attend sensor host …` has it. A launcher's own program comes before
/// the attend it runs, so an argv that starts with attend, as `own` itself
/// does, launches nothing.
pub fn launches_argv(args: &[String], own: &[String]) -> bool {
    let Some(tail) = own.get(1..).filter(|t| !t.is_empty()) else { return false };
    args.len() > tail.len() + 1
        && args.ends_with(tail)
        && is_attend(args[args.len() - tail.len() - 1].rsplit(['/', '\\']).next().unwrap_or_default())
}

/// Whether `args` start an attend sensor host: an argument naming a
/// program `attend`, first or after an interpreter such as `sh`, followed
/// by `sensor host`, or by a leftover `run` ([`host_args`]).
pub fn is_sensor_host_argv(args: &[String]) -> bool {
    args.iter().position(|a| is_attend(a.rsplit(['/', '\\']).next().unwrap_or_default())).is_some_and(|i| host_args(&args[i + 1..]))
}

fn is_attend(name: &str) -> bool {
    name.strip_suffix(".exe").unwrap_or(name) == "attend"
}

/// Whether the arguments after the program name a host: `sensor host`, or
/// `run`. `attend run` is gone (#1316), but an `attend run` an older attend
/// started can still be running when the binary updates, and it predates
/// the sensor marker, so its ancestry is the only thing that keeps a
/// command its sensors run from acting as the session.
fn host_args(rest: &[String]) -> bool {
    // #1316 grace: remove `run` after the next release.
    rest.first().map(String::as_str) == Some("run") || rest.get(..2).is_some_and(|r| r == ["sensor", "host"])
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

/// Test support: the temp directory for fixtures that stand up session
/// projects. It is the system one, or `/tmp` when paths under the system one
/// sit under a managed worktree (a TMPDIR inside an agent's checkout). The
/// strip applies to every session record's cwd, a peer's as much as this
/// session's, so the fixture moves rather than the rule (#902).
#[doc(hidden)]
pub fn fixture_temp_dir() -> PathBuf {
    fixture_temp_dir_for(std::env::temp_dir(), Path::new("/tmp"))
}

/// [`fixture_temp_dir`] for a given temp dir and fallback. A path under
/// `tmp` is checked rather than `tmp` itself, so a TMPDIR of
/// `<root>/.claude/worktrees` with no trailing slash is caught. A fallback
/// that cannot be written to fails here, naming the cause.
fn fixture_temp_dir_for(tmp: PathBuf, fallback: &Path) -> PathBuf {
    if origin_kept(&tmp.join("x")) {
        return tmp;
    }
    // One probe per call: parallel tests each make and remove their own.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let probe = fallback.join(format!(".attend-fixture-probe-{}-{n}", std::process::id()));
    if let Err(e) = std::fs::create_dir(&probe) {
        panic!(
            "the temp dir {} sits under a managed worktree (/.claude/worktrees/), where session identity would resolve \
             fixture projects to the checkout's root, and the fallback {} is not writable: {e} (#902)",
            tmp.display(),
            fallback.display()
        );
    }
    let _ = std::fs::remove_dir(&probe);
    fallback.to_path_buf()
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

    /// A fresh directory, removed with its contents when dropped.
    struct TestDir(PathBuf);

    impl std::ops::Deref for TestDir {
        type Target = PathBuf;
        fn deref(&self) -> &PathBuf {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn tempdir_like() -> TestDir {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "attend-session-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        TestDir(p)
    }

    /// Whether a host is above `own` in a process tree of each pid's
    /// parent.
    fn tree_host(own: u32, parents: &[(u32, u32)], hosts: &[u32]) -> bool {
        let parents: HashMap<u32, u32> = parents.iter().copied().collect();
        host_above(own, |p| parents.get(&p).copied(), |p| hosts.contains(&p))
    }

    /// A process below a sensor host has a host above it, however deep and
    /// whatever sits between; the host itself, the mod's own calls and a
    /// Bash tool shell beside the host do not. Pid 10 is Claude Code.
    #[test]
    fn a_host_anywhere_above_is_found() {
        let mut tree = vec![(10, 1), (20, 10), (21, 20), (30, 10), (31, 30)];
        // Twenty nested shells under the sensor command 21, the deepest 60.
        tree.extend((41..=60).map(|p| (p, if p == 41 { 21 } else { p - 1 })));
        assert!(tree_host(21, &tree, &[20]), "the sensor command");
        assert!(tree_host(60, &tree, &[20]), "twenty shells below it");
        assert!(!tree_host(20, &tree, &[20]), "the host resolves its own session, before and after a re-exec");
        assert!(!tree_host(31, &tree, &[20]), "a Bash tool shell beside the host");
        assert!(!tree_host(30, &tree, &[20]), "the mod's own call");
        assert!(!tree_host(60, &tree, &[]), "no host at all");
        assert!(!tree_host(5, &[(5, 6), (6, 5)], &[]), "a cycle ends the walk");
    }

    #[test]
    fn a_sensor_host_is_named_by_its_argv() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(is_sensor_host_argv(&args(&["attend", "sensor", "host", "--session", "s"])));
        assert!(is_sensor_host_argv(&args(&["/home/u/.local/bin/attend", "sensor", "host", "--session", "s"])));
        // #1316 grace: an older attend's leftover run still counts as a host.
        assert!(is_sensor_host_argv(&args(&["attend", "run"])), "a leftover attend run is a host for one release");
        assert!(is_sensor_host_argv(&args(&["/home/u/.local/bin/attend", "run", "--catchup"])));
        assert!(!is_sensor_host_argv(&args(&["attend", "sensor", "poll", "git"])));
        assert!(!is_sensor_host_argv(&args(&["attend", "message", "send", "run"])));
        assert!(!is_sensor_host_argv(&args(&["attend-chat", "run"])));
        assert!(!is_sensor_host_argv(&args(&["sh", "-c", "attend sensor host"])));
        assert!(is_sensor_host_argv(&args(&["sh", "/home/u/bin/attend", "sensor", "host"])), "a host run through an interpreter");
        assert!(!is_sensor_host_argv(&args(&["grep", "attend", "run.log"])));
    }

    /// A wrapper that runs this attend with its arguments unchanged is its
    /// launcher; a program that runs other arguments, or a non-attend
    /// program in that place, is not.
    #[test]
    fn an_argv_wrapper_launches_the_attend_it_names() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let own = args(&["/bin/attend", "sensor", "host", "--session", "s", "--cwd", "/p"]);
        for wrapper in [&["timeout", "75"][..], &["env", "A=1"], &["nice", "-n", "5"], &["nohup"], &["setsid"], &["stdbuf", "-oL"]] {
            let mut w = args(wrapper);
            w.extend(args(&["attend", "sensor", "host", "--session", "s", "--cwd", "/p"]));
            assert!(launches_argv(&w, &own), "{w:?}");
        }
        assert!(!launches_argv(&args(&["timeout", "75", "attend", "sensor", "host", "--session", "other", "--cwd", "/p"]), &own), "other arguments");
        assert!(!launches_argv(&args(&["timeout", "75", "fake", "sensor", "host", "--session", "s", "--cwd", "/p"]), &own), "not attend");
        assert!(!launches_argv(&args(&["sensor", "host", "--session", "s", "--cwd", "/p"]), &own), "nothing before the arguments");
        assert!(!launches_argv(&own, &args(&["/bin/attend"])), "no arguments of its own");
        assert!(!launches_argv(&own, &own), "a host's own argv launches no child with its arguments");
        assert!(!launches_argv(&args(&["attend", "sensor", "host", "--session", "s", "--cwd", "/p"]), &own), "attend first is no wrapper");
        let w = args(&["timeout", "75", "attend", "sensor", "host", "--session", "s", "--cwd", "/p"]);
        assert!(launcher(Some("timeout"), Some(&w), Some(&own)), "a wrapper whose executable is named");
        assert!(!launcher(None, Some(&w), Some(&own)), "an executable that cannot be named counts as attend");
        assert!(!launcher(None, Some(&own), Some(&own)), "a host whose executable cannot be named stays a host");
        assert!(!launcher(Some("attend"), Some(&w), Some(&own)), "attend under another argv[0] is no wrapper");
    }

    #[test]
    fn a_marker_holds_a_pid_and_maybe_a_start_time() {
        assert_eq!(parse_marker("4242:8828"), Some((4242, Some(8828))));
        assert_eq!(parse_marker("4242"), Some((4242, None)));
        for bad in ["", "host", "4242:", ":8828", "4242:x", "-1:5", "4242:8828:1"] {
            assert_eq!(parse_marker(bad), None, "{bad:?}");
        }
    }

    /// A marker names a live attend while the pid it holds runs attend,
    /// that pid is not this process, and its start time is the one the
    /// marker holds. A pid that has exited, runs something else, or was
    /// taken by a later attend with another start time is stale, as are
    /// pid 1 and a value that is not a marker. A marker with no start time
    /// is checked by its pid alone.
    #[test]
    fn a_marker_names_only_the_live_attend_it_was_set_by() {
        let attend = |p: u32| p == 4242;
        let started = |p: u32| (p == 4242).then_some(8828);
        let host = |v: &str, own: u32| marker_host(std::ffi::OsStr::new(v), own, attend, started);
        assert_eq!(host("4242:8828", 100), Some(4242), "the live attend that set it");
        assert_eq!(host("4242", 100), Some(4242), "a marker with no start time");
        assert_eq!(host("4242:9000", 100), None, "the pid now runs a later attend");
        assert_eq!(host("4243:8828", 100), None, "a pid that runs no attend, or none at all");
        assert_eq!(host("4242:8828", 4242), None, "a marker naming this process");
        assert_eq!(marker_host(std::ffi::OsStr::new("1"), 100, |_| true, |_| None), None, "pid 1");
        assert_eq!(host("not-a-pid", 100), None);
    }

    /// A marked command carries the marker naming this process, start
    /// time included where it can be read, over a value the caller set
    /// before.
    #[test]
    fn a_marked_command_names_this_process() {
        let mut cmd = std::process::Command::new("true");
        cmd.env(SENSOR_HOST_VAR, "0");
        mark_sensor_child(&mut cmd);
        let own = std::process::id();
        let set: Vec<_> = cmd.get_envs().filter(|(k, _)| *k == SENSOR_HOST_VAR).map(|(_, v)| v.map(|v| v.to_os_string())).collect();
        assert_eq!(set, [Some(marker_value(own).into())]);
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        assert_eq!(parse_marker(&marker_value(own)), Some((own, crate::process::start_stamp(own))));
    }

    /// This test process runs no attend, so its own pid in a marker is
    /// stale; a child that has exited and been reaped names no process.
    #[test]
    fn a_real_pid_that_runs_no_attend_is_stale() {
        assert!(!runs_attend(std::process::id()), "the test binary is not attend");
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let gone = child.id();
        child.wait().unwrap();
        assert!(!runs_attend(gone), "an exited pid");
    }

    #[test]
    fn find_session_in_returns_the_matched_ancestor_pid() {
        let dir = tempdir_like();
        let parent = crate::process::parent_pid(std::process::id()).expect("a parent pid");
        write_session(&dir, "sess-parent", parent, "/via/hop");
        assert_eq!(find_session_in(&dir, std::process::id()), Some(("sess-parent".to_string(), parent)));
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
        let only_home = identity_in_dirs(std::slice::from_ref(&*home), pid, |_| false);
        assert!(!only_home.resolved(), "~/.claude alone does not know it");
        let id = identity_in_dirs(&[target.clone(), home.clone()], pid, |_| false);
        assert_eq!((id.session_id.as_str(), id.origin_path.as_str()), ("sess-target", "/work/b"));
        write_session(&home, "sess-home", pid, "/work/a");
        assert_eq!(identity_in_dirs(&[target.clone(), home.clone()], pid, |_| false).session_id, "sess-target");
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
        assert_eq!(find_session_in_dirs(std::slice::from_ref(&*home), own, inner), None);
        assert!(!identity_in_dirs(std::slice::from_ref(&*home), own, inner).resolved());
        // The control: with no Claude Code process on the way, the walk
        // climbs to the outer record.
        assert_eq!(find_session_in_dirs(std::slice::from_ref(&*home), own, |_| false), Some(("sess-outer".to_string(), outer)));
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
            assert_eq!(find_session_in_dirs(std::slice::from_ref(&*home), own, is_claude), Some(("sess-outer".to_string(), session)), "{argv:?}");
        }
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
        assert!(origin_kept(&fixture_temp_dir().join("x")));
        let fallback = std::env::temp_dir();
        assert_eq!(fixture_temp_dir_for("/home/a/proj/.claude/worktrees/agent-1/tmp".into(), &fallback), fallback);
        assert_eq!(fixture_temp_dir_for("/var/tmp".into(), &fallback), PathBuf::from("/var/tmp"));
    }

    /// A TMPDIR naming the worktrees folder itself, with no trailing slash:
    /// the path is kept, but every fixture made under it would not be.
    #[test]
    fn a_temp_dir_of_the_worktrees_folder_itself_falls_back() {
        let tmp = PathBuf::from("/home/a/proj/.claude/worktrees");
        assert!(origin_kept(&tmp), "the folder alone has no segment after it");
        let fallback = std::env::temp_dir();
        assert_eq!(fixture_temp_dir_for(tmp, &fallback), fallback);
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[should_panic(expected = "is not writable")]
    fn an_unwritable_fallback_fails_naming_the_cause() {
        fixture_temp_dir_for("/home/a/proj/.claude/worktrees/agent-1/tmp".into(), Path::new("/proc/attend-no-such-dir"));
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
