//! The theme engine: a theme is ten RGB slots; every role the TUI draws is
//! derived from them. Self-contained, so it is testable apart from the rest
//! of the crate (`tests/themes.rs` includes it by path).

// Phase 1 ships the engine before the UI calls it.
#![allow(dead_code, unused_imports)]

mod bundled;
mod depth;
mod derive;
mod model;
mod text;

#[cfg(test)]
mod tests;

pub use bundled::{BUNDLED, ThemeSet, Source};
pub use depth::{color, ColorDepth};
pub use derive::{contrast, Roles, SegPair};
pub use model::{Background, Kind, Overrides, Rgb, Slots, Theme};
pub use text::{parse, to_toml, validate, ThemeError};
