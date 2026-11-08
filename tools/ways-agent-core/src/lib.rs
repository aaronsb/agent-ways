// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! What the ways agent (ADR-502) and its clients share: engine profiles, the
//! session's engine (ADR-708), the key store, the judge's question and its
//! reply parser, and the socket protocol with its client.
//! The crate carries no network code, so the `ways` hook can link it without
//! TLS; the provider clients and the server live in `ways-agent`.

pub mod client;
pub mod cost;
pub mod engine;
pub mod judge;
pub mod models;
pub mod keys;
pub mod live;
pub mod profile;
pub mod protocol;
pub mod settings;
pub mod spend;
