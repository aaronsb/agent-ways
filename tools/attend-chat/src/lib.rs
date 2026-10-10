//! Internal module surface for `attend-chat`.
//!
//! The binary stays the primary entry point; `lib.rs` exists so
//! integration tests under `tests/` can link against the crate: the
//! watcher round trip, and the screen's golden frames and keys through
//! `agent-tui`'s test kit.

pub mod app;
pub mod attach;
pub mod chip;
pub mod consumers;
pub mod grammar;
pub mod groups;
pub mod helper;
pub mod legend;
pub mod peers;
pub mod sessions;
pub mod settings;
pub mod signal;
pub mod slash;
pub mod tabs;
#[cfg(test)]
mod test_dir;
pub mod theme;
pub mod watcher;
