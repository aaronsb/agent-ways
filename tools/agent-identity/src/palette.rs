//! Color palette + style axes.
//!
//! Terminal colour depth is agent-theme's to detect (ADR-504 §6); this
//! module picks a palette for a given depth. Entries carry both an RGB
//! triple and an ANSI index, and `PaletteEntry::color` hands agent-theme
//! the form the depth calls for, so this crate keeps no rendering opinion
//! of its own.
//!
//! These identity colours are agent-ways' one categorical palette: agent
//! and group chips, and any output that tells unordered things apart by
//! colour (`ways list` pins), draw from it. They are not theme roles.
//!
//! Philosophy: use **color** as the primary identity signal, and
//! reserve **style bits** (bold / italic / underline) as secondary
//! axes that grow the identity space when the palette alone isn't
//! enough. Italic renders inconsistently across terminals (some
//! italicize, some invert, some ignore), so we use it sparingly and
//! always paired with color so a broken italic doesn't break identity.

use agent_theme::{Color, ColorDepth};

/// One palette entry: an RGB triple *and* the nearest ANSI bright code.
///
/// Callers pick the right form for their renderer: an RGB colour on rich
/// terminals; on basic terminals the ANSI bright index maps to an indexed
/// or named colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaletteEntry {
    pub rgb: (u8, u8, u8),
    /// ANSI 16-color code (0–15). 8–15 are the bright half.
    pub ansi16: u8,
    /// Short human name for debugging and tests.
    pub name: &'static str,
}

impl PaletteEntry {
    /// The entry as agent-theme draws it at `depth`: RGB where the rich
    /// palette applies (agent-theme reduces it to an index at 256 colours),
    /// the ANSI code at 16, nothing without colour.
    pub fn color(&self, depth: ColorDepth) -> Option<Color> {
        match depth {
            ColorDepth::TrueColor | ColorDepth::Ansi256 => Some(Color::rgb(self.rgb.0, self.rgb.1, self.rgb.2)),
            ColorDepth::Ansi16 => Some(Color::Ansi(self.ansi16)),
            ColorDepth::NoColor => None,
        }
    }
}

/// Whether `depth` draws from the 20-entry rich palette (truecolor and
/// 256) rather than the 12-entry basic one.
pub fn is_rich(depth: ColorDepth) -> bool {
    matches!(depth, ColorDepth::TrueColor | ColorDepth::Ansi256)
}

/// 20 distinct colors tuned for readability on both light and dark
/// terminal backgrounds. Saturation kept mid-to-high, lightness around
/// 0.55 — so neither white-on-white nor black-on-black renders.
pub const RICH_PALETTE: &[PaletteEntry] = &[
    PaletteEntry { rgb: (0xff, 0x6b, 0x6b), ansi16: 9,  name: "coral"     },
    PaletteEntry { rgb: (0xff, 0xa5, 0x4c), ansi16: 11, name: "amber"     },
    PaletteEntry { rgb: (0xf4, 0xd0, 0x3f), ansi16: 11, name: "gold"      },
    PaletteEntry { rgb: (0xc9, 0xe2, 0x65), ansi16: 10, name: "lime"      },
    PaletteEntry { rgb: (0x7d, 0xd8, 0x7d), ansi16: 10, name: "mint"      },
    PaletteEntry { rgb: (0x4e, 0xc9, 0xb0), ansi16: 14, name: "teal"      },
    PaletteEntry { rgb: (0x5a, 0xc8, 0xfa), ansi16: 14, name: "sky"       },
    PaletteEntry { rgb: (0x7a, 0xa2, 0xf7), ansi16: 12, name: "azure"     },
    PaletteEntry { rgb: (0xa3, 0x8c, 0xff), ansi16: 12, name: "iris"      },
    PaletteEntry { rgb: (0xc6, 0x7e, 0xff), ansi16: 13, name: "orchid"    },
    PaletteEntry { rgb: (0xff, 0x8a, 0xd4), ansi16: 13, name: "rose"      },
    PaletteEntry { rgb: (0xff, 0xb3, 0xa1), ansi16: 9,  name: "peach"     },
    PaletteEntry { rgb: (0xd4, 0x9a, 0x6a), ansi16: 11, name: "bronze"    },
    PaletteEntry { rgb: (0xb0, 0xbe, 0xc5), ansi16: 15, name: "slate"     },
    PaletteEntry { rgb: (0x9e, 0xa7, 0xad), ansi16: 7,  name: "pewter"    },
    PaletteEntry { rgb: (0x6b, 0xc7, 0x9a), ansi16: 10, name: "sage"      },
    PaletteEntry { rgb: (0xe0, 0x7b, 0x7b), ansi16: 9,  name: "brick"     },
    PaletteEntry { rgb: (0xe6, 0xc3, 0x8c), ansi16: 11, name: "wheat"     },
    PaletteEntry { rgb: (0x9a, 0xd9, 0xd0), ansi16: 14, name: "seafoam"   },
    PaletteEntry { rgb: (0xba, 0xa6, 0xff), ansi16: 12, name: "lavender"  },
];

