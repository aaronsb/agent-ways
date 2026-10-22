//! The running Claude Code sessions and the engine each judges with
//! (ADR-708 §7, §8): what `ways status`, `ways agent status` and the gate
//! screen of `ways settings` list.
//!
//! A session is live when a Claude Code session record names it and the
//! record's process is running: its pid answers, and where the process's
//! start time can be read, it is the one the record names. Each session's
//! engine is the resolution it holds, or with none held, the one the gate
//! would reach now, read through [`engine::read_gate`] as the gate reads it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use claude_sessions::{read_session_records, ClaudeDir, SessionRecord};

use crate::engine::{self, GateReading, SessionEngine, HOST_FILE, OFF};
use crate::profile::{self, Profile, Provider};

/// A running Claude Code session and its engine.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveSession {
    pub session: String,
    /// The held resolution; under `gate.mode: off`, [`OFF`] whatever is
    /// held; with none held, the gate's resolution now when it names no
    /// engine. `None` while the session would resolve to an engine and has
    /// made no judge request.
    pub engine: Option<SessionEngine>,
    /// Whether the session holds a resolution in its judge state.
    pub held: bool,
}

impl LiveSession {
    /// The session id as the screens show it: its first eight characters.
    pub fn short_id(&self) -> String {
        self.session.chars().take(8).collect()
    }

    /// True when the gate judges nothing for this session: `gate.mode: off`.
    pub fn gate_off(&self) -> bool {
        self.engine.as_ref().is_some_and(|e| e.resolution.engine.is_none() && e.resolution.reason == OFF)
    }
}

/// The sessions directories a session record can be in, as attend finds
/// them: the config directory this process runs under (`CLAUDE_CONFIG_DIR`),
/// then `~/.claude`.
pub fn sessions_dirs() -> Vec<PathBuf> {
    sessions_dirs_for(std::env::var_os("CLAUDE_CONFIG_DIR").as_deref().map(Path::new), &claude_sessions::home_dir())
}

/// [`sessions_dirs`] for a given `CLAUDE_CONFIG_DIR` and home.
pub fn sessions_dirs_for(config_dir: Option<&Path>, home: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(d) = config_dir.filter(|d| !d.as_os_str().is_empty()) {
        out.push(ClaudeDir::at(d).sessions_dir());
    }
    push_new(&mut out, ClaudeDir::at(home.join(".claude")).sessions_dir());
    out
}

/// Adds `dir` to `dirs` unless it is there already.
pub fn push_new(dirs: &mut Vec<PathBuf>, dir: PathBuf) {
    if !dirs.contains(&dir) {
        dirs.push(dir);
    }
}

/// The live sessions whose records are in `dirs`, each with its engine,
/// read from agent.yaml, the key files and each session's judge state.
pub fn live_sessions(dirs: &[PathBuf]) -> Vec<LiveSession> {
    let has_key = |p| crate::keys::locate_file(p).is_some();
    live_sessions_in(dirs, &engine::judge_state_root(), &profile::user_layer_path(), has_key)
}

/// [`live_sessions`] with the judge state root and agent.yaml given. Each
/// session is listed once, in id order, so a list reads the same however
/// its records were found; an id that is not a plain directory name is left
/// out.
pub fn live_sessions_in(dirs: &[PathBuf], root: &Path, layer: &Path, has_key: impl Fn(Provider) -> bool) -> Vec<LiveSession> {
    let mut ids = BTreeSet::new();
    let mut seen = Vec::new();
    for dir in dirs {
        if seen.contains(dir) {
            continue;
        }
        seen.push(dir.clone());
        ids.extend(read_session_records(dir).into_iter().filter(record_alive).map(|r| r.session_id).filter(|id| engine::is_plain_session_id(id)));
    }
    let reading = engine::read_gate(layer, &has_key, false);
    ids.into_iter()
        .map(|session| {
            let dir = root.join(&session);
            let held = SessionEngine::read(&dir);
            let is_held = held.is_some();
            let engine = match (&reading, held) {
                // Off judges nothing, whatever a session holds.
                (Ok(GateReading::Off), _) => Some(engine::resolve_reading(&reading, false, &has_key)),
                (_, Some(h)) => Some(h),
                (_, None) => {
                    let now = engine::resolve_reading(&reading, dir.join(HOST_FILE).is_file(), &has_key);
                    now.resolution.engine.is_none().then_some(now)
                }
            };
            LiveSession { session, engine, held: is_held }
        })
        .collect()
}

