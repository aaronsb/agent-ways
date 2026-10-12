//! Shared terminal formatting and utilities for agent-ways tools.
//!
//! Provides ANSI-aware table rendering, banner display, help formatting,
//! and permission matching (ADR-116).

mod banner;
pub mod permissions;
mod pipe;
mod table;
pub mod when;
mod width;

pub use banner::{Banner, GRADIENT_CORAL, GRADIENT_TEAL};
pub use pipe::exit_quietly_on_closed_pipe;
pub use table::{Align, Table, terminal_width};
pub use width::{clip_visible, fit_visible, pad_visible, truncate_visible, visible_len};
pub use when::{compact_time, compact_time_with_offset};
