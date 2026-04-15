//! Per-subcommand implementations.
//!
//! Extracted from `main.rs` in issue #51. Each submodule owns the
//! `cmd_*` function plus any helpers that are specific to that
//! command. Shared helpers used across multiple commands live in
//! `crate::util`. The top-level `fn main()` is now a thin dispatcher
//! that matches on argv and forwards to these modules.

pub(crate) mod cleanup;
pub(crate) mod config_cmd;
pub(crate) mod focus;
pub(crate) mod inbox;
pub(crate) mod peers;
pub(crate) mod permissions;
pub(crate) mod run;
pub(crate) mod scene;
pub(crate) mod send;
pub(crate) mod status;
pub(crate) mod tune;
