//! Color palette + terminal capability detection + style axes.
//!
//! We don't depend on a terminal crate here — capability detection is
//! a couple of env-var probes, and we emit colors as palette indices
//! that the consumer maps onto its own rendering layer (iocraft `Color`,
//! crossterm `Color`, raw ANSI, etc.). That keeps this crate free of a
//! rendering opinion.
//!
//! Philosophy: use **color** as the primary identity signal, and
//! reserve **style bits** (bold / italic / underline) as secondary
//! axes that grow the identity space when the palette alone isn't
//! enough. Italic renders inconsistently across terminals (some
//! italicize, some invert, some ignore), so we use it sparingly and
//! always paired with color so a broken italic doesn't break identity.

/// What the terminal can render.
///
/// We probe env vars only — a PTY round-trip would be more accurate
/// but adds latency and complexity we don't need for picking from
/// three palette sizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TermCaps {
    /// Truecolor or 256-color: full 20-entry palette available.
    Rich,
    /// 16-color ANSI with bright variants.
    Basic,
    /// Monochrome or unknown. We still emit style bits.
    Mono,
}

impl TermCaps {
    /// Detect capability from the current process environment.
    pub fn detect() -> Self {
        Self::detect_from(|k| std::env::var(k).ok())
    }

    /// Detection driven by an injectable env getter. Tests use this to
    /// exercise every branch without mutating the global process env.
    pub fn detect_from<F: Fn(&str) -> Option<String>>(get: F) -> Self {
        if let Some(v) = get("NO_COLOR") {
            if !v.is_empty() {
                return TermCaps::Mono;
            }
        }
        if let Some(v) = get("COLORTERM") {
            let v = v.to_ascii_lowercase();
            if v.contains("truecolor") || v.contains("24bit") {
                return TermCaps::Rich;
            }
        }
        if let Some(term) = get("TERM") {
            let term = term.to_ascii_lowercase();
            if term.contains("256color") || term.contains("direct") {
                return TermCaps::Rich;
            }
            if term == "dumb" {
                return TermCaps::Mono;
            }
            if !term.is_empty() {
                return TermCaps::Basic;
            }
        }
        TermCaps::Mono
    }
}

/// One palette entry: an RGB triple *and* the nearest ANSI bright code.
///
/// Callers pick the right form for their renderer: iocraft accepts
/// `Color::Rgb { r, g, b }` on rich terminals; on basic terminals the
/// ANSI bright index maps to its `Color::AnsiValue` or a named variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaletteEntry {
    pub rgb: (u8, u8, u8),
    /// ANSI 16-color code (0–15). 8–15 are the bright half.
    pub ansi16: u8,
    /// Short human name for debugging and tests.
    pub name: &'static str,
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

/// Pick a palette entry + style deterministically from a seed and the
/// detected capability.
///
/// Contract: identical `(seed, caps)` → identical `Resolved`. Different
/// caps levels may produce different entries — we tune per-palette.
pub fn resolve(seed: u64, caps: TermCaps) -> Resolved {
    let palette: &[PaletteEntry] = match caps {
        TermCaps::Rich => RICH_PALETTE,
        TermCaps::Basic => BASIC_PALETTE,
        TermCaps::Mono => {
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

    fn env<'a>(map: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| map.iter().find(|(kk, _)| *kk == k).map(|(_, v)| v.to_string())
    }

    #[test]
    fn truecolor_is_rich() {
        let e = env(&[("COLORTERM", "truecolor"), ("TERM", "xterm")]);
        assert_eq!(TermCaps::detect_from(e), TermCaps::Rich);
    }

    #[test]
    fn term_256color_is_rich() {
        let e = env(&[("TERM", "xterm-256color")]);
        assert_eq!(TermCaps::detect_from(e), TermCaps::Rich);
    }

    #[test]
    fn bare_xterm_is_basic() {
        let e = env(&[("TERM", "xterm")]);
        assert_eq!(TermCaps::detect_from(e), TermCaps::Basic);
    }

    #[test]
    fn dumb_is_mono() {
        let e = env(&[("TERM", "dumb")]);
        assert_eq!(TermCaps::detect_from(e), TermCaps::Mono);
    }

    #[test]
    fn no_color_wins() {
        // NO_COLOR overrides any rich signal — the spec at no-color.org
        // says any non-empty value disables color.
        let e = env(&[("NO_COLOR", "1"), ("COLORTERM", "truecolor")]);
        assert_eq!(TermCaps::detect_from(e), TermCaps::Mono);
    }

    #[test]
    fn no_env_is_mono() {
        let e = env(&[]);
        assert_eq!(TermCaps::detect_from(e), TermCaps::Mono);
    }

    #[test]
    fn resolve_is_stable() {
        let a = resolve(12345, TermCaps::Rich);
        let b = resolve(12345, TermCaps::Rich);
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
            .map(|s| resolve(s, TermCaps::Rich).entry.name)
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
            let r = resolve(s, TermCaps::Mono);
            assert_eq!(r.entry.name, "mono");
            if r.style.bold { saw_bold = true; } else { saw_plain = true; }
        }
        assert!(saw_bold && saw_plain, "mono style not varying across seeds");
    }

    #[test]
    fn palette_sizes_match_doc() {
        assert_eq!(RICH_PALETTE.len(), 20);
        assert_eq!(BASIC_PALETTE.len(), 12);
    }
}
