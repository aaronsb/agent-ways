//! The ways agent (ADR-502): one resident process per user that judges whether
//! a way the matcher picked is relevant to the conversation (ADR-196) and is
//! the one process that reads the provider key.
//!
//! The library carries what both sides share: engine profiles, the key store,
//! the judge's question and the socket protocol with its client. The `net`
//! feature adds the provider HTTP clients and the server; the hook links the
//! crate without it.

pub mod client;
pub mod judge;
pub mod keys;
pub mod profile;
pub mod protocol;

#[cfg(feature = "net")]
pub mod net;
#[cfg(all(feature = "net", unix))]
pub mod server;
