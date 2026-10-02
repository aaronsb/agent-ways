//! What the ways agent (ADR-502) and its clients share: engine profiles, the
//! key store, the judge's question, and the socket protocol with its client.
//! The crate carries no network code, so the `ways` hook can link it without
//! TLS; the provider clients and the server live in `ways-agent`.

pub mod client;
pub mod judge;
pub mod keys;
pub mod profile;
pub mod protocol;
pub mod settings;
