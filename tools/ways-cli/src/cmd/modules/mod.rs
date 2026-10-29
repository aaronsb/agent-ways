//! Mods, sensors and themes as items (ADR-516). One function per family
//! ([`mods`], [`sensors`], [`themes`]) builds each item's card: its
//! descriptor ([`agent_settings::Item`]) and the family's own rows. Paths
//! are kept whole; the settings screens write them as they write every path.

pub mod mods;
pub mod sensors;
pub mod themes;

/// A family row, as the card holds it.
fn row(label: &str, value: impl Into<String>) -> (String, String) {
    (label.to_string(), value.into())
}
