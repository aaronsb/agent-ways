//! Internal module surface for `attend-chat`.
//!
//! The binary stays the primary entry point; `lib.rs` exists only so
//! integration tests under `tests/` can link against the crate and
//! exercise `signal` + `watcher` end-to-end without re-declaring them
//! via `#[path]`.

pub mod app;
pub mod chip;
pub mod groups;
pub mod legend;
pub mod sessions;
pub mod signal;
pub mod text_layout;
pub mod watcher;