/// How much later than its record's `startedAt` a session's process may
/// have started and still be the record's, as attend's ADR-129 rule has it:
/// Claude Code writes the record once it is up, which can take seconds, and
/// a `/clear` rewrites it later still, so only a process that started well
/// after the record is a later one.
const RECORD_SLACK_MS: u64 = 60_000;

/// Whether `r`'s process is running and is the one it names. Pid 0 is no
/// process, and `kill(pid, 0)` is the only sign of a running one. A pid
/// reused by another process is not taken for the session's: where the
/// record carries `procStart` and the kernel gives the process's start in
/// clock ticks, the two must be equal; otherwise the process must have
/// started no later than `startedAt` and [`RECORD_SLACK_MS`].
fn record_alive(r: &SessionRecord) -> bool {
    if r.pid == 0 || !process_alive(r.pid) {
        return false;
    }
    let ticks = process_start_ticks(r.pid);
    same_process(r, ticks, ticks.and_then(ticks_to_ms))
}

/// Whether `r` names a running process whose start is `ticks` after boot,
/// at `began_ms` since the epoch; `None` where the kernel cannot say.
fn same_process(r: &SessionRecord, ticks: Option<u64>, began_ms: Option<u64>) -> bool {
    if let (Some(recorded), Some(actual)) = (r.proc_start, ticks) {
        return recorded == actual;
    }
    match (r.started_at_ms, began_ms) {
        (Some(recorded), Some(began)) => began <= recorded.saturating_add(RECORD_SLACK_MS),
        _ => true,
    }
}

/// Whether a process holds `pid`. Off Unix a session record is taken at its
/// word: Claude Code removes it when the session ends.
fn process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let Ok(pid) = libc::pid_t::try_from(pid) else { return false };
        // SAFETY: signal 0 delivers nothing; the call only checks the pid.
        let ret = unsafe { libc::kill(pid, 0) };
        ret == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

/// When process `pid` started, in clock ticks after boot: `/proc/<pid>/stat`
/// field 22, the value a record's `procStart` holds. `None` where `/proc`
/// cannot say.
fn process_start_ticks(pid: u32) -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // Fields after the command's closing `)` start at field 3.
        stat[stat.rfind(')')? + 1..].split_whitespace().nth(19)?.parse().ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        None
    }
}

/// A start `ticks` after boot in milliseconds since the epoch, over the boot
/// time (`btime` in `/proc/stat`, whole seconds).
fn ticks_to_ms(ticks: u64) -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let btime: u64 = std::fs::read_to_string("/proc/stat").ok()?.lines().find_map(|l| l.strip_prefix("btime ")?.trim().parse().ok())?;
        // SAFETY: sysconf reads a constant.
        let hz = u64::try_from(unsafe { libc::sysconf(libc::_SC_CLK_TCK) }).ok().filter(|h| *h > 0)?;
        Some(btime * 1000 + ticks * 1000 / hz)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = ticks;
        None
    }
}

/// An engine as the operator knows it: its profile name with the profile's
/// display name, such as `claude-code (Claude Code internal)`; a profile of
/// your own has no display name, and no engine reads `no engine`.
pub fn engine_label(name: Option<&str>, profiles: &BTreeMap<String, Profile>) -> String {
    let Some(name) = name else { return "no engine".to_string() };
    match profiles.get(name).and_then(|p| p.display_name.as_deref()) {
        Some(shown) => format!("{name} ({shown})"),
        None => name.to_string(),
    }
}

