//! The hook's side of the socket (ADR-502 §1, §8).
//!
//! A hook connects, sends one request, and reads one reply. When no agent is
//! listening it starts one and retries briefly. Every failure comes back as a
//! fallback reason; the hook then keeps the matcher's decision.

use std::path::PathBuf;
use std::time::Duration;

use crate::protocol::{Envelope, Reply, Request, PROTOCOL};

const BIN: &str = "ways-agent";

/// How long a hook waits for a freshly started agent to listen.
const START_WAIT: Duration = Duration::from_millis(1500);

/// The `ways-agent` binary: beside the running executable, else in the
/// projected `~/.claude/bin`, else on `PATH`.
pub fn agent_binary() -> Option<PathBuf> {
    let name = format!("{BIN}{}", std::env::consts::EXE_SUFFIX);
    let beside = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|d| d.join(&name)));
    let projected = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude").join("bin").join(&name));
    let on_path = std::env::var_os("PATH")
        .and_then(|paths| std::env::split_paths(&paths).map(|d| d.join(&name)).find(|p| p.is_file()));
    [beside, projected, on_path].into_iter().flatten().find(|p| p.is_file())
}

/// Sends `request` and returns the reply, or a fallback reason. With `start`,
/// a missing agent is started first.
pub fn call(request: Request, timeout: Duration, start: bool) -> Result<Reply, String> {
    imp::call(request, timeout, start)
}

#[cfg(unix)]
mod imp {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::os::unix::process::CommandExt;
    use std::time::Instant;

    pub fn call(request: Request, timeout: Duration, start: bool) -> Result<Reply, String> {
        let sock = crate::protocol::socket_path();
        let stream = match UnixStream::connect(&sock) {
            Ok(s) => s,
            Err(_) if start => {
                spawn_agent()?;
                connect_within(&sock, START_WAIT)?
            }
            Err(_) => return Err("agent_absent".to_string()),
        };
        exchange(stream, &Envelope { protocol: PROTOCOL, request }, timeout)
    }

    fn connect_within(sock: &std::path::Path, wait: Duration) -> Result<UnixStream, String> {
        let begun = Instant::now();
        loop {
            match UnixStream::connect(sock) {
                Ok(s) => return Ok(s),
                Err(_) if begun.elapsed() < wait => std::thread::sleep(Duration::from_millis(15)),
                Err(e) => return Err(format!("agent_start: {e}")),
            }
        }
    }

    fn spawn_agent() -> Result<(), String> {
        let bin = agent_binary().ok_or_else(|| "agent_missing".to_string())?;
        std::process::Command::new(bin)
            .arg("serve")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            // Its own process group: the agent outlives the hook and is not
            // signalled with it.
            .process_group(0)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("agent_start: {e}"))
    }

    fn exchange(stream: UnixStream, envelope: &Envelope, timeout: Duration) -> Result<Reply, String> {
        let io = |e: std::io::Error| {
            if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) {
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
