//! The attend CLI verbs the chat's cleanup items run (ADR-404 D3e, ADR-405
//! amendment of 2026-10-13): `attend message delete` and `attend message
//! tombstone`. attend-chat has no crate dependency on attend, so it runs
//! attend as a child process with `--json` and reads the report: the attend
//! installed beside attend-chat, else the one on `PATH` ([`program`]). An attend too old to have a verb is told apart from one that
//! refused: the first needs updating, the second gave its reason.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::groups::BASE_CHANNEL_NAME;
use crate::tabs::Tab;

/// The attend the verbs run: the one installed beside this program, as the
/// suite installs them together, else `attend` on `PATH`.
pub fn program() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("attend")))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| "attend".into())
}

/// Which messages a delete reaches: `#open`, a channel, or the direct
/// messages between the person and one agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    Open,
    Channel(String),
    /// The thread with the agent of this membership id (`--dm`).
    Dm(String),
}

impl Scope {
    /// The scope of a channel's menu: `#open` for the base channel.
    pub fn channel(name: &str) -> Scope {
        if name == BASE_CHANNEL_NAME {
            Scope::Open
        } else {
            Scope::Channel(name.to_string())
        }
    }

    /// The scope of the tab shown: `#open`, a channel, or the operator's DMs
    /// with an agent. The operator view and an agent's own DMs have none.
    pub fn of(tab: &Tab) -> Option<Scope> {
        match tab {
            Tab::Channel(c) => Some(Scope::channel(c)),
            Tab::Dm(m) => Some(Scope::Dm(m.clone())),
            Tab::Merged | Tab::Review(_) => None,
        }
    }

    /// The scope's flags for `attend message delete`. A thread is always
    /// the person's (`--as-person`): attend would otherwise take the caller
    /// to be a Claude Code session the chat was started under, and the
    /// delete would reach that session's thread with the agent.
    pub fn args(&self) -> Vec<String> {
        match self {
            Scope::Open => vec!["--open".into()],
            Scope::Channel(c) => vec!["--channel".into(), c.clone()],
            Scope::Dm(m) => vec!["--dm".into(), m.clone(), "--as-person".into()],
        }
    }

    /// The scope as a menu id carries it.
    pub fn key(&self) -> String {
        match self {
            Scope::Open => BASE_CHANNEL_NAME.into(),
            Scope::Channel(c) => format!("#{c}"),
            Scope::Dm(m) => format!("dm:{m}"),
        }
    }

    /// The scope a menu id carries ([`Scope::key`]).
    pub fn from_key(key: &str) -> Option<Scope> {
        if key == BASE_CHANNEL_NAME {
            return Some(Scope::Open);
        }
        if let Some(c) = key.strip_prefix('#') {
            return Some(Scope::channel(c));
        }
        key.strip_prefix("dm:").map(|m| Scope::Dm(m.to_string()))
    }
}

/// Delete older than: the submenu's choices and the `--older-than` each
/// passes. Custom… takes a duration or a local date instead ([`custom`]).
pub const OLDER_THAN: [(&str, &str); 3] = [("Current sessions", "current"), ("Today (local midnight)", "today"), ("All", "all")];

/// What Custom… takes: a duration such as `2h` or `3d`, or a local date
/// `YYYY-MM-DD`, as attend reads it; `Err` says what it takes.
pub fn custom(text: &str) -> Result<String, String> {
    let t = text.trim();
    if agent_fmt::when::parse_span(t).is_some() || agent_fmt::when::parse_local_date(t).is_some() {
        Ok(t.to_string())
    } else {
        Err("not a duration or a date YYYY-MM-DD".into())
    }
}

/// How a question reads `--older-than`'s value: `today` as from before
/// today, a date as from before it, a duration as older than it; `all` adds
/// nothing.
pub fn older_words(older: &str) -> String {
    match older {
        "all" => String::new(),
        "current" => " older than the current sessions".into(),
        "today" => " from before today".into(),
        d if agent_fmt::when::parse_local_date(d).is_some() => format!(" from before {d}"),
        d => format!(" older than {d}"),
    }
}

