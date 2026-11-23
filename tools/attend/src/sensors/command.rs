// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The poll of a `command` sensor: its argv run directly, with no shell,
//! in its `cwd` (the project when unset) with its `env` added, killed once
//! its timeout passes. Its stdout is one JSON document (`output: json`) or
//! `magnitude|description` lines (`output: lines`). A run that cannot
//! start, times out, exits non-zero or prints what does not parse is a
//! failed poll, and the failure says which.
//!
//! The session's variables and tokens are kept out of the command's
//! environment. From a shipped or user file it inherits attend's
//! environment less every variable whose name starts with one of
//! [`STRIPPED`] (Claude Code's session id, project dir, config dir and
//! OAuth token, the API key and base URL, attend's own re-exec and
//! hand-over markers) or is one of [`STRIPPED_NAMES`]. From a project file,
//! code a repository's author chose, it starts from an empty environment
//! with only [`PROJECT_ENV`] carried over. The file's `env` adds to either.
//!
//! This is not a sandbox: the command runs as the operator. attend finds
//! the session through parent processes, not the environment. An
//! `attend` the command runs refuses every verb that acts as the session:
//! it sees a sensor host among its ancestors, or, once it has detached
//! from the host, the sensor marker [`sensor_env`] sets in its
//! environment.
//!
//! References:
//! - ADR-403 §1: the command kind's block and its recorded failures.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Read;
use std::path::Path;
use std::process::{Command as Process, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use sensor_spec::{CommandBlock, Output};

use super::hosted::{Fetched, Source};

/// The most stdout a run may print, in bytes.
const MAX_OUTPUT: u64 = 1024 * 1024;

/// The prefixes of the variables a shipped or user file's command does
/// not inherit.
pub const STRIPPED: &[&str] = &["CLAUDE", "ANTHROPIC_", "ATTEND_"];

/// The variables, by exact name, a shipped or user file's command does not
/// inherit: the marker Claude Code sets for tools it runs, and the Bedrock
/// token it may run under.
pub const STRIPPED_NAMES: &[&str] = &["AI_AGENT", "AWS_BEARER_TOKEN_BEDROCK"];

/// The variables a project file's command inherits; `LC_*` stands for
/// every locale variable.
pub const PROJECT_ENV: &[&str] = &["PATH", "HOME", "USER", "LOGNAME", "SHELL", "LANG", "LC_*", "TZ", "TMPDIR", "XDG_RUNTIME_DIR", "DBUS_SESSION_BUS_ADDRESS"];

/// Which environment a command starts from, by the layer of the file that
/// set what it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Inherit {
    /// A shipped or user file: attend's environment less [`STRIPPED`].
    Stripped,
    /// A project file: only [`PROJECT_ENV`].
    Project,
}

impl Inherit {
    /// The rule for a command whose `argv`, `env` or `cwd` the layer
    /// `origin` set last.
    pub fn of(origin: Option<sensor_spec::Origin>) -> Inherit {
        match origin {
            Some(sensor_spec::Origin::Project) => Inherit::Project,
            _ => Inherit::Stripped,
        }
    }

    /// The variables of `vars` a command under this rule inherits.
    pub fn filter<I: IntoIterator<Item = (OsString, OsString)>>(self, vars: I) -> Vec<(OsString, OsString)> {
        vars.into_iter()
            .filter(|(k, _)| {
                let k = k.to_string_lossy();
                match self {
                    Inherit::Stripped => !STRIPPED.iter().any(|p| k.starts_with(p)) && !STRIPPED_NAMES.contains(&k.as_ref()),
                    Inherit::Project => PROJECT_ENV.iter().any(|p| match p.strip_suffix('*') {
                        Some(prefix) => k.starts_with(prefix),
                        None => k == *p,
                    }),
                }
            })
            .collect()
    }
}

/// A command sensor's poll, for the project at `project`.
pub struct Command {
    block: CommandBlock,
    project: String,
    inherit: Inherit,
}

impl Command {
    pub fn new(block: CommandBlock, project: &str, inherit: Inherit) -> Command {
        Command { block, project: project.to_string(), inherit }
    }
}

