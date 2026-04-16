//! Curated nickname pool for agent display identities.
//!
//! Constraints:
//! - ASCII only. No diacritics. A user typing `@` autocomplete should
//!   never be blocked by a keyboard that can't produce é, ü, ø, etc.
//!   Names that would normally carry a mark are transliterated plainly
//!   (Rene, not René; Zoe, not Zoé).
//! - Real given names or real historical/literary proper nouns. No
//!   fantasy-quirky Docker-style adjective+noun concoctions.
//! - Slight literary/historical leaning — claude-ish in spirit without
//!   being all variants of "Claude". Famous Claudes (Monet, Debussy,
//!   Shannon) are included as a nod, not the anchor.
//! - Short to mid-length; most 4–8 characters. Long enough to be
//!   distinctive, short enough to render in a 16-col chip.
//!
//! Size: aim for ~200. Larger pools reduce collisions among concurrent
//! peers but pay diminishing returns past the point where humans
//! distinguish by color+style as well as name. With 20 palette colors
//! and ~3 style variants this gives ~12k effective identities before
//! collisions even start to matter.

pub const NAMES: &[&str] = &[
    // Claude variants and direct nods.
    "Claudette", "Claudio", "Claudia", "Claudine", "Claudius", "Clovis",
    // Famous Claudes (transliterated to ASCII).
    "Monet", "Debussy", "Shannon", "Rains", "Levi", "Chabrol", "Garamond",
    "Lorrain", "Bernard", "McKay", "Akins",
    // Adjacent-sounding whimsical picks.
    "Clement", "Clarice", "Clelia", "Cleo", "Clio", "Clover", "Chaucer",
    // A–C
    "Alden", "Anwen", "Aria", "Aster", "Auden", "August", "Aurora", "Avery",
    "Basil", "Beatrix", "Benedict", "Bernadette", "Blaise", "Bruno",
    "Calista", "Calliope", "Camille", "Cassio", "Cato", "Celeste", "Celia",
    "Ceres", "Chiron", "Cicero", "Cora", "Cosima", "Cyrano", "Cyril", "Cyrus",
    // D–F
    "Dahlia", "Dante", "Daphne", "Darius", "Delta", "Desmond", "Dimitri",
    "Dorian", "Dulcie",
    "Edison", "Edmund", "Edwin", "Elio", "Elise", "Elora", "Emeric", "Emilio",
    "Enzo", "Esme", "Ezra",
    "Fable", "Felix", "Finnian", "Fiora", "Flora", "Florian", "Fox",
    // G–I
    "Gable", "Galen", "Gareth", "Garnet", "Genevieve", "Gideon", "Gilbert",
    "Giselle", "Godfrey",
    "Halcyon", "Hana", "Harlan", "Harper", "Heloise", "Hesper", "Hollis",
    "Horatio",
    "Idris", "Igor", "Inigo", "Iris", "Isadora", "Ivo",
    // J–L
    "Jasper", "Jericho", "Jessamine", "Jovan", "Jubilee", "Jules", "Juniper",
    "Kenna", "Kestrel", "Kieran", "Kit",
    "Lachlan", "Larkin", "Laszlo", "Lavinia", "Leander", "Leopold", "Linnea",
    "Lior", "Lorcan", "Lucian", "Luna", "Lyra",
    // M–O
    "Mabel", "Magnus", "Malachi", "Marigold", "Mateo", "Matilda", "Maude",
    "Maxim", "Mercer", "Miles", "Minerva", "Mira", "Miroslav", "Monroe", "Moss",
    "Nadia", "Neven", "Nicolai", "Niko", "Nina", "Nolan", "Nora", "Nyx",
    "Octavia", "Odessa", "Odin", "Olive", "Omar", "Orion", "Osmund", "Otto",
    "Ovid", "Owain",
    // P–S
    "Pascal", "Perrin", "Petra", "Phaedra", "Phineas", "Poppy", "Primrose",
    "Prosper",
    "Quinn", "Quill", "Quincy",
    "Rafael", "Ramona", "Raphael", "Raven", "Reinette", "Remy", "Rhea",
    "Roland", "Roman", "Rosalind", "Roscoe", "Rowan", "Rufus", "Rune",
    "Sable", "Saffron", "Saoirse", "Saskia", "Seraphine", "Severin", "Shea",
    "Sidra", "Silas", "Sloane", "Solveig", "Soren", "Stellan", "Sylvan",
    // T–Z
    "Tamsin", "Tarquin", "Tatiana", "Tavish", "Teague", "Thaddeus", "Thea",
    "Thora", "Tiernan", "Tobias", "Torsten",
    "Ulric", "Una", "Urban", "Uriel",
    "Vala", "Valentin", "Vera", "Veronica", "Vesper", "Victor", "Vivienne",
    "Wallace", "Walden", "Wells", "Wilmot", "Winifred", "Wren",
    "Xander", "Xenia", "Ximena",
    "Yara", "Yolanda", "Yuri", "Yvette",
    "Zephyr", "Zinnia", "Zoe", "Zora",
];

/// Pick a nickname deterministically from a seed.
///
/// `seed` is expected to already be mixed (e.g. an FNV-1a of the
/// identity key). This function is a pure modulo, so callers are
/// responsible for distribution.
pub fn pick(seed: u64) -> &'static str {
    NAMES[(seed as usize) % NAMES.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_is_reasonably_large() {
        // Below 128 we start seeing visible collisions among everyday
        // peer counts. The concrete floor here is a smoke test — the
        // real contract is "collisions are rare enough that humans
        // don't notice them on a typical session".
        assert!(NAMES.len() >= 128, "nickname pool shrunk below safety floor: {}", NAMES.len());
    }

    #[test]
    fn all_names_are_ascii() {
        for n in NAMES {
            assert!(
                n.is_ascii(),
                "{n:?} contains non-ASCII — strip diacritics before adding"
            );
        }
    }

    #[test]
    fn all_names_nonempty_and_capitalized() {
        for n in NAMES {
            assert!(!n.is_empty(), "empty name");
            let first = n.chars().next().unwrap();
            assert!(first.is_ascii_uppercase(), "{n:?} should start capitalized");
        }
    }

    #[test]
    fn no_duplicates() {
        let mut sorted: Vec<&str> = NAMES.to_vec();
        sorted.sort_unstable();
        for pair in sorted.windows(2) {
            assert_ne!(pair[0], pair[1], "duplicate nickname: {:?}", pair[0]);
        }
    }

    #[test]
    fn pick_is_stable() {
        assert_eq!(pick(42), pick(42));
    }
}
