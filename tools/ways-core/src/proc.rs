// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! Running a child process under a time limit, for the probes a hook runs,
//! such as [`crate::handoff::repo_root`]'s git call. A hung child (git
//! behind a stale lock on a network filesystem) costs the caller the limit
//! and no more.

use std::process::{Child, Command, Stdio};
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

/// How long the reader gets to see end of file once the child has
/// exited, however close to the deadline the exit came.
const DRAIN_FLOOR: Duration = Duration::from_millis(50);

/// How long a killed child gets to be reaped before the wait moves to a
/// thread of its own.
const REAP_WAIT: Duration = Duration::from_millis(100);

/// Run `cmd` with stdin closed, stderr dropped and stdout captured, and
/// kill it once `limit` has passed. On Unix the child leads a process
/// group of its own and the whole group is killed, so what it started
/// dies with it; on Windows only the child is.
///
/// The child's exit is seen without reaping it, and it is reaped only
/// after any group kill, so its pid, which is the group id, stays
/// reserved and the kill cannot reach a group that took the id over.
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
    loop {
        match exited(&mut child) {
            Ok(true) => break,
            Ok(false) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(false) => {
                kill(child);
                return Outcome::TimedOut;
            }
            // The child is no longer ours to wait on, so its pid may be
            // another process's by now: signal nothing.
            Err(_) => return Outcome::Failed,
        }
    }
    // Something the child started can hold its stdout open after it
    // exits. An exit just before the deadline still gives the reader
    // time to see end of file.
    let drain = deadline.saturating_duration_since(Instant::now()).max(DRAIN_FLOOR);
    let Ok(stdout) = rx.recv_timeout(drain) else {
        kill(child);
        return Outcome::TimedOut;
    };
    match child.wait() {
        Ok(status) if status.success() => Outcome::Stdout(String::from_utf8_lossy(&stdout).into_owned()),
        _ => Outcome::Failed,
    }
}

/// Whether `child` has exited. On Unix the exit is seen without reaping
/// it (`waitid` with `WNOWAIT`), so it stays a zombie and no new process
/// can take its pid, and with it its process group id, before
/// [`kill`] signals the group.
fn exited(child: &mut Child) -> std::io::Result<bool> {
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

/// Spawn `cmd`, trying again a few times while the executable is busy: a
/// file written a moment ago refuses to exec with ETXTBSY while a forked
/// child elsewhere still holds its write handle.
fn spawn(cmd: &mut Command) -> std::io::Result<Child> {
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
/// The child is still unreaped here, so the group id is still its own.
/// A child that is not reaped within [`REAP_WAIT`] (one in uninterruptible
/// sleep, say) is waited on by a thread of its own, so the caller keeps
/// to its limit.
fn kill(mut child: Child) {
    #[cfg(unix)]
    // SAFETY: kill(2) takes no pointers; a negative pid names the process
    // group the child leads, which process_group(0) made.
    unsafe {
        libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
    }
    let _ = child.kill();
    let until = Instant::now() + REAP_WAIT;
    while Instant::now() < until {
        if !matches!(child.try_wait(), Ok(None)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::spawn(move || {
        let _ = child.wait();
    });
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

    /// A pid file under the temp dir, unique to `name` and this process.
    fn pid_file(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("ways-core-proc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    /// The pid the grandchild wrote to `path`.
    fn read_pid(path: &std::path::Path) -> i32 {
        let pid = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let _ = std::fs::remove_file(path);
        pid.trim().parse().unwrap_or_else(|_| panic!("pid file holds {pid:?}"))
    }

    /// Whether `pid` has ended within a second: signal 0 fails, or on
    /// Linux it is a zombie waiting for its new parent to reap it.
    fn ends_soon(pid: i32) -> bool {
        let until = Instant::now() + Duration::from_secs(1);
        while Instant::now() < until {
            // SAFETY: kill(2) with signal 0 sends nothing and takes no pointers.
            if unsafe { libc::kill(pid, 0) } != 0 {
                return true;
            }
            if std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| s.rsplit_once(')').is_some_and(|(_, rest)| rest.trim_start().starts_with('Z'))) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    /// A grandchild script that prints its pid, writes it to `path` and
    /// holds the stdout it inherited.
    fn holder(path: &std::path::Path) -> String {
        format!("sh -c 'echo $$; echo $$ > {}; exec sleep 10'", path.display())
    }

    /// The grandchild that holds stdout after the child exits dies with
    /// the group at the limit.
    #[test]
    fn the_group_dies_when_stdout_is_held_past_exit() {
        let path = pid_file("held");
        let script = format!("{} & while [ ! -s {} ]; do sleep 0.01; done", holder(&path), path.display());
        assert_eq!(stdout_within(sh(&script), Duration::from_millis(500)), Outcome::TimedOut);
        let pid = read_pid(&path);
        assert!(ends_soon(pid), "grandchild {pid} outlived the run");
    }

    /// The grandchild of a child that hangs dies with the group at the
    /// limit.
    #[test]
    fn the_group_dies_when_the_child_hangs() {
        let path = pid_file("hung");
        let script = format!("({}; :) & wait", holder(&path));
        assert_eq!(stdout_within(sh(&script), Duration::from_millis(500)), Outcome::TimedOut);
        let pid = read_pid(&path);
        assert!(ends_soon(pid), "grandchild {pid} outlived the run");
    }
}