/// 12-color ANSI fallback (bright 8..15 + 4 from the standard set to
/// round it out). Skips black/white/bright-black/bright-white since
/// those disappear on most backgrounds.
pub const BASIC_PALETTE: &[PaletteEntry] = &[
    PaletteEntry { rgb: (0xcd, 0x00, 0x00), ansi16: 1,  name: "red"       },
    PaletteEntry { rgb: (0x00, 0xcd, 0x00), ansi16: 2,  name: "green"     },
    PaletteEntry { rgb: (0xcd, 0xcd, 0x00), ansi16: 3,  name: "yellow"    },
    PaletteEntry { rgb: (0x00, 0x00, 0xee), ansi16: 4,  name: "blue"      },
    PaletteEntry { rgb: (0xcd, 0x00, 0xcd), ansi16: 5,  name: "magenta"   },
    PaletteEntry { rgb: (0x00, 0xcd, 0xcd), ansi16: 6,  name: "cyan"      },
    PaletteEntry { rgb: (0xff, 0x5c, 0x5c), ansi16: 9,  name: "bred"      },
    PaletteEntry { rgb: (0x5c, 0xff, 0x5c), ansi16: 10, name: "bgreen"    },
    PaletteEntry { rgb: (0xff, 0xff, 0x5c), ansi16: 11, name: "byellow"   },
    PaletteEntry { rgb: (0x5c, 0x5c, 0xff), ansi16: 12, name: "bblue"     },
    PaletteEntry { rgb: (0xff, 0x5c, 0xff), ansi16: 13, name: "bmagenta"  },
    PaletteEntry { rgb: (0x5c, 0xff, 0xff), ansi16: 14, name: "bcyan"     },
];

/// Style axes that combine with color to grow the identity space.
///
/// Italic is intentionally rare — some terminals render it as reverse
/// video or ignore it entirely, and we don't want identity to hinge on
/// it. Underline is reserved for transient UI state (hover, focus) in
/// consumers; we do *not* include it in identity style bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
}

/// Palette resolution result: which table to index into, plus the
/// style bits applied on top.
#[derive(Clone, Copy, Debug)]
pub struct Resolved {
    pub entry: PaletteEntry,
    pub style: Style,
}

/// The rich palette in categorical order: for things told apart by colour
/// alone, the `i`th thing takes `categorical(i)`. Neighbours differ in hue,
/// and in their ANSI code too, so adjacent categories stay apart on a
/// 16-colour terminal: sky, mint, coral, iris, gold, rose (six distinct
/// codes), then teal, lime, amber, azure.
pub const CATEGORICAL: [usize; 10] = [6, 4, 0, 8, 2, 10, 5, 3, 1, 7];

/// The `i`th categorical colour, cycling after ten.
pub fn categorical(i: usize) -> PaletteEntry {
    RICH_PALETTE[CATEGORICAL[i % CATEGORICAL.len()]]
}