impl Source for Command {
    fn fetch(&mut self) -> Result<Fetched, String> {
        let out = run(&self.block, Path::new(&self.project), self.inherit)?;
        match self.block.output {
            Output::Json => serde_json::from_str(&out).map(Fetched::Json).map_err(|e| format!("its output is not one JSON document: {e}")),
            Output::Lines => lines(&out).map(Fetched::Lines),
        }
    }
}

/// Give `cmd`, a process that runs a command a sensor file chose, its
/// environment: the variables `inherit` keeps of attend's own, then
/// `extra`, the sensor file's `env`, then the sensor marker naming this
/// process ([`attend_presence::session::mark_sensor_child`]), last so
/// neither the inherited variables nor the file can remove or replace it.
/// A `command` sensor's child and an `mcp` sensor's server are built
/// here; the compiled sensors that run `git` or `ways` mark those with
/// `mark_sensor_child` directly.
pub fn sensor_env(cmd: &mut Process, inherit: Inherit, extra: &BTreeMap<String, String>) {
    cmd.env_clear().envs(inherit.filter(std::env::vars_os())).envs(extra);
    attend_presence::session::mark_sensor_child(cmd);
}

/// Run `block` from `project` and return its stdout. The command runs in
/// a process group of its own, so the timeout ends it and everything it
/// started, and the timeout covers its pipes too: a process left behind
/// that holds stdout open ends with the group at the deadline.
fn run(block: &CommandBlock, project: &Path, inherit: Inherit) -> Result<String, String> {
    let Some((program, args)) = block.argv.split_first() else { return Err("its argv is empty".into()) };
    let cwd = block.cwd.as_ref().map_or(project.to_path_buf(), |c| project.join(c));
    if !cwd.is_dir() {
        return Err(format!("its cwd {} is not a directory", cwd.display()));
    }
    let mut cmd = Process::new(program);
    sensor_env(&mut cmd, inherit, &block.env);
    cmd.args(args).current_dir(&cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = cmd.spawn().map_err(|e| format!("cannot run {program}: {e}"))?;
    let (tx, rx) = mpsc::channel::<(bool, Vec<u8>)>();
    for (is_out, pipe) in [(true, child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>)), (false, child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>))] {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(p) = pipe {
                p.take(MAX_OUTPUT + 1).read_to_end(&mut buf).ok();
            }
            tx.send((is_out, buf)).ok();
        });
    }
    let deadline = Instant::now() + Duration::from_secs(block.timeout);
    let timed_out = |child: &mut std::process::Child| {
        kill_group(child);
        child.wait().ok();
        Err(format!("{program} timed out after {}s", block.timeout))
    };
    // The leader is left unreaped until the end, so its group id stays
    // reserved through any group kill.
    loop {
        match leader_exited(&mut child) {
            Ok(true) => break,
            Ok(false) if Instant::now() >= deadline => return timed_out(&mut child),
            Ok(false) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(format!("cannot wait for {program}: {e}")),
        }
    }
    let (mut out, mut err) = (None, None);
    while out.is_none() || err.is_none() {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok((true, buf)) => out = Some(buf),
            Ok((false, buf)) => err = Some(buf),
            Err(_) => return timed_out(&mut child),
        }
    }
    let status = child.wait().map_err(|e| format!("cannot wait for {program}: {e}"))?;
    let (out, err) = (out.unwrap_or_default(), err.unwrap_or_default());
    if out.len() as u64 > MAX_OUTPUT {
        return Err(format!("{program} printed more than {} KiB", MAX_OUTPUT / 1024));
    }
    if !status.success() {
        let first = String::from_utf8_lossy(&err).lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default().to_string();
        let code = status.code().map_or("a signal".to_string(), |c| format!("status {c}"));
        return Err(match first.is_empty() {
            true => format!("{program} exited with {code}"),
            false => format!("{program} exited with {code}: {first}"),
        });
    }
    String::from_utf8(out).map_err(|_| format!("{program} printed text that is not UTF-8"))
}

