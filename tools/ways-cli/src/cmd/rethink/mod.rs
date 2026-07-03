//! Interactive session replay — animate `ways list` state across a session's history.
//!
//! Reconstructs the progressive disclosure timeline from events.jsonl,
//! building cumulative frames at each epoch. Renders each frame using
//! the same visual format as `ways list`, with interactive controls.
//!
//! The command is split into focused submodules:
//! - [`model`] — the replay data types (`WayEvent`, `ActiveWay`, `Frame`, `Player`, …).
//! - [`scope`] — project-scope resolution and matching.
//! - [`frames`] — frame reconstruction and event/token loading.
//! - [`sessions`] — session enumeration, listing, and the interactive picker.
//! - [`run`] — the entry points (`run`, `run_live`) and the TUI/live loops.
//! - [`keys`] — key handling and cursor-anchor navigation (tui).
//! - [`drilldown`] — the why-fired drill-down (tui).
//! - [`layout`] — screen composition, headers, status bar, and terminal fitting.

mod frames;
mod model;
mod scope;
mod sessions;

mod run;

#[cfg(feature = "tui")]
mod drilldown;
#[cfg(feature = "tui")]
mod keys;
mod layout;

// Re-export the surface referenced as `crate::cmd::rethink::X` by main.rs,
// cmd/introspect.rs, and cmd/rethink_dump.rs. The module boundaries above are an
// internal reorganization; these paths keep working unchanged.
pub(crate) use frames::{find_session_project, load_session_events, reconstruct_frames};
pub(crate) use model::Frame;
pub(crate) use run::{run, run_live};
pub(crate) use scope::{project_matches, resolve_project_scope};
pub(crate) use sessions::gather_sessions;