/// Pick a palette entry + style deterministically from a seed and the
/// terminal's colour depth.
///
/// Contract: identical `(seed, depth)` → identical `Resolved`. Truecolor
/// and 256 share the rich palette; 16 colours take the basic one.
pub fn resolve(seed: u64, depth: ColorDepth) -> Resolved {
    let palette: &[PaletteEntry] = match depth {
        ColorDepth::TrueColor | ColorDepth::Ansi256 => RICH_PALETTE,
        ColorDepth::Ansi16 => BASIC_PALETTE,
        ColorDepth::NoColor => {
            // No color — identity carries entirely on style. Return a
            // neutral entry and vary style bits across the full range.
            let neutral = PaletteEntry {
                rgb: (0xc0, 0xc0, 0xc0),
                ansi16: 7,
                name: "mono",
            };
            return Resolved {
                entry: neutral,
                style: style_from_seed(seed, /*mono=*/ true),
            };
        }
    };
    let entry = palette[(seed as usize) % palette.len()];
    // Derive style from the *next* hash step so two identities that
    // land on the same color still distinguish on bold.
    let style = style_from_seed(seed.rotate_left(17), /*mono=*/ false);
    Resolved { entry, style }
}

fn style_from_seed(seed: u64, mono: bool) -> Style {
    // Color-capable terminals: bold is cheap, italic rare (roughly 1/4
    // of seeds) so the italic terminals-that-misrender-it case stays
    // visually in the minority.
    // Monochrome: lean harder on bold + italic so style alone can
    // distinguish a handful of peers.
    let bold = (seed & 0b1) == 1;
    let italic = if mono {
        (seed & 0b10) == 0b10
    } else {
        (seed & 0b111) == 0b111
    };
    Style { bold, italic }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_is_stable() {
        let a = resolve(12345, ColorDepth::TrueColor);
        let b = resolve(12345, ColorDepth::TrueColor);
        assert_eq!(a.entry, b.entry);
        assert_eq!(a.style, b.style);
    }

    #[test]
    fn resolve_distributes_across_rich_palette() {
        // Run a handful of seeds and verify we touch a reasonable slice
        // of the palette. This isn't a uniformity test, just a sanity
        // check that we don't collapse onto one color.
        use std::collections::HashSet;
        let colors: HashSet<&str> = (0u64..200)
            .map(|s| resolve(s, ColorDepth::TrueColor).entry.name)
            .collect();
        assert!(
            colors.len() >= 12,
            "rich palette coverage too low: {} of {}",
            colors.len(),
            RICH_PALETTE.len()
        );
    }

    #[test]
    fn mono_returns_neutral_but_still_styles() {
        // Cycle seeds and confirm both bold=true and bold=false appear.
        let mut saw_bold = false;
        let mut saw_plain = false;
        for s in 0u64..32 {
            let r = resolve(s, ColorDepth::NoColor);
            assert_eq!(r.entry.name, "mono");
            if r.style.bold { saw_bold = true; } else { saw_plain = true; }
        }
        assert!(saw_bold && saw_plain, "mono style not varying across seeds");
    }

    #[test]
    fn depth_picks_the_palette_and_the_form() {
        assert_eq!(resolve(7, ColorDepth::TrueColor).entry, resolve(7, ColorDepth::Ansi256).entry);
        assert!(BASIC_PALETTE.contains(&resolve(7, ColorDepth::Ansi16).entry));
        let e = RICH_PALETTE[0];
        assert_eq!(e.color(ColorDepth::TrueColor), Some(Color::rgb(0xff, 0x6b, 0x6b)));
        assert_eq!(e.color(ColorDepth::Ansi16), Some(Color::Ansi(9)));
        assert_eq!(e.color(ColorDepth::NoColor), None);
    }

    #[test]
    fn categorical_neighbours_differ_on_a_16_colour_terminal() {
        let codes: Vec<u8> = (0..CATEGORICAL.len()).map(|i| categorical(i).ansi16).collect();
        for i in 0..codes.len() {
            let next = codes[(i + 1) % codes.len()];
            assert_ne!(codes[i], next, "clusters {i} and {} share ANSI {next}", (i + 1) % codes.len());
        }
        let first6: std::collections::HashSet<_> = codes[..6].iter().collect();
        assert_eq!(first6.len(), 6, "the first six clusters each get their own code: {codes:?}");
    }

    #[test]
    fn categorical_order_is_ten_distinct_rich_entries() {
        let names: std::collections::HashSet<_> = (0..10).map(|i| categorical(i).name).collect();
        assert_eq!(names.len(), 10);
        assert_eq!(categorical(10), categorical(0));
        assert_eq!(categorical(0).name, "sky");
    }

    #[test]
    fn palette_sizes_match_doc() {
        assert_eq!(RICH_PALETTE.len(), 20);
        assert_eq!(BASIC_PALETTE.len(), 12);
    }
}