/// Whether `child` has exited. On Unix the exit is seen without reaping
/// it (`waitid` with `WNOWAIT`), so it stays a zombie and no new process
/// can take its process group id before the group is killed.
pub(super) fn leader_exited(child: &mut std::process::Child) -> std::io::Result<bool> {
    #[cfg(unix)]
    {
        // SAFETY: `info` is a zeroed siginfo_t that waitid fills; with
        // WNOHANG and no exit it leaves si_signo at 0.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::waitid(libc::P_PID, child.id() as libc::id_t, &mut info, libc::WEXITED | libc::WNOWAIT | libc::WNOHANG) };
        if rc != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(info.si_signo == libc::SIGCHLD)
    }
    #[cfg(not(unix))]
    {
        Ok(child.try_wait()?.is_some())
    }
}

/// End `child` and every process in its group.
pub(super) fn kill_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    // SAFETY: kill(2) takes no pointers; a negative pid names the process
    // group the child leads, which process_group(0) made.
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    child.kill().ok();
}

/// `magnitude|description` lines, blank lines skipped.
fn lines(text: &str) -> Result<Vec<(f64, String)>, String> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let (m, d) = l.split_once('|').ok_or_else(|| format!("its output line `{l}` is not magnitude|description"))?;
            let m: f64 = m.trim().parse().map_err(|_| format!("its output line `{l}` has no number before `|`"))?;
            if !(0.0..=100.0).contains(&m) {
                return Err(format!("its output line `{l}` has a magnitude outside 0 to 100"));
            }
            Ok((m, d.trim().to_string()))
        })
        .collect()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn block(script: &str, output: Output, timeout: u64) -> CommandBlock {
        CommandBlock { argv: vec!["sh".into(), "-c".into(), script.into()], env: BTreeMap::from([("X_SENSOR".into(), "from-env".into())]), cwd: None, timeout, output }
    }

    fn fetch(script: &str, output: Output, timeout: u64) -> Result<Fetched, String> {
        Command::new(block(script, output, timeout), "/", Inherit::Stripped).fetch()
    }

    #[test]
    fn json_output_reads_as_one_document_with_env_and_cwd() {
        let Ok(Fetched::Json(v)) = fetch(r#"printf '{"env":"%s","cwd":"%s"}' "$X_SENSOR" "$(pwd)""#, Output::Json, 10) else { panic!() };
        assert_eq!(v, serde_json::json!({"env": "from-env", "cwd": "/"}));
    }

    #[test]
    fn lines_output_reads_magnitude_and_description() {
        let Ok(Fetched::Lines(l)) = fetch("printf '2|one\\n\\n0.5 | two\\n'", Output::Lines, 10) else { panic!() };
        assert_eq!(l, vec![(2.0, "one".to_string()), (0.5, "two".to_string())]);
        assert_eq!(fetch("echo nope", Output::Lines, 10).unwrap_err(), "its output line `nope` is not magnitude|description");
        assert_eq!(fetch("echo '200|x'", Output::Lines, 10).unwrap_err(), "its output line `200|x` has a magnitude outside 0 to 100");
    }

    #[test]
    fn each_failure_says_what_failed() {
        let started = Instant::now();
        assert_eq!(fetch("sleep 5", Output::Json, 1).unwrap_err(), "sh timed out after 1s");
        assert!(started.elapsed() < Duration::from_secs(4), "the run is killed at its timeout");
        assert_eq!(fetch("echo broken >&2; exit 3", Output::Json, 10).unwrap_err(), "sh exited with status 3: broken");
        assert_eq!(fetch("exit 4", Output::Json, 10).unwrap_err(), "sh exited with status 4");
        assert!(fetch("echo '{not json'", Output::Json, 10).unwrap_err().starts_with("its output is not one JSON document: "));
        let missing = CommandBlock { argv: vec!["/nonexistent/sensor-cmd".into()], env: BTreeMap::new(), cwd: None, timeout: 10, output: Output::Json };
        assert!(Command::new(missing, "/", Inherit::Stripped).fetch().unwrap_err().starts_with("cannot run /nonexistent/sensor-cmd: "));
        let gone = CommandBlock { argv: vec!["sh".into()], env: BTreeMap::new(), cwd: Some("nonexistent-dir".into()), timeout: 10, output: Output::Json };
        assert_eq!(Command::new(gone, "/", Inherit::Stripped).fetch().unwrap_err(), "its cwd /nonexistent-dir is not a directory");
        assert_eq!(fetch("head -c 2000000 /dev/zero | tr '\\0' a", Output::Json, 10).unwrap_err(), "sh printed more than 1024 KiB");
    }

    fn vars(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        pairs.iter().map(|(k, v)| (OsString::from(k), OsString::from(v))).collect()
    }

    fn names(v: Vec<(OsString, OsString)>) -> Vec<String> {
        v.into_iter().map(|(k, _)| k.to_string_lossy().into_owned()).collect()
    }

    /// A user or shipped file's command loses Claude Code's, Anthropic's and
    /// attend's variables and keeps the rest; a project file's keeps only
    /// the fixed set.
    #[test]
    fn each_layer_inherits_its_own_environment() {
        let all = vars(&[
            ("CLAUDE_CODE_SESSION_ID", "fake"),
            ("CLAUDE_PROJECT_DIR", "/p"),
            ("CLAUDECODE", "1"),
            ("ANTHROPIC_API_KEY", "fake"),
            ("ATTEND_RUN_SINCE", "1"),
            ("AI_AGENT", "claude-code"),
            ("AWS_BEARER_TOKEN_BEDROCK", "fake"),
            ("AI_AGENTS_DIR", "/a"),
            ("KEEP_ME", "1"),
            ("PATH", "/bin"),
            ("HOME", "/h"),
            ("LC_ALL", "C"),
            ("TZ", "UTC"),
        ]);
        assert_eq!(names(Inherit::Stripped.filter(all.clone())), ["AI_AGENTS_DIR", "KEEP_ME", "PATH", "HOME", "LC_ALL", "TZ"], "exact names strip only themselves");
        assert_eq!(names(Inherit::Project.filter(all)), ["PATH", "HOME", "LC_ALL", "TZ"]);
        assert_eq!(Inherit::of(Some(sensor_spec::Origin::Project)), Inherit::Project);
        assert_eq!(Inherit::of(Some(sensor_spec::Origin::User)), Inherit::Stripped);
        assert_eq!(Inherit::of(Some(sensor_spec::Origin::Builtin)), Inherit::Stripped);
    }

    /// A command from either layer carries the sensor marker naming this
    /// process, whatever the inherited environment or the file's `env`
    /// set it to.
    #[test]
    fn every_layer_carries_the_sensor_marker() {
        let own = attend_presence::session::marker_value(std::process::id());
        for inherit in [Inherit::Stripped, Inherit::Project] {
            let mut b = block(r#"printf '{"marker":"%s"}' "$ATTEND_SENSOR_HOST""#, Output::Json, 10);
            b.env.insert(attend_presence::session::SENSOR_HOST_VAR.into(), "0".into());
            let Ok(Fetched::Json(v)) = Command::new(b, "/", inherit).fetch() else { panic!("{inherit:?}") };
            assert_eq!(v, serde_json::json!({ "marker": own }), "{inherit:?}");
        }
    }

    /// A process the command leaves behind holding stdout open does not
    /// hold the poll past its timeout.
    #[test]
    fn the_timeout_covers_a_background_process_holding_the_pipe() {
        let started = Instant::now();
        assert_eq!(fetch("sleep 30 & echo {}", Output::Json, 1).unwrap_err(), "sh timed out after 1s");
        assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
    }

    /// The timeout ends the whole process group, not only the shell.
    #[test]
    fn the_timeout_ends_what_the_command_started() {
        let dir = std::env::temp_dir().join(format!("attend-cmd-group-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pidfile = dir.join("pid");
        let script = format!("sleep 30 & echo $! > {}; wait", pidfile.display());
        assert_eq!(fetch(&script, Output::Json, 1).unwrap_err(), "sh timed out after 1s");
        let pid = std::fs::read_to_string(&pidfile).unwrap().trim().to_string();
        std::thread::sleep(Duration::from_millis(100));
        let alive = std::process::Command::new("kill").args(["-0", &pid]).stderr(Stdio::null()).status().unwrap().success();
        std::fs::remove_dir_all(&dir).ok();
        assert!(!alive, "the background sleep {pid} outlived the timeout");
    }
}
