//! The socket protocol between a hook and the ways agent (ADR-502 §1-4).
//!
//! One JSON object per line, one request and one reply per connection. Every
//! request carries the protocol number; the agent answers a different number
//! with an error, and the hook falls back.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::judge::{Candidate, Turn};
use crate::profile::{Mode, Provider};

pub const PROTOCOL: u32 = 1;

/// A request, wrapped with the protocol number.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub protocol: u32,
    #[serde(flatten)]
    pub request: Request,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Judge every candidate against the turns.
    Judge(JudgeRequest),
    Status,
    Shutdown,
}

/// Self-contained: the agent keeps no per-session state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JudgeRequest {
    pub session: String,
    /// The agent tool asking, e.g. `claude-code`.
    pub tool: String,
    pub turns: Vec<Turn>,
    pub candidates: Vec<Candidate>,
}

/// Which agent answered: its version and the binary it runs from. A client
/// that would start a different binary retires this agent (ADR-502 §4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentId {
    pub version: String,
    pub exe: String,
}

/// A reply, wrapped with the identity of the agent that sent it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplyEnvelope {
    pub agent: AgentId,
    #[serde(flatten)]
    pub reply: Reply,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reply {
    Judged(Judged),
    /// The gate did not judge; the hook keeps the matcher's decision.
    Fallback { reason: String, latency_ms: u64 },
    Status(Status),
    Ok,
    Error { message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Judged {
    pub engine: String,
    pub provider: Provider,
    pub model: String,
    pub mode: Mode,
    pub threshold: f64,
    pub verdicts: Vec<Verdict>,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Verdict {
    pub id: String,
    pub p_yes: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub version: String,
    pub pid: u32,
    pub uptime_s: u64,
    pub engine: Option<String>,
    pub model: Option<String>,
    pub mode: Option<Mode>,
    pub requests: u64,
    pub judged: u64,
    /// Fallbacks by reason.
    pub fallbacks: std::collections::BTreeMap<String, u64>,
    pub in_flight: usize,
    pub concurrency: usize,
    /// Judge latency over the most recent calls, in milliseconds.
    pub latency_p50_ms: Option<u64>,
    pub latency_p95_ms: Option<u64>,
}

/// The agent's socket: `$WAYS_AGENT_SOCK`, else
/// `$XDG_RUNTIME_DIR/agent-ways.sock`, else a per-user directory under the
/// system temp directory.
pub fn socket_path() -> PathBuf {
    if let Some(p) = std::env::var_os("WAYS_AGENT_SOCK").filter(|v| !v.is_empty()) {
        return PathBuf::from(p);
    }
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|d| d.is_absolute()) {
        return dir.join("agent-ways.sock");
    }
    std::env::temp_dir().join(format!("agent-ways-{}", user_id())).join("agent.sock")
}

#[cfg(unix)]
pub(crate) fn user_id() -> u32 {
    // SAFETY: getuid has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}

/// Makes the socket's directory safe to serve from, or says why it is not.
/// A missing directory is created mode 0700. An existing one must be a real
/// directory (not a symlink), owned by this user, with no group or other
/// access: anyone else able to write there could replace the socket and read
/// the conversation turns hooks send.
#[cfg(unix)]
pub fn secure_dir(dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    if !dir.exists() {
        if let Some(parent) = dir.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
        }
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(dir)
            .map_err(|e| format!("creating {}: {e}", dir.display()))?;
    }
    let meta = std::fs::symlink_metadata(dir).map_err(|e| format!("reading {}: {e}", dir.display()))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(format!("{} is not a plain directory", dir.display()));
    }
    if meta.uid() != user_id() {
        return Err(format!("{} is owned by uid {}, not this user", dir.display(), meta.uid()));
    }
    if meta.mode() & 0o077 != 0 {
        return Err(format!("{} is mode {:o}; others can reach the socket", dir.display(), meta.mode() & 0o777));
    }
    Ok(())
}

/// True when the socket at `sock` is one this user's agent made: a socket,
/// owned by this user, in a directory [`secure_dir`] accepts.
#[cfg(unix)]
pub fn trusted_socket(sock: &Path) -> bool {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let Ok(meta) = std::fs::symlink_metadata(sock) else { return false };
    meta.file_type().is_socket()
        && meta.uid() == user_id()
        && sock.parent().is_some_and(|dir| {
            std::fs::symlink_metadata(dir)
                .is_ok_and(|d| d.is_dir() && d.uid() == user_id() && d.mode() & 0o077 == 0)
        })
}

#[cfg(not(unix))]
fn user_id() -> u32 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::judge::Role;

    #[test]
    fn requests_and_replies_round_trip_with_tags() {
        let req = Envelope {
            protocol: PROTOCOL,
            request: Request::Judge(JudgeRequest {
                session: "s".into(),
                tool: "claude-code".into(),
                turns: vec![Turn { role: Role::User, text: "hi".into() }],
                candidates: vec![Candidate { id: "a/b".into(), text: "a › b\nd".into() }],
            }),
        };
        let line = serde_json::to_string(&req).unwrap();
        assert!(line.contains("\"op\":\"judge\"") && line.contains("\"protocol\":1"));
        assert_eq!(serde_json::from_str::<Envelope>(&line).unwrap(), req);

        let status: Envelope = serde_json::from_str(r#"{"protocol":1,"op":"status"}"#).unwrap();
        assert_eq!(status.request, Request::Status);

        let reply = ReplyEnvelope {
            agent: AgentId { version: "0.1.0".into(), exe: "/x/ways-agent".into() },
            reply: Reply::Fallback { reason: "no_key".into(), latency_ms: 0 },
        };
        let line = serde_json::to_string(&reply).unwrap();
        assert!(line.contains("\"kind\":\"fallback\"") && line.contains("\"exe\":\"/x/ways-agent\""));
        assert_eq!(serde_json::from_str::<ReplyEnvelope>(&line).unwrap(), reply);
    }

    #[cfg(unix)]
    #[test]
    fn secure_dir_creates_0700_and_refuses_loose_or_linked_dirs() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!("ways-agent-sock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("run");
        secure_dir(&dir).unwrap();
        assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(secure_dir(&dir).is_err());
        std::os::unix::fs::symlink(&dir, base.join("link")).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(secure_dir(&base.join("link")).is_err());
        assert!(!trusted_socket(&dir.join("agent.sock")));
        std::fs::remove_dir_all(&base).unwrap();
    }
}
