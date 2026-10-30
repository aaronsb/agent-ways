//! Themes, mods, sensors, ways and judge profiles as items (ADR-516). One
//! function per family builds each item: its descriptor
//! ([`agent_settings::item::Item`]) and the family's own fields, in an
//! [`agent_settings::item::Entry`]. The CLI prints the entry and the
//! settings screens draw a card from it (`settings::tui::card`). Paths are
//! kept whole; the screens write them as they write every path.

pub mod mods;
pub mod profiles;
pub mod sensors;
pub mod themes;
pub mod ways;