/// `attend message delete` for `scope` and `older`, reporting only with
/// `dry_run`.
pub fn delete_argv(scope: &Scope, older: &str, dry_run: bool) -> Vec<String> {
    let mut out: Vec<String> = vec!["message".into(), "delete".into()];
    out.extend(scope.args());
    out.extend(["--older-than".into(), older.to_string()]);
    if dry_run {
        out.push("--dry-run".into());
    }
    out.push("--json".into());
    out
}

/// `attend message delete --undo <batch>`.
pub fn undo_argv(batch: &str) -> Vec<String> {
    ["message", "delete", "--undo", batch, "--json"].map(String::from).to_vec()
}

/// `attend message tombstone <session>`.
pub fn tombstone_argv(session: &str) -> Vec<String> {
    ["message", "tombstone", session, "--json"].map(String::from).to_vec()
}

/// What a delete marked, or would mark on a dry run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub marked: usize,
    /// The batch `--undo` names; none on a dry run or when nothing was marked.
    pub batch: Option<String>,
    /// Each live session that kept messages it has not read: its name and
    /// how many.
    pub held: Vec<(String, usize)>,
}

impl Report {
    /// How many messages stay unread, and by how many sessions.
    pub fn kept(&self) -> (usize, usize) {
        (self.held.iter().map(|(_, n)| n).sum(), self.held.len())
    }

    /// `· 3 unread are kept`, or nothing when none are.
    pub fn kept_words(&self) -> String {
        match self.kept() {
            (0, _) => String::new(),
            (1, _) => " · 1 unread is kept".into(),
            (n, _) => format!(" · {n} unread are kept"),
        }
    }
}

/// What an undo restored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Undone {
    pub unmarked: usize,
    pub already_removed: usize,
}

/// Why a verb did nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// No `attend` on `PATH`.
    NoAttend { verb: String },
    /// The installed attend lacks the verb or a flag of it.
    TooOld { verb: String },
    /// attend refused, in its own words.
    Refused { verb: String, why: String },
    /// attend ended with no report the chat can read.
    Broken { verb: String, why: String },
    /// attend gave no answer in time and was stopped.
    TimedOut { verb: String, wait: Duration },
    /// The chat was asked to stop it ([`Running::stop`]).
    Stopped { verb: String },
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::NoAttend { verb } => write!(f, "`attend {verb}` needs attend on PATH"),
            Failure::TooOld { verb } => write!(f, "`attend {verb}` is missing: needs a newer attend"),
            Failure::Refused { verb, why } => write!(f, "{}: {why}", verb.rsplit(' ').next().unwrap_or(verb)),
            Failure::Broken { verb, why } => write!(f, "attend {verb}: {why}"),
            Failure::TimedOut { verb, wait } => write!(f, "`attend {verb}` gave no answer in {} s and was stopped", wait.as_secs_f64()),
            Failure::Stopped { verb } => write!(f, "`attend {verb}` was stopped"),
        }
    }
}

/// A verb running on a thread of its own, which answers once over a
/// channel ([`start`]); the screen polls it on its tick and stays live
/// meanwhile.
pub struct Running {
    rx: Receiver<Result<serde_json::Value, Failure>>,
    stop: Arc<AtomicBool>,
}

impl Running {
    /// Stop it: the child is killed, and the worker answers
    /// [`Failure::Stopped`] without waiting on any pipe a process the child
    /// started still holds open.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// The answer, once it has come.
    pub fn poll(&self) -> Option<Result<serde_json::Value, Failure>> {
        match self.rx.try_recv() {
            Ok(answer) => Some(answer),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(Failure::Broken { verb: "attend".into(), why: "the worker ended without an answer".into() })),
        }
    }

    /// Block for the answer.
    pub fn wait(self) -> Result<serde_json::Value, Failure> {
        self.rx.recv().unwrap_or_else(|_| Err(Failure::Broken { verb: "attend".into(), why: "the worker ended without an answer".into() }))
    }
}

