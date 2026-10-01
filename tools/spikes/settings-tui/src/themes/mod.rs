//! The theme engine: a theme is ten RGB slots; every role the TUI draws is
//! derived from them. It knows nothing of the TUI, so it moves to the shared
//! theme crate as it is.

mod bundled;
mod depth;
mod derive;
mod model;
mod oklab;
mod text;

#[cfg(test)]
mod tests;

pub use bundled::{BUNDLED, ThemeSet, Source};
pub use depth::{color, ColorDepth};
pub use derive::{contrast, text_on, Roles, MIN_DISTINCT, MIN_TEXT};
pub use oklab::delta_e;
pub use model::{Background, Kind, Rgb, Slots, Theme};
pub use text::{parse, to_toml};
