//! ANSI rendering of an identity for terminals that don't use a component
//! framework.
//!
//! Consumers that render to iocraft/ratatui/etc. should map the
//! `PaletteEntry` onto their own types instead — `ansi::wrap` is for
//! `println!`-style callers (attend's `peers` table, banners, status
//! output, etc.). The escape sequences themselves are agent-theme's
//! (ADR-504 §6); identity colours are categorical, not theme roles, so
//! they are drawn as fixed colours at the caller's depth.

use crate::palette::{PaletteEntry, Style};
use agent_theme::{ColorDepth, Painter};

/// `text` in `entry`'s colour and `style`'s bold and italic, at `depth`.
///
/// Without colour only `style` survives — a monochrome terminal still
/// speaks bold/italic — and text with no style at all comes back bare.
pub fn wrap(text: &str, entry: &PaletteEntry, style: Style, depth: ColorDepth) -> String {
    let mut s = agent_theme::Style::new();
    if style.bold {
        s = s.bold();
    }
    if style.italic {
        s = s.italic();
    }
    if let Some(c) = entry.color(depth) {
        s = s.fg(c);
    }
    Painter::terminal(depth).paint(s, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::RICH_PALETTE;
    use agent_theme::{Color, RESET};

    fn sgr(s: agent_theme::Style) -> String {
        Painter::terminal(ColorDepth::TrueColor).sgr(s)
    }

    #[test]
    fn truecolor_emits_the_rgb_entry() {
        let e = RICH_PALETTE[0];
        let out = wrap("hi", &e, Style { bold: false, italic: false }, ColorDepth::TrueColor);
        let rgb = sgr(agent_theme::Style::new().fg(Color::rgb(e.rgb.0, e.rgb.1, e.rgb.2)));
        assert_eq!(out, format!("{rgb}hi{RESET}"));
    }

    #[test]
    fn ansi256_reduces_the_entry_to_an_index() {
        let e = RICH_PALETTE[0];
        let out = wrap("hi", &e, Style::default(), ColorDepth::Ansi256);
        let idx = agent_theme::nearest_256(agent_theme::Rgb(e.rgb.0, e.rgb.1, e.rgb.2));
        assert_eq!(out, format!("{}hi{RESET}", sgr(agent_theme::Style::new().fg(Color::Indexed(idx)))));
    }

    #[test]
    fn basic_emits_the_16_colour_code_with_bold() {
        let e = PaletteEntry { rgb: (0, 0, 0), ansi16: 12, name: "bblue" };
        let out = wrap("x", &e, Style { bold: true, italic: false }, ColorDepth::Ansi16);
        assert_eq!(out, format!("{}x{RESET}", sgr(agent_theme::Style::new().bold().fg(Color::Ansi(12)))));
    }

    #[test]
    fn no_colour_keeps_style_drops_color() {
        let e = RICH_PALETTE[0];
        let out = wrap("x", &e, Style { bold: true, italic: true }, ColorDepth::NoColor);
        assert_eq!(out, format!("{}x{RESET}", sgr(agent_theme::Style::new().bold().italic())));
    }

    #[test]
    fn plain_style_without_colour_is_bare_text() {
        let out = wrap("hello", &RICH_PALETTE[0], Style::default(), ColorDepth::NoColor);
        assert_eq!(out, "hello");
    }
}
