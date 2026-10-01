//! The ways agent (ADR-502): one resident process per user that judges whether
//! a way the matcher picked is relevant to the conversation (ADR-196) and is
//! the one process that reads the provider key.
//!
//! The library carries what both sides share: engine profiles and the key
//! store. The `net` feature adds the provider HTTP clients; the hook links the
//! crate without it.

pub mod keys;
pub mod profile;

#[cfg(feature = "net")]
pub mod net;
