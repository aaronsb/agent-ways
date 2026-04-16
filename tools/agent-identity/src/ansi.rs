//! Raw ANSI escape rendering for terminals that don't use a component
//! framework.
//!
//! Consumers that render to iocraft/ratatui/etc. should map the
//! `PaletteEntry` onto their own types instead — `ansi::wrap` is for
//! `println!`-style callers (attend's `peers` table, banners, status
//! output, etc.).

use crate::palette::{PaletteEntry, Style, TermCaps};

/// Wrap `text` in ANSI SGR codes that express `entry` and `style`
/// under the given capability level.
///
/// On `Mono` terminals the color is dropped and only `style` survives —
/// a monochrome terminal still speaks bold/italic.
pub fn wrap(text: &str, entry: &PaletteEntry, style: Style, caps: TermCaps) -> String {
    let mut prefix = String::new();
    if style.bold {
        prefix.push_str("\x1b[1m");
    }
    if style.italic {
        prefix.push_str("\x1b[3m");
    }
    match caps {
        TermCaps::Rich => {
            prefix.push_str(&format!(
                "\x1b[38;2;{};{};{}m",
                entry.rgb.0, entry.rgb.1, entry.rgb.2
            ));
        }
        TermCaps::Basic => {
            // ANSI 16-color codes: 30–37 for normal, 90–97 for bright.
            let code = if entry.ansi16 < 8 {
                30 + entry.ansi16
            } else {
                90 + (entry.ansi16 - 8)
            };
            prefix.push_str(&format!("\x1b[{code}m"));
        }
        TermCaps::Mono => {
            // No color. Style-only prefix already set above; fall
            // through to the reset suffix.
        }
    }
    format!("{prefix}{text}\x1b[0m")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::RICH_PALETTE;

    #[test]
    fn rich_emits_truecolor_sgr() {
        let e = RICH_PALETTE[0];
        let out = wrap(
            "hi",
            &e,
            Style { bold: false, italic: false },
            TermCaps::Rich,
        );
        let rgb_code = format!("\x1b[38;2;{};{};{}m", e.rgb.0, e.rgb.1, e.rgb.2);
        assert!(out.contains(&rgb_code), "missing truecolor SGR: {out:?}");
        assert!(out.ends_with("\x1b[0m"), "missing reset: {out:?}");
    }

    #[test]
    fn basic_emits_16color_sgr() {
        let e = PaletteEntry {
            rgb: (0, 0, 0),
            ansi16: 12, // bright blue
            name: "bblue",
        };
        let out = wrap(
            "x",
            &e,
            Style { bold: true, italic: false },
            TermCaps::Basic,
        );
        // 12 is bright (>= 8) so we emit 90+(12-8) = 94.
        assert!(out.contains("\x1b[94m"), "missing 94 code: {out:?}");
        assert!(out.contains("\x1b[1m"), "missing bold: {out:?}");
    }

    #[test]
    fn mono_keeps_style_drops_color() {
        let e = RICH_PALETTE[0];
        let out = wrap(
            "x",
            &e,
            Style { bold: true, italic: true },
            TermCaps::Mono,
        );
        assert!(out.contains("\x1b[1m"));
        assert!(out.contains("\x1b[3m"));
        assert!(!out.contains("\x1b[38;"), "mono leaked color: {out:?}");
    }

    #[test]
    fn plain_style_on_mono_is_nearly_empty() {
        let e = RICH_PALETTE[0];
        let out = wrap(
            "hello",
            &e,
            Style::default(),
            TermCaps::Mono,
        );
        // No SGR prefix, just reset at the end (harmless but present).
        assert_eq!(out, "hello\x1b[0m");
    }
}
