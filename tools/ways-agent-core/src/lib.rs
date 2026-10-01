//! What the ways agent (ADR-502) and its clients share: engine profiles and
//! the key store. The crate carries no network code, so the `ways` hook can
//! link it without TLS; the provider clients live in `ways-agent`.

pub mod keys;
pub mod profile;
