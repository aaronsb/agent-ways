// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! Running a child process under a time limit, for the probes a hook runs,
//! such as [`crate::handoff::repo_root`]'s git call. A hung child (git
//! behind a stale lock on a network filesystem) costs the caller the limit
//! and no more.

use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How a bounded run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// It exited 0 within the limit; its stdout.
    Stdout(String),
    /// It could not be started, or exited non-zero within the limit.
    Failed,
    /// It ran past the limit, or held its stdout open past it, and was
    /// killed.
    TimedOut,
}

/// Run `cmd` with stdin closed, stderr dropped and stdout captured, and
/// kill it once `limit` has passed. On Unix the child leads a process
/// group of its own and the whole group is killed, so what it started
/// dies with it; on Windows only the child is.
pub fn stdout_within(mut cmd: Command, limit: Duration) -> Outcome {
    use std::io::Read;
    let deadline = Instant::now() + limit;
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let Ok(mut child) = spawn(&mut cmd) else { return Outcome::Failed };
    // Read on a thread, so a child that fills the pipe never blocks, and
    // the wait for its stdout keeps to the deadline too.
    let (tx, rx) = mpsc::channel();
    if let Some(mut out) = child.stdout.take() {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = out.read_to_end(&mut buf);
            let _ = tx.send(buf);
        });
    }
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                kill(&mut child);
                return Outcome::TimedOut;
            }
            Err(_) => {
                kill(&mut child);
                return Outcome::Failed;
            }
        }
    };
    // Something the child started can hold its stdout open after it exits.
    let Ok(stdout) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) else {
        kill(&mut child);
        return Outcome::TimedOut;
    };
    if status.success() {
        Outcome::Stdout(String::from_utf8_lossy(&stdout).into_owned())
    } else {
        Outcome::Failed
    }
}

/// Spawn `cmd`, trying again a few times while the executable is busy: a
/// file written a moment ago refuses to exec with ETXTBSY while a forked
/// child elsewhere still holds its write handle.
fn spawn(cmd: &mut Command) -> std::io::Result<std::process::Child> {
    let mut tries = 1;
    loop {
        match cmd.spawn() {
            Err(e) if tries < 5 && e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(20));
            }
            other => return other,
        }
    }
}

/// Kill `child` and, on Unix, the process group it leads, then reap it.
/// The group is signalled before the reap, while its id is still the
/// child's.
fn kill(child: &mut std::process::Child) {
    #[cfg(unix)]
    let _ = Command::new("kill")
        .args(["-KILL", "--", &format!("-{}", child.id())])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn sh(script: &str) -> Command {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", script]);
        cmd
    }

    #[test]
    fn a_quick_command_gives_its_stdout_or_fails() {
        assert_eq!(stdout_within(sh("echo hi"), Duration::from_secs(5)), Outcome::Stdout("hi\n".into()));
        assert_eq!(stdout_within(sh("echo hi; exit 3"), Duration::from_secs(5)), Outcome::Failed);
        assert_eq!(stdout_within(Command::new("/nonexistent/ways-core-probe"), Duration::from_secs(5)), Outcome::Failed);
    }

    #[test]
    fn a_hung_command_is_killed_at_the_limit() {
        let started = Instant::now();
        assert_eq!(stdout_within(sh("sleep 10"), Duration::from_millis(200)), Outcome::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
    }

    /// A child that exits while something it started holds its stdout is
    /// still held to the limit.
    #[test]
    fn a_stdout_held_open_after_exit_is_held_to_the_limit() {
        let started = Instant::now();
        assert_eq!(stdout_within(sh("sleep 10 & echo hi"), Duration::from_millis(300)), Outcome::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
    }
}
