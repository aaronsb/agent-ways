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
//! References:
//! - ADR-403 §1: the command kind's block and its recorded failures.

use std::io::Read;
use std::path::Path;
use std::process::{Command as Process, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use sensor_spec::{CommandBlock, Output};

use super::hosted::{Fetched, Source};

/// The most stdout a run may print, in bytes.
const MAX_OUTPUT: u64 = 1024 * 1024;

/// A command sensor's poll, for the project at `project`.
pub struct Command {
    block: CommandBlock,
    project: String,
}

impl Command {
    pub fn new(block: CommandBlock, project: &str) -> Command {
        Command { block, project: project.to_string() }
    }
}

impl Source for Command {
    fn fetch(&mut self) -> Result<Fetched, String> {
        let out = run(&self.block, Path::new(&self.project))?;
        match self.block.output {
            Output::Json => serde_json::from_str(&out).map(Fetched::Json).map_err(|e| format!("its output is not one JSON document: {e}")),
            Output::Lines => lines(&out).map(Fetched::Lines),
        }
    }
}

/// Run `block` from `project` and return its stdout. The command runs in
/// a process group of its own, so the timeout ends it and everything it
/// started, and the timeout covers its pipes too: a process left behind
/// that holds stdout open ends with the group at the deadline.
fn run(block: &CommandBlock, project: &Path) -> Result<String, String> {
    let Some((program, args)) = block.argv.split_first() else { return Err("its argv is empty".into()) };
    let cwd = block.cwd.as_ref().map_or(project.to_path_buf(), |c| project.join(c));
    if !cwd.is_dir() {
        return Err(format!("its cwd {} is not a directory", cwd.display()));
    }
    let mut cmd = Process::new(program);
    cmd.args(args).envs(&block.env).current_dir(&cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
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
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => return timed_out(&mut child),
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(format!("cannot wait for {program}: {e}")),
        }
    };
    let (mut out, mut err) = (None, None);
    while out.is_none() || err.is_none() {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok((true, buf)) => out = Some(buf),
            Ok((false, buf)) => err = Some(buf),
            Err(_) => return timed_out(&mut child),
        }
    }
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

/// End `child` and every process in its group.
fn kill_group(child: &mut std::process::Child) {
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
    use std::collections::BTreeMap;

    fn block(script: &str, output: Output, timeout: u64) -> CommandBlock {
        CommandBlock { argv: vec!["sh".into(), "-c".into(), script.into()], env: BTreeMap::from([("X_SENSOR".into(), "from-env".into())]), cwd: None, timeout, output }
    }

    fn fetch(script: &str, output: Output, timeout: u64) -> Result<Fetched, String> {
        Command::new(block(script, output, timeout), "/").fetch()
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
        assert!(Command::new(missing, "/").fetch().unwrap_err().starts_with("cannot run /nonexistent/sensor-cmd: "));
        let gone = CommandBlock { argv: vec!["sh".into()], env: BTreeMap::new(), cwd: Some("nonexistent-dir".into()), timeout: 10, output: Output::Json };
        assert_eq!(Command::new(gone, "/").fetch().unwrap_err(), "its cwd /nonexistent-dir is not a directory");
        assert_eq!(fetch("head -c 2000000 /dev/zero | tr '\\0' a", Output::Json, 10).unwrap_err(), "sh printed more than 1024 KiB");
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
