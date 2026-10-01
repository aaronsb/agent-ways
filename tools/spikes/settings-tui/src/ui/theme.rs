//! The look, taken from the operator's Claude Code status line
//! (`~/.dotfiles/claude/statusline*.sh`): its colour tokens, its powerline
//! lozenges, and one meaning per attribute. Every colour the TUI draws is
//! named here.
//!
//! Attributes mean one thing each, as on the status line: bold needs you
//! (a pending change), italic is secondary (hints, the `[a]` marker),
//! underline is unused.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

// The status line's SL_ tokens, by role. The ANSI ones follow the terminal's
// own palette, so they track a light or dark theme.
pub const OK: Color = Color::Green; // SL_OK 32
pub const WARN: Color = Color::Yellow; // SL_WARN 33
pub const ERR: Color = Color::Red; // SL_ERR 31
pub const HOT: Color = Color::Indexed(208); // SL_HOT
pub const MUTED: Color = Color::DarkGray; // SL_MUTED 90
pub const INK: Color = Color::Indexed(16); // SL_INK: text on a lozenge
pub const TEXT: Color = Color::Indexed(255); // SL_TEXT: text on a dark segment
pub const RULE: Color = Color::Indexed(239); // SL_RULE: separators, borders

// The accent is one agent-ways identity colour (RICH_PALETTE "sky") with the
// two stages the status line derives from it: dim is 60% (`_ss_dim`), shade
// is the hue at 18% lightness (`_ss_hsl .18`). Dim also tints icons and text
// on the terminal ground, where it reads on light and dark backgrounds.
pub const ACCENT: Color = Color::Rgb(90, 200, 250);
pub const ACCENT_DIM: Color = Color::Rgb(54, 120, 150);
pub const ACCENT_SHADE: Color = Color::Rgb(3, 62, 89);

/// Values in the tree, by state.
pub fn changed() -> Style {
    Style::new().fg(WARN).add_modifier(Modifier::BOLD)
}
pub fn non_default() -> Style {
    Style::new().fg(ACCENT_DIM)
}
pub fn read_only() -> Style {
    Style::new().fg(MUTED)
}
pub fn secret(present: bool) -> Style {
    Style::new().fg(if present { OK } else { MUTED })
}
pub fn queued() -> Style {
    Style::new().fg(HOT)
}
pub fn hint() -> Style {
    Style::new().fg(MUTED).add_modifier(Modifier::ITALIC)
}
pub fn rule() -> Style {
    Style::new().fg(RULE)
}
pub fn title() -> Style {
    Style::new().fg(ACCENT_DIM).add_modifier(Modifier::BOLD)
}
/// The selected row: the accent's shade under the row, a block in the accent
/// beside it. The highlight sets no foreground, so a value keeps its state
/// colour; `selected_text` is the row's base for text with none.
pub fn selected() -> Style {
    Style::new().bg(ACCENT_SHADE).add_modifier(Modifier::BOLD)
}
pub fn selected_text() -> Style {
    Style::new().fg(TEXT)
}
pub const SELECTED_MARK: &str = "▌";

/// One segment of a lozenge: its text, foreground and background.
pub struct Seg {
    pub text: String,
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
}

impl Seg {
    pub fn new(text: impl Into<String>, fg: Color, bg: Color) -> Self {
        Seg { text: text.into(), fg, bg, bold: false }
    }
    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }
}

/// The segment joins, as `SESSIONS_SHAPE` names them: the cap that opens a
/// lozenge and the glyph that joins segments and closes it. Every shape but
/// `plain` needs a Nerd Font; `plain` lets the backgrounds abut.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shape {
    pub cap: &'static str,
    pub join: &'static str,
}

impl Shape {
    pub const ROUND: Shape = Shape { cap: "\u{e0b6}", join: "\u{e0b4}" };
    pub const PLAIN: Shape = Shape { cap: "", join: "" };

    /// The status line's own setting, so the TUI draws what the status line
    /// draws: round by default, plain for a terminal without a Nerd Font.
    pub fn from_env() -> Shape {
        Shape::named(&std::env::var("SESSIONS_SHAPE").unwrap_or_default())
    }

    pub fn named(name: &str) -> Shape {
        match name {
            "plain" => Shape::PLAIN,
            "flame" => Shape { cap: "\u{e0c2}", join: "\u{e0c0}" },
            "arrow" => Shape { cap: "", join: "\u{e0b0}" },
            "slant" => Shape { cap: "\u{e0ba}", join: "\u{e0bc}" },
            "pixel" => Shape { cap: "", join: "\u{e0c6}" },
            _ => Shape::ROUND,
        }
    }

    /// Segments as one lozenge, built like a status line session cell: the
    /// cap in the first background, each join carrying one background into
    /// the next, the last closing onto the terminal ground.
    pub fn lozenge(&self, segs: &[Seg]) -> Vec<Span<'static>> {
        let mut out = Vec::new();
        let Some(first) = segs.first() else { return out };
        if !self.cap.is_empty() {
            out.push(Span::styled(self.cap, Style::new().fg(first.bg)));
        }
        for (i, s) in segs.iter().enumerate() {
            let mut st = Style::new().fg(s.fg).bg(s.bg);
            if s.bold {
                st = st.add_modifier(Modifier::BOLD);
            }
            out.push(Span::styled(s.text.clone(), st));
            if !self.join.is_empty() {
                let join = Style::new().fg(s.bg);
                out.push(Span::styled(self.join, segs.get(i + 1).map_or(join, |n| join.bg(n.bg))));
            }
        }
        out
    }
}

/// The thin rule between flat parts of the status line.
pub fn sep() -> Span<'static> {
    Span::styled(" │ ", rule())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lozenge_carries_each_background_into_the_next_join() {
        let s = Shape::ROUND.lozenge(&[Seg::new(" a ", INK, ACCENT), Seg::new(" b ", TEXT, ACCENT_DIM)]);
        let text: String = s.iter().map(|x| x.content.as_ref()).collect();
        assert_eq!(text, "\u{e0b6} a \u{e0b4} b \u{e0b4}");
        assert_eq!((s[2].style.fg, s[2].style.bg), (Some(ACCENT), Some(ACCENT_DIM)));
        assert_eq!((s[4].style.fg, s[4].style.bg), (Some(ACCENT_DIM), None));
        let plain: String = Shape::PLAIN.lozenge(&[Seg::new(" a ", INK, ACCENT)]).iter().map(|x| x.content.as_ref()).collect();
        assert_eq!(plain, " a ");
        assert_eq!(Shape::named("anything"), Shape::ROUND);
    }
}
