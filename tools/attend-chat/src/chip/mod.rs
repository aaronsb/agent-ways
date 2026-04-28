//! Sender-chip module: rendering, identity registry, and `@`-routing.
//!
//! Split into three submodules around independent concerns so each
//! seam can be reviewed and tested in isolation. Public symbols are
//! re-exported here so `crate::chip::*` keeps the same import surface
//! the rest of the crate already uses.
//!
//! - `render` — `chip_for`, `ChipInfo`, `color_for`, `CHIP_WIDTH`.
//!   Pure display: takes a wire `from`/`project`/`cwd` triple, returns
//!   the colored bordered chip the message list draws.
//! - `registry` — `KnownIdentity`, `known_identities`,
//!   `known_identities_with_liveness`. Builds the dedup'd list of
//!   peers the legend / completion / `@`-resolver consume.
//! - `routing` — `resolve_nickname`. Turns an `@Nickname` into a
//!   routable cwd, with case-insensitive exact match plus a
//!   Levenshtein fuzzy fallback.

pub mod registry;
pub mod render;
pub mod routing;

pub use registry::{known_identities, known_identities_with_liveness, KnownIdentity};
pub use render::{chip_for, color_for, ChipInfo, CHIP_WIDTH};
pub use routing::resolve_nickname;
