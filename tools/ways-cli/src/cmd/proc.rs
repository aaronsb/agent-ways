//! Running a child process under a time limit. Used for the `claude` CLI
//! calls of a repair ([`super::mcp_register`], [`super::attend_mod`]) and for
//! way macros ([`super::show`]).

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Why [`output_within`] gave no stdout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProcError {
    /// The command could not be started, or could not be waited on.
    Spawn(String),
    /// It ran past its limit and was killed.
    TimedOut(Duration),
    /// It exited non-zero: its stderr, or its exit code when stderr was empty.
    Failed(String),
}

impl std::fmt::Display for ProcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(e) | Self::Failed(e) => f.write_str(e),
            Self::TimedOut(limit) => write!(f, "timed out after {}s", limit.as_secs_f64()),
        }
    }
}

/// Run `cmd` with stdin closed and its output captured, killing it once
/// `limit` has passed: on Unix its whole process group, so what it started
/// dies with it. Its stdout on success.
///
/// The child runs in a process group of its own. A Ctrl-C at the terminal
/// then stops `ways` and not the child, which finishes on its own: for a
/// `claude` call, one short local edit of Claude Code's plugin or MCP config
/// that a signal mid-write could leave half done. The cost is the other
/// side: when `ways` itself is killed before `limit` (Claude Code timing out
/// or cancelling the hook that runs a macro), the child's group is not
/// signalled and the child runs on as an orphan. On Windows only the direct
/// child is killed at the limit.
pub(crate) fn output_within(mut cmd: Command, limit: Duration) -> Result<String, ProcError> {
    use std::io::Read;
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| ProcError::Spawn(e.to_string()))?;
    // Read both pipes on threads, so a child that fills one never blocks.
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            buf
        })
    };
    let out = drain(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let err = drain(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let deadline = Instant::now() + limit;
    let status = loop {
        match child.try_wait().map_err(|e| ProcError::Spawn(e.to_string()))? {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                #[cfg(unix)]
                let _ = Command::new("kill")
                    .args(["-KILL", "--", &format!("-{}", child.id())])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
                let _ = child.kill();
                let _ = child.wait();
                return Err(ProcError::TimedOut(limit));
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    if status.success() {
        Ok(String::from_utf8_lossy(&stdout).into_owned())
    } else {
        let e = String::from_utf8_lossy(&stderr).trim().to_string();
        Err(ProcError::Failed(if e.is_empty() { format!("exit {}", status.code().unwrap_or(-1)) } else { e }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_hung_call_is_killed_at_its_limit() {
        let mut slow = Command::new("sh");
        slow.args(["-c", "sleep 5"]);
        let started = Instant::now();
        let limit = Duration::from_millis(200);
        assert_eq!(output_within(slow, limit), Err(ProcError::TimedOut(limit)));
        assert!(started.elapsed() < Duration::from_secs(3), "killed, not waited out");
        let mut fine = Command::new("sh");
        fine.args(["-c", "echo out; echo err >&2"]);
        assert_eq!(output_within(fine, Duration::from_secs(5)).unwrap(), "out\n");
        let mut failing = Command::new("sh");
        failing.args(["-c", "echo why >&2; exit 3"]);
        assert_eq!(output_within(failing, Duration::from_secs(5)), Err(ProcError::Failed("why".into())));
        assert_eq!(ProcError::TimedOut(Duration::from_secs(60)).to_string(), "timed out after 60s");
    }

    /// What the timed-out call started dies with it: its process group is
    /// killed, not only the direct child.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_timeout_kills_what_the_call_started() {
        let pidfile = std::env::temp_dir().join(format!("ways-output-within-{}", std::process::id()));
        let mut slow = Command::new("sh");
        slow.args(["-c", &format!("sleep 30 & echo $! > {}; wait", pidfile.display())]);
        assert!(matches!(output_within(slow, Duration::from_millis(300)), Err(ProcError::TimedOut(_))));
        let pid = std::fs::read_to_string(&pidfile).unwrap().trim().to_string();
        std::fs::remove_file(&pidfile).ok();
        // Reaped by init once killed; give the kernel a moment.
        let alive = || std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| !s.contains(") Z "));
        for _ in 0..50 {
            if !alive() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!alive(), "the grandchild {pid} outlived the timeout");
    }
}
