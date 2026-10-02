//! Display-layer identity for agent-ways tools.
//!
//! Given a working directory (for a claude) or a username (for a
//! human), produces a stable nickname, color, and style. Pure
//! derivation — no network, no state; the one filesystem write is
//! [`signal::write_signal_file`], the wire file both writers share. Callers render
//! with whatever stack they have (ratatui, raw ANSI); this crate does not
//! pick one.
//!
//! Why a separate crate: attend and attend-chat both need the same
//! mapping so a claude sees the same nickname in its own banner that
//! its peers see when the peer renders a signal from it. Duplicating
//! the derivation risked drift the moment either side tweaked a
//! constant.
//!
//! Non-goals:
//! - Identity does **not** ride the wire. Signals keep carrying
//!   `claude:<uuid>` / `external:<user>` / cwd exactly as before.
//!   Receivers derive the display identity locally. Claude is never
//!   forced to use or publish a nickname — this is a view layer.
//! - No opinion on how focus groups form or are discovered. That lives
//!   in ADR-118 and the consumer crates.

pub mod ansi;
pub mod groups;
pub mod identity;
pub mod names;
pub mod palette;
pub mod signal;

pub use groups::{Group, GLYPHS};
pub use identity::{cwd_basename, sanitize_id_component, signal_filename, Identity};
pub use signal::{is_valid_signal_id, parse_signal, ParsedSignal};
pub use palette::{
    categorical, is_rich, resolve, PaletteEntry, Resolved, Style, BASIC_PALETTE, CATEGORICAL, RICH_PALETTE,
};