/// Run `program` with `args` on a thread of its own, stopping it after
/// `wait` when one is given. The chat's cleanup verbs give none: a delete
/// or a tombstone runs to its end however long attend takes, and the chat
/// says what it is doing until then (ADR-405, amendment of 2026-10-14).
pub fn start(program: PathBuf, args: Vec<String>, wait: Option<Duration>) -> Running {
    let (tx, rx) = channel();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = Arc::clone(&stop);
    std::thread::spawn(move || {
        let _ = tx.send(run(&program, &args, wait, &stopped));
    });
    Running { rx, stop }
}

/// How a child's run ended.
enum Ended {
    /// It ran to its end: its exit status and what it wrote.
    Done(ExitStatus, String, String),
    /// A given wait passed, and it was killed.
    TimedOut,
    /// It was stopped ([`Running::stop`]).
    Stopped,
}

/// Run `program` with `args` and read its one JSON document. A document
/// with an `error` is attend's refusal; no document, from an attend whose
/// argument parser did not know the verb or a flag, means it is too old. A
/// child still running after `wait`, when one is given, is killed.
fn run(program: &Path, args: &[String], wait: Option<Duration>, stop: &AtomicBool) -> Result<serde_json::Value, Failure> {
    let verb = args.iter().take(2).cloned().collect::<Vec<_>>().join(" ");
    let (status, stdout, stderr) = match output(program, args, wait, stop) {
        Ok(Ended::Done(status, out, err)) => (status, out, err),
        Ok(Ended::TimedOut) => return Err(Failure::TimedOut { verb, wait: wait.unwrap_or_default() }),
        Ok(Ended::Stopped) => return Err(Failure::Stopped { verb }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Failure::NoAttend { verb }),
        Err(e) => return Err(Failure::Broken { verb, why: e.to_string() }),
    };
    match serde_json::from_str::<serde_json::Value>(stdout.trim()) {
        Ok(doc) if doc.get("error").is_some() => {
            let why = doc.get("message").and_then(|m| m.as_str()).unwrap_or("refused").to_string();
            Err(Failure::Refused { verb, why })
        }
        Ok(doc) if status.success() => Ok(doc),
        _ if ["unrecognized subcommand", "unexpected argument"].iter().any(|w| stderr.contains(w)) => Err(Failure::TooOld { verb }),
        _ => {
            let why = stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("no report").trim().to_string();
            Err(Failure::Broken { verb, why })
        }
    }
}

/// Run `program` with `args` to its end: its exit status and what it
/// wrote, or how it was cut short. Its output is read as it comes, so a
/// report larger than a pipe holds never stalls it. A child still running
/// once a given `wait` has passed, or when `stop` is set, is killed; and a
/// pipe that a process the child started still holds open after the child
/// ended is waited on only until then, never past a stop. A program file
/// still held open for writing (`ExecutableFileBusy`: an attend being
/// installed, or a test's fake while another thread forks) is tried again
/// shortly.
fn output(program: &Path, args: &[String], wait: Option<Duration>, stop: &AtomicBool) -> std::io::Result<Ended> {
    let mut tries = 0;
    let mut child = loop {
        let spawned = Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
        match spawned {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy && tries < 50 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(20));
            }
            other => break other?,
        }
    };
    let drain = |mut pipe: Box<dyn Read + Send>| {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = pipe.read_to_string(&mut text);
            let _ = tx.send(text);
        });
        rx
    };
    let out = drain(Box::new(child.stdout.take().expect("piped")));
    let err = drain(Box::new(child.stderr.take().expect("piped")));
    let began = Instant::now();
    let cut = || {
        if stop.load(Ordering::Relaxed) {
            Some(Ended::Stopped)
        } else if wait.is_some_and(|w| began.elapsed() >= w) {
            Some(Ended::TimedOut)
        } else {
            None
        }
    };
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if let Some(ended) = cut() {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(ended);
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let read = |rx: &Receiver<String>| loop {
        match rx.recv_timeout(Duration::from_millis(10)) {
            Ok(text) => return Ok(text),
            Err(RecvTimeoutError::Disconnected) => return Ok(String::new()),
            Err(RecvTimeoutError::Timeout) => {
                if let Some(ended) = cut() {
                    return Err(ended);
                }
            }
        }
    };
    let stdout = match read(&out) {
        Ok(t) => t,
        Err(ended) => return Ok(ended),
    };
    let stderr = match read(&err) {
        Ok(t) => t,
        Err(ended) => return Ok(ended),
    };
    Ok(Ended::Done(status, stdout, stderr))
}

