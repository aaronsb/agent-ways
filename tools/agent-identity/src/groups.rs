//! Display identity for focus groups.
//!
//! Parallel to `Identity` for agents: take a group name, produce a
//! stable glyph + color + style. Different derivation *kind* (glyph
//! pool, not nickname pool) but same shape so call sites render
//! groups and agents with the same primitives.
//!
//! Like agents, everything is receiver-side. Groups are identified
//! on the wire by the directory name (`@groupname/`); the glyph is a
//! display convenience, never quoted or typed. Users still address
//! groups by name (`#deploy`), not by glyph.

use crate::identity::fnv1a_64;
use crate::palette::{resolve, PaletteEntry, Resolved, Style, TermCaps};

/// Curated glyph pool — ~20 distinctive Unicode symbols chosen for
/// broad font coverage. Avoiding anything that commonly degrades to
/// `?` or U+FFFD on default terminal fonts: no emoji, no rare script
/// glyphs, no ASCII-art double-widths.
///
/// If a specific glyph proves unreadable in the wild, swap it rather
/// than adding a new one — the hash-to-index keeps seats stable.
pub const GLYPHS: &[char] = &[
    '●', '○', '◆', '◇', '■', '□', '▲', '△', '▼', '▽',
    '★', '☆', '♦', '♠', '♣', '♥', '⬢', '⬡', '✦', '✧',
];

/// Everything a renderer needs to paint a group.
#[derive(Clone, Debug)]
pub struct Group {
    /// The group's name as it appears in the signal bus
    /// (`@<name>/`). Case sensitive — `deploy` and `Deploy` are
    /// different groups. Users address with `#<name>`.
    pub name: String,
    /// Single-char visual marker. Users never type it; it's purely
    /// a glance-cue.
    pub glyph: char,
    pub palette: PaletteEntry,
    pub style: Style,
    /// Seed available for callers that want to derive their own
    /// per-group state (e.g., unread counts) consistently.
    pub seed: u64,
}

impl Group {
    /// Derive a group's display identity from its name under the
    /// given terminal capability.
    ///
    /// Same hash as `Identity::for_user`, but namespaced so a group
    /// named "aaron" doesn't land on the same palette entry as a
    /// human user named "aaron".
    pub fn for_name(name: &str, caps: TermCaps) -> Self {
        let key = format!("group:{name}");
        let seed = fnv1a_64(key.as_bytes());
        let glyph = GLYPHS[(seed as usize) % GLYPHS.len()];
        // Stir before resolving so glyph and color aren't trivially
        // correlated — same trick as `Identity::for_key`. Constant is
        // φ·2^64 (golden-ratio hash), matching the mixing pattern in
        // `boost::hash_combine` and the Rust standard-library hasher.
        let Resolved { entry, style } = resolve(seed ^ 0x9e37_79b9_7f4a_7c15, caps);
        Group {
            name: name.to_string(),
            glyph,
            palette: entry,
            style,
            seed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_name_same_group() {
        let a = Group::for_name("deploy", TermCaps::Rich);
        let b = Group::for_name("deploy", TermCaps::Rich);
        assert_eq!(a.glyph, b.glyph);
        assert_eq!(a.palette, b.palette);
        assert_eq!(a.style, b.style);
        assert_eq!(a.seed, b.seed);
    }

    #[test]
    fn different_names_differ() {
        let a = Group::for_name("deploy", TermCaps::Rich);
        let b = Group::for_name("infra", TermCaps::Rich);
        assert_ne!(a.seed, b.seed);
    }

    #[test]
    fn group_namespace_disjoint_from_user() {
        // A group called "aaron" and a user called "aaron" must
        // resolve to different palette seats — the `group:` prefix
        // in the hash input is what guarantees this.
        use crate::identity::Identity;
        let g = Group::for_name("aaron", TermCaps::Rich);
        let u = Identity::for_user("aaron", "ignored", TermCaps::Rich);
        assert_ne!(g.seed, u.seed);
    }

    #[test]
    fn glyph_pool_reasonable_size() {
        // Too-small pool guarantees collisions. Sanity check only.
        assert!(GLYPHS.len() >= 16, "glyph pool shrunk: {}", GLYPHS.len());
    }

    #[test]
    fn glyphs_are_single_char_non_ascii() {
        // We don't want ASCII here — the whole point is distinctive
        // visual markers. But every entry must be one code point so
        // single-column rendering is safe.
        for g in GLYPHS {
            assert!(!g.is_ascii(), "glyph {g:?} is ASCII — use `char` shapes");
            assert!(g.len_utf8() >= 3, "glyph {g:?} unexpectedly narrow");
        }
    }

    #[test]
    fn case_sensitive_group_names() {
        // Group names follow the dir name on disk; filesystems are
        // case-sensitive on Linux, so we treat the names that way.
        let a = Group::for_name("Deploy", TermCaps::Rich);
        let b = Group::for_name("deploy", TermCaps::Rich);
        assert_ne!(a.seed, b.seed);
    }

    #[test]
    fn mono_produces_mono_entry() {
        let g = Group::for_name("deploy", TermCaps::Mono);
        assert_eq!(g.palette.name, "mono");
    }
}
