//! The hook's side of the socket (ADR-502 §1, §4, §8).
//!
//! A hook connects, sends one request, and reads one reply. When no agent is
//! listening it starts one and waits briefly. A start that fails is not
//! retried for a while, so a broken agent costs one wait, not one per prompt.
//! An agent built against a different version of this crate is asked to stop
//! after it answers a judge request, so the next prompt starts a current one.
//! Every failure comes back as a fallback reason; the hook then keeps the
//! matcher's decision.

use std::path::PathBuf;
use std::time::Duration;

use crate::protocol::{Envelope, Reply, ReplyEnvelope, Request, PROTOCOL};

const BIN: &str = "ways-agent";

/// How long a hook waits for a freshly started agent to listen.
const START_WAIT: Duration = Duration::from_millis(1500);

/// After a failed start, hooks do not try again for this long.
const START_BACKOFF: Duration = Duration::from_secs(300);

/// The `ways-agent` binary: beside the running executable, else in the
/// projected `~/.claude/bin`, else on `PATH`.
pub fn agent_binary() -> Option<PathBuf> {
    let name = format!("{BIN}{}", std::env::consts::EXE_SUFFIX);
    let beside = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|d| d.join(&name)));
    let projected = Some(ways_core::paths::projected_bin_root().join(&name));
    let on_path = std::env::var_os("PATH")
        .and_then(|paths| std::env::split_paths(&paths).map(|d| d.join(&name)).find(|p| p.is_file()));
    [beside, projected, on_path].into_iter().flatten().find(|p| p.is_file())
}

/// Forgets a recorded start failure, so the next call tries again at once.
/// An explicit `ways agent load` calls this; hooks keep the back-off.
pub fn clear_start_backoff() {
    let _ = std::fs::remove_file(ways_core::paths::state_root().join("agent").join("start-failed"));
}

/// Sends `request` and returns the reply, or a fallback reason. With `start`,
/// a missing agent is started first.
pub fn call(request: Request, timeout: Duration, start: bool) -> Result<Reply, String> {
    imp::call(request, timeout, start)
}

#[cfg(unix)]
mod imp {
    use super::*;
    use std::io::{BufRead, BufReader, ErrorKind, Write};
    use std::os::unix::net::UnixStream;
    use std::os::unix::process::CommandExt;
    use std::time::{Instant, SystemTime};

    pub fn call(request: Request, timeout: Duration, start: bool) -> Result<Reply, String> {
        let sock = crate::protocol::socket_path();
        let stream = match UnixStream::connect(&sock) {
            Ok(s) => s,
            // Only a missing or dead agent is started; any other error (a
            // permission problem, say) would not be cured by a new one.
            Err(e) if start && matches!(e.kind(), ErrorKind::NotFound | ErrorKind::ConnectionRefused) => {
                start_agent(&sock)?
            }
            Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::ConnectionRefused) => {
                return Err("agent_absent".to_string())
            }
            Err(e) => return Err(format!("connect: {e}")),
        };
        if !crate::protocol::trusted_socket(&sock) {
            return Err("agent_untrusted: the socket or its directory is not this user's alone".to_string());
        }
        let judging = matches!(request, Request::Judge(_));
        let envelope = exchange(stream, &Envelope { protocol: PROTOCOL, request }, timeout)?;
        if judging {
            retire_if_foreign(&envelope.agent.core);
        }
        Ok(envelope.reply)
    }

    fn backoff_marker() -> PathBuf {
        ways_core::paths::state_root().join("agent").join("start-failed")
    }

    fn start_agent(sock: &std::path::Path) -> Result<UnixStream, String> {
        let marker = backoff_marker();
        let recent_failure = std::fs::metadata(&marker)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age < START_BACKOFF);
        if recent_failure {
            return Err("agent_start_backoff".to_string());
        }
        let result = spawn_and_connect(sock);
        if result.is_err() {
            if let Some(dir) = marker.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&marker, b"");
        } else {
            let _ = std::fs::remove_file(&marker);
        }
        result
    }

    fn spawn_and_connect(sock: &std::path::Path) -> Result<UnixStream, String> {
        let bin = agent_binary().ok_or_else(|| "agent_missing".to_string())?;
        let mut cmd = std::process::Command::new(bin);
        cmd.arg("serve")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            // Pin no project directory for the agent's lifetime.
            .current_dir("/")
            // The agent serves every session, so it takes none of this one's
            // environment beyond what locates its files. Provider key
            // variables are left out on purpose: the first session's key must
            // not become every session's. The agent reads the key file.
            .env_clear()
            .envs(std::env::vars_os().filter(|(k, _)| {
                let k = k.to_string_lossy();
                matches!(k.as_ref(), "HOME" | "PATH" | "USER" | "LOGNAME" | "LANG" | "TMPDIR" | "WAYS_AGENT_SOCK")
                    || k.starts_with("XDG_")
                    || k.starts_with("LC_")
                    // The HTTP client honours these; behind a proxy the agent
                    // reaches no provider without them.
                    || matches!(k.to_ascii_uppercase().as_str(), "ALL_PROXY" | "HTTPS_PROXY" | "HTTP_PROXY" | "NO_PROXY")
            }));
        // SAFETY: setsid in the child before exec only detaches it from the
        // hook's session and terminal; it allocates nothing.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        let mut child = cmd.spawn().map_err(|e| format!("agent_start: {e}"))?;
        let begun = Instant::now();
        loop {
            if let Ok(s) = UnixStream::connect(sock) {
                // Reap the child when it exits, so a long-lived client leaves
                // no zombie behind.
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                return Ok(s);
            }
            if let Ok(Some(status)) = child.try_wait() {
                // It exited without listening. An agent that lost the start
                // race exits cleanly, and the winner may be up by now.
                return UnixStream::connect(sock).map_err(|_| format!("agent_start: exited with {status}"));
            }
            if begun.elapsed() >= START_WAIT {
                return Err("agent_start: no socket after 1.5 s".to_string());
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    /// Asks an agent built against a different version of the shared crate to
    /// stop, so the next prompt starts a current one. The agent's own binary
    /// watch covers an update at the same version.
    fn retire_if_foreign(agent_core: &str) {
        if agent_core != crate::protocol::CORE_VERSION {
            let sock = crate::protocol::socket_path();
            if let Ok(stream) = UnixStream::connect(&sock) {
                let _ = exchange(stream, &Envelope { protocol: PROTOCOL, request: Request::Shutdown }, Duration::from_secs(1));
            }
        }
    }

    fn exchange(stream: UnixStream, envelope: &Envelope, timeout: Duration) -> Result<ReplyEnvelope, String> {
        let io = |e: std::io::Error| {
            if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) {
                "deadline".to_string()
            } else {
                format!("io: {e}")
            }
        };
        stream.set_read_timeout(Some(timeout)).map_err(io)?;
        stream.set_write_timeout(Some(timeout)).map_err(io)?;
        let mut line = serde_json::to_string(envelope).map_err(|e| format!("encode: {e}"))?;
        line.push('\n');
        (&stream).write_all(line.as_bytes()).map_err(io)?;
        let mut reply = String::new();
        BufReader::new(&stream).read_line(&mut reply).map_err(io)?;
        if reply.is_empty() {
            return Err("agent_closed".to_string());
        }
        serde_json::from_str(&reply).map_err(|e| format!("decode: {e}"))
    }
}

#[cfg(not(unix))]
mod imp {
    use super::*;

    pub fn call(_request: Request, _timeout: Duration, _start: bool) -> Result<Reply, String> {
        Err("unsupported_platform".to_string())
    }
}
