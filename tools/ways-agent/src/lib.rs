//! The ways agent (ADR-502): one resident process per user that judges whether
//! a way the matcher picked is relevant to the conversation (ADR-196) and is
//! the one process that reads the provider key.
//!
//! The shared pieces (profiles, the key store, the judge's question, the
//! socket protocol and its client) live in `ways-agent-core`, which the hook
//! links; this crate adds the provider HTTP clients and the server.

pub use ways_agent_core::{client, cost, judge, keys, profile, protocol};

pub mod net;
pub mod report;
#[cfg(unix)]
pub mod server;
