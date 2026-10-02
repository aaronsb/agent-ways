//! The agent-ways theme engine (ADR-504 §4-§6).
//!
//! Every surface that prints colour draws it from here: terminal colour
//! depth and `NO_COLOR`, theme files in the dotfiles palette structure,
//! roles derived from a theme's slots with contrast and distinctness
//! floors, and the outputs that turn roles into terminal styling: ANSI SGR
//! strings always, ratatui styles with the `ratatui` feature.
//!
//! Plain output uses the process painter:
//!
//! ```
//! use agent_theme::{paint, Role, Style};
//! let line = format!("{} {}", paint(Style::new().bold(), "Status"), paint(Role::Ok, "ok"));
//! # let _ = line;
//! ```
//!
//! A raw SGR escape anywhere else in the workspace fails the `sgr_lint`
//! test in this crate.
//!
//! Dependency direction: `agent-identity` depends on this crate for colour
//! depth; this crate depends on nothing of agent-ways, except that the
//! `settings` feature declares the `theme` settings section against
//! `agent-settings`' schema types.

mod bundled;
mod color;
mod depth;
mod derive;
mod model;
mod oklab;
mod paint;
#[cfg(feature = "ratatui")]
pub mod ratatui;
#[cfg(feature = "settings")]
pub mod settings;
mod text;

#[cfg(test)]
mod tests;

pub use bundled::{user_dir, Source, ThemeSet, BUNDLED, EXTENSIONS, TERMINAL};
pub use color::{index_rgb, nearest_16, nearest_256, Color, ANSI16_RGB};
pub use depth::ColorDepth;
pub use derive::{contrast, text_on, Roles, SegPair, MIN_DISTINCT, MIN_MUTED, MIN_TEXT};
pub use model::{Background, Kind, Overrides, Rgb, Slots, Theme};
pub use oklab::delta_e;
pub use paint::{install, pair, paint, painter, reset, scoped, sgr, Ink, Painter, Resolved, Role, Scope, Style, RESET};
pub use text::{parse, to_text, validate, ThemeError};