/// The count `field` of a report, which every report of the verb carries:
/// one without it is not a report the chat can read.
fn count(doc: &serde_json::Value, field: &str, verb: &str) -> Result<usize, Failure> {
    doc.get(field).and_then(|v| v.as_u64()).map(|n| n as usize).ok_or_else(|| Failure::Broken { verb: verb.into(), why: format!("its report has no `{field}`") })
}

/// What `attend message delete` reported, or its dry run.
pub fn report(doc: &serde_json::Value) -> Result<Report, Failure> {
    let held = doc
        .get("held")
        .and_then(|h| h.as_array())
        .into_iter()
        .flatten()
        .map(|h| Ok((h.get("name").and_then(|n| n.as_str()).unwrap_or("?").to_string(), count(h, "kept", "message delete")?)))
        .collect::<Result<_, Failure>>()?;
    let batch = doc.get("batch").and_then(|b| b.as_str()).map(str::to_string);
    Ok(Report { marked: count(doc, "marked", "message delete")?, batch, held })
}

/// What `attend message delete --undo` reported.
pub fn undone(doc: &serde_json::Value) -> Result<Undone, Failure> {
    Ok(Undone { unmarked: count(doc, "unmarked", "message delete")?, already_removed: count(doc, "already_removed", "message delete")? })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn each_scope_and_age_builds_its_argv() {
        assert_eq!(delete_argv(&Scope::Open, "today", true), s(&["message", "delete", "--open", "--older-than", "today", "--dry-run", "--json"]));
        assert_eq!(delete_argv(&Scope::Channel("deploy".into()), "all", false), s(&["message", "delete", "--channel", "deploy", "--older-than", "all", "--json"]));
        assert_eq!(delete_argv(&Scope::Dm("s1".into()), "3d", false), s(&["message", "delete", "--dm", "s1", "--as-person", "--older-than", "3d", "--json"]));
        assert_eq!(undo_argv("17-4"), s(&["message", "delete", "--undo", "17-4", "--json"]));
        assert_eq!(tombstone_argv("s9"), s(&["message", "tombstone", "s9", "--json"]));
    }

    #[test]
    fn a_tab_gives_its_scope_and_a_menu_id_carries_it() {
        assert_eq!(Scope::of(&Tab::Channel("open".into())), Some(Scope::Open));
        assert_eq!(Scope::of(&Tab::Channel("deploy".into())), Some(Scope::Channel("deploy".into())));
        assert_eq!(Scope::of(&Tab::Dm("s1".into())), Some(Scope::Dm("s1".into())));
        assert_eq!(Scope::of(&Tab::Merged), None);
        assert_eq!(Scope::of(&Tab::Review("s1".into())), None);
        for scope in [Scope::Open, Scope::Channel("deploy".into()), Scope::Dm("s1".into())] {
            assert_eq!(Scope::from_key(&scope.key()), Some(scope));
        }
    }

    #[test]
    fn custom_takes_a_duration_or_a_date() {
        assert_eq!(custom(" 2h "), Ok("2h".into()));
        assert_eq!(custom("2026-10-01"), Ok("2026-10-01".into()));
        assert!(custom("yesterday").is_err() && custom("").is_err());
        assert_eq!(older_words("all"), "");
        assert_eq!(older_words("today"), " from before today");
        assert_eq!(older_words("3d"), " older than 3d");
        assert_eq!(older_words("2026-10-01"), " from before 2026-10-01");
    }

    /// A program in `dir` that prints `stdout` and `stderr` and exits `code`.
    fn fake(dir: &Path, name: &str, stdout: &str, stderr: &str, code: i32) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\nprintf '%s' '{stdout}'\nprintf '%s\\n' '{stderr}' >&2\nexit {code}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    #[cfg(unix)]
    #[test]
    fn a_report_a_refusal_and_an_old_attend_read_apart() {
        // Every fake is written before any is run: a fork in another test's
        // thread can hold a fake open for writing while it is exec'd.
        let dir = crate::test_dir::unique("attend-cli");
        let ok = fake(&dir, "ok", r##"{"scope":"#open","dry_run":true,"marked":12,"batch":null,"ids":[],"held":[{"session":"a","name":"Ada-x","kept":2},{"session":"b","name":"Bo-y","kept":1}]}"##, "", 0);
        let refused = fake(&dir, "refused", r#"{"error":{"kind":"not_exited","session":"s9"},"message":"s9 has not exited"}"#, "attend message tombstone: s9 has not exited", 1);
        let unread = fake(&dir, "unread", r##"{"scope":"#open","dry_run":true}"##, "", 0);
        let old = fake(&dir, "old", "", "error: unrecognized subcommand 'delete'", 2);
        let crashed = fake(&dir, "crashed", "", "thread main panicked", 101);
        let none = dir.join("no-attend");
        let call = |p: &Path, args: Vec<String>| start(p.to_path_buf(), args, None).wait();
        let r = report(&call(&ok, delete_argv(&Scope::Open, "all", true)).unwrap()).unwrap();
        assert_eq!(r, Report { marked: 12, batch: None, held: vec![("Ada-x".into(), 2), ("Bo-y".into(), 1)] });
        assert_eq!(r.kept_words(), " · 3 unread are kept");
        assert_eq!(call(&refused, tombstone_argv("s9")).unwrap_err().to_string(), "tombstone: s9 has not exited", "attend's words, after the verb");
        assert_eq!(call(&unread, delete_argv(&Scope::Open, "all", true)).and_then(|d| report(&d)).unwrap_err().to_string(), "attend message delete: its report has no `marked`", "no count is no zero");
        let e = call(&old, delete_argv(&Scope::Open, "all", true)).unwrap_err();
        assert_eq!(e.to_string(), "`attend message delete` is missing: needs a newer attend");
        assert_eq!(call(&none, tombstone_argv("s9")).unwrap_err().to_string(), "`attend message tombstone` needs attend on PATH");
        assert_eq!(call(&crashed, undo_argv("1")).unwrap_err().to_string(), "attend message delete: thread main panicked");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A stop ends the run at once, both while the child runs and after it
    /// ended with a process it started still holding its output open.
    #[cfg(unix)]
    #[test]
    fn a_stop_ends_the_run_whatever_holds_its_pipes() {
        use std::os::unix::fs::PermissionsExt;
        let dir = crate::test_dir::unique("attend-cli");
        let slow = dir.join("slow");
        std::fs::write(&slow, "#!/bin/sh\nexec sleep 30\n").unwrap();
        let orphan = dir.join("orphan");
        std::fs::write(&orphan, "#!/bin/sh\nsleep 30 &\necho '{}'\n").unwrap();
        for p in [&slow, &orphan] {
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        for p in [slow, orphan] {
            let began = Instant::now();
            let running = start(p, delete_argv(&Scope::Open, "all", false), None);
            std::thread::sleep(Duration::from_millis(300));
            assert!(running.poll().is_none(), "still running, or still held");
            running.stop();
            let e = running.wait().unwrap_err();
            assert_eq!(e.to_string(), "`attend message delete` was stopped");
            assert!(began.elapsed() < Duration::from_secs(5), "stopped, not waited out: {:?}", began.elapsed());
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A child that does not answer in time is stopped, and the wait says
    /// how long it was given; the answer comes over the channel, not the
    /// caller's thread.
    #[cfg(unix)]
    #[test]
    fn a_child_that_does_not_answer_is_stopped() {
        let dir = crate::test_dir::unique("attend-cli");
        let slow = dir.join("slow");
        std::fs::write(&slow, "#!/bin/sh\nexec sleep 30\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&slow, std::fs::Permissions::from_mode(0o755)).unwrap();
        let began = Instant::now();
        let running = start(slow, tombstone_argv("s9"), Some(Duration::from_millis(200)));
        assert!(running.poll().is_none(), "still running: the caller is not held");
        let e = running.wait().unwrap_err();
        assert_eq!(e.to_string(), "`attend message tombstone` gave no answer in 0.2 s and was stopped");
        assert!(began.elapsed() < Duration::from_secs(5), "stopped, not waited out");
        let _ = std::fs::remove_dir_all(dir);
    }
}
