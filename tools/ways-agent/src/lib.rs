//! The ways agent (ADR-502): one resident process per user that judges whether
//! a way the matcher picked is relevant to the conversation (ADR-196) and is
//! the one process that reads the provider key.
//!
//! The shared pieces (profiles, the key store) live in `ways-agent-core`,
//! which the hook links; this crate adds the provider HTTP clients.

pub use ways_agent_core::{keys, profile};

pub mod net;
