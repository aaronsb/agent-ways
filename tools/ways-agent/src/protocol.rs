//! The socket protocol between a hook and the ways agent (ADR-502 §1-4).
//!
//! One JSON object per line, one request and one reply per connection. Every
//! request carries the protocol number; the agent answers a different number
//! with an error, and the hook falls back.

use std::path::PathBuf;

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
fn user_id() -> u32 {
    // SAFETY: getuid has no preconditions and cannot fail.
    unsafe { libc::getuid() }
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

        let reply = Reply::Fallback { reason: "no_key".into(), latency_ms: 0 };
        let line = serde_json::to_string(&reply).unwrap();
        assert!(line.contains("\"kind\":\"fallback\""));
        assert_eq!(serde_json::from_str::<Reply>(&line).unwrap(), reply);
    }
}