/// One live session as `ways status` and `ways agent status` list it: the
/// short id, the engine, and the reason, which for an unavailable engine
/// names the remedy (ADR-708 §7).
pub fn session_line(s: &LiveSession, profiles: &BTreeMap<String, Profile>) -> String {
    match &s.engine {
        None => format!("{}  not resolved yet: no judge request so far", s.short_id()),
        Some(_) if s.gate_off() => format!("{}  gate off — {OFF}", s.short_id()),
        Some(e) => format!("{}  {} — {}", s.short_id(), engine_label(e.resolution.engine.as_deref(), profiles), e.resolution.reason),
    }
}

/// What an unset `gate.engine` resolves to now (ADR-708 §8): the first
/// shipped profile whose provider has a key file. `claude-code` is chosen
/// only by name, so it is never the answer.
pub fn unset_engine_line(profiles: &BTreeMap<String, Profile>, has_key: impl Fn(Provider) -> bool) -> String {
    match profile::first_with_key(profiles, has_key) {
        Some(name) => engine_label(Some(name), profiles),
        None => "none: no Anthropic or OpenRouter key file, so an unset engine leaves the gate off".to_string(),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::engine::{resolve_session, Resolution, EXPLICIT, NO_HOST};

    fn resolution(engine: Option<&str>, reason: &str) -> Resolution {
        Resolution { engine: engine.map(str::to_string), reason: reason.to_string() }
    }
    use crate::profile::shipped;

    struct Fx {
        base: PathBuf,
    }

    impl Fx {
        fn new(tag: &str) -> Fx {
            let base = std::env::temp_dir().join(format!("ways-live-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&base);
            std::fs::create_dir_all(base.join("sessions")).unwrap();
            Fx { base }
        }
        fn record(&self, pid: u32, id: &str, started_at: Option<u64>) {
            self.record_started(pid, id, started_at, None);
        }
        /// A record with `startedAt` and `procStart`, written as Claude Code
        /// writes them: a number and a string.
        fn record_started(&self, pid: u32, id: &str, started_at: Option<u64>, proc_start: Option<u64>) {
            let at = started_at.map(|t| format!(",\"startedAt\":{t}")).unwrap_or_default();
            let ps = proc_start.map(|t| format!(",\"procStart\":\"{t}\"")).unwrap_or_default();
            std::fs::write(self.base.join(format!("sessions/{pid}.json")), format!("{{\"pid\":{pid},\"sessionId\":\"{id}\"{at}{ps}}}")).unwrap();
        }
        fn held(&self, id: &str, engine: Option<&str>, provider: Option<Provider>, reason: &str) {
            let e = SessionEngine { resolution: resolution(engine, reason), provider };
            std::fs::create_dir_all(self.base.join("judge").join(id)).unwrap();
            std::fs::write(self.base.join("judge").join(id).join(engine::RESOLUTION_FILE), serde_json::to_vec(&e).unwrap()).unwrap();
        }
        fn layer(&self, text: &str) -> PathBuf {
            let p = self.base.join("agent.yaml");
            std::fs::write(&p, text).unwrap();
            p
        }
        fn live(&self, layer: &Path, has_key: impl Fn(Provider) -> bool) -> Vec<LiveSession> {
            live_sessions_in(&[self.base.join("sessions"), self.base.join("sessions")], &self.base.join("judge"), layer, has_key)
        }
    }

    impl Drop for Fx {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    /// A pid no process holds: a child that ran and was reaped.
    fn reaped_pid() -> u32 {
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        pid
    }

    fn only(p: Provider) -> impl Fn(Provider) -> bool {
        move |q| q == p
    }

    #[test]
    fn each_live_session_shows_its_engine_and_why() {
        let fx = Fx::new("lines");
        let me = std::process::id();
        let parent = std::os::unix::process::parent_id();
        fx.record(me, "3f6c2a91-aaaa", None);
        fx.record(parent, "bbbbbbbb-0000", None);
        fx.held("3f6c2a91-aaaa", Some("claude-code"), Some(Provider::ClaudeCode), EXPLICIT);
        let fell = resolve_session(Some("claude-code"), &shipped(), false, only(Provider::Anthropic));
        fx.held("bbbbbbbb-0000", fell.engine.as_deref(), Some(Provider::Anthropic), &fell.reason);
        let layer = fx.layer("engine: claude-code\n");
        let live = fx.live(&layer, only(Provider::Anthropic));
        let lines: Vec<String> = live.iter().map(|s| session_line(s, &shipped())).collect();
        assert_eq!(lines, ["3f6c2a91  claude-code (Claude Code internal) — explicit", "bbbbbbbb  anthropic (Anthropic API key) — claude-code unavailable: no Claude Code host"]);
        assert!(live.iter().all(|s| s.held));
        assert_eq!(fell.reason, NO_HOST);
    }

    #[test]
    fn a_session_with_no_engine_holds_nothing_and_resolves_as_the_gate_does() {
        let fx = Fx::new("none");
        let me = std::process::id();
        fx.record(me, "cccccccc-0000", None);
        let layer = fx.layer("engine: claude-code\n");
        // No key and no host: no engine, and its reason names the remedy.
        let live = fx.live(&layer, |_| false);
        let s = &live[0];
        assert!(!s.held && s.engine.as_ref().unwrap().resolution.is_unavailable(), "{s:?}");
        let line = session_line(s, &shipped());
        assert!(line.starts_with("cccccccc  no engine — engine_unavailable: claude-code unavailable"), "{line}");
        assert!(line.contains("configure an Anthropic or OpenRouter key") && !line.contains("--provider claude-code"), "{line}");
        // A key now: it would resolve to an engine and has asked nothing yet.
        let live = fx.live(&layer, |_| true);
        assert_eq!(live[0].engine, None);
        assert_eq!(session_line(&live[0], &shipped()), "cccccccc  not resolved yet: no judge request so far");
    }

    #[test]
    fn a_layer_the_gate_cannot_use_shows_its_config_reason() {
        let fx = Fx::new("config");
        fx.record(std::process::id(), "dddddddd-0000", None);
        let layer = fx.layer("engine: [claude-code\n");
        let live = fx.live(&layer, |_| true);
        let reason = &live[0].engine.as_ref().unwrap().resolution.reason;
        assert!(reason.starts_with("config: ") && reason.ends_with("the gate is off until it is fixed"), "{reason}");
        assert!(!live[0].gate_off());
    }

    #[test]
    fn mode_off_shows_the_gate_off_over_a_held_engine() {
        let fx = Fx::new("off");
        fx.record(std::process::id(), "3f6c2a91-aaaa", None);
        fx.held("3f6c2a91-aaaa", Some("claude-code"), Some(Provider::ClaudeCode), EXPLICIT);
        let layer = fx.layer("engine: claude-code\nmode: off\n");
        let live = fx.live(&layer, |_| true);
        assert!(live[0].held && live[0].gate_off(), "{:?}", live[0]);
        assert_eq!(session_line(&live[0], &shipped()), "3f6c2a91  gate off — gate.mode off");
    }

    #[test]
    fn only_a_running_process_the_record_names_is_live() {
        let fx = Fx::new("alive");
        let me = std::process::id();
        let dead = reaped_pid();
        fx.record(dead, "deadbeef-0000", None);
        fx.record(0, "zero0000-0000", None);
        fx.record(me, "3f6c2a91-aaaa", None);
        let layer = fx.layer("engine: claude-code\n");
        let ids: Vec<String> = fx.live(&layer, |_| false).into_iter().map(|s| s.session).collect();
        assert_eq!(ids, ["3f6c2a91-aaaa"], "a reaped pid and pid 0 are not live, and two listings of one dir count once");
    }

    #[test]
    fn a_record_names_its_process_by_proc_start_where_the_kernel_gives_it() {
        let Some(ticks) = process_start_ticks(std::process::id()) else { return }; // no /proc
        let fx = Fx::new("procstart");
        let me = std::process::id();
        let layer = fx.layer("engine: claude-code\n");
        let began = ticks_to_ms(ticks).unwrap();
        fx.record_started(me, "3f6c2a91-aaaa", Some(began), Some(ticks));
        assert_eq!(fx.live(&layer, |_| false).len(), 1, "procStart is this process's start");
        // A start the pid's process does not have: the pid was reused, even
        // with a `startedAt` the fallback rule would take.
        fx.record_started(me, "3f6c2a91-aaaa", Some(began), Some(ticks + 1));
        assert!(fx.live(&layer, |_| false).is_empty(), "procStart differs: another process holds the pid");
        // No procStart: the process started no later than startedAt + 60 s.
        fx.record_started(me, "3f6c2a91-aaaa", Some(began - 59_000), None);
        assert_eq!(fx.live(&layer, |_| false).len(), 1, "started 59 s after the record");
        fx.record_started(me, "3f6c2a91-aaaa", Some(began - 61_000), None);
        assert!(fx.live(&layer, |_| false).is_empty(), "started 61 s after the record: a later process");
    }

    fn rec(started_at_ms: Option<u64>, proc_start: Option<u64>) -> SessionRecord {
        SessionRecord { pid: 1, session_id: "s".into(), cwd: None, started_at_ms, proc_start, path: PathBuf::new() }
    }

    #[test]
    fn proc_start_must_equal_the_kernels_where_both_are_known() {
        let at = 1_792_688_488_645;
        assert!(same_process(&rec(Some(at), Some(52_880_213)), Some(52_880_213), Some(at)));
        // Pid reused: a different start, whatever startedAt says.
        assert!(!same_process(&rec(Some(at), Some(52_880_213)), Some(52_880_214), Some(at)));
        assert!(!same_process(&rec(None, Some(52_880_213)), Some(99), None));
        // A procStart the kernel cannot check falls back to startedAt.
        assert!(same_process(&rec(Some(at), Some(52_880_213)), None, None));
    }

    #[test]
    fn with_no_proc_start_the_process_started_no_later_than_started_at_plus_60_s() {
        let at = 1_792_688_488_645;
        for (began, live) in [
            (at + 6_000, true),            // a slow start
            (at + 60_000, true),           // the edge
            (at + 60_001, false),          // a later process took the pid
            (at - 3_600_000, true),        // a record rewritten by /clear, an hour on
            (at + 3_600_000, false),       // an hour later
        ] {
            assert_eq!(same_process(&rec(Some(at), None), None, Some(began)), live, "began {}", began as i64 - at as i64);
        }
        // Nothing to compare: the pid answering is enough.
        assert!(same_process(&rec(None, None), None, Some(at)));
        assert!(same_process(&rec(Some(at), None), None, None));
    }

    #[test]
    fn sessions_dirs_put_the_config_dir_first_and_list_each_once() {
        let home = Path::new("/h");
        assert_eq!(sessions_dirs_for(None, home), [PathBuf::from("/h/.claude/sessions")]);
        assert_eq!(sessions_dirs_for(Some(Path::new("/c")), home), [PathBuf::from("/c/sessions"), PathBuf::from("/h/.claude/sessions")]);
        assert_eq!(sessions_dirs_for(Some(Path::new("/h/.claude")), home), [PathBuf::from("/h/.claude/sessions")]);
    }

    #[test]
    fn an_unset_engine_names_the_first_key_engine_never_claude_code() {
        assert_eq!(unset_engine_line(&shipped(), |_| true), "anthropic (Anthropic API key)");
        assert_eq!(unset_engine_line(&shipped(), only(Provider::Openrouter)), "openrouter (OpenRouter API key)");
        assert!(unset_engine_line(&shipped(), |_| false).starts_with("none: no Anthropic or OpenRouter key file"));
        assert_eq!(engine_label(Some("mine"), &shipped()), "mine");
    }
}
