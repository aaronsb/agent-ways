//! The look, taken from the operator's Claude Code status line
//! (`~/.dotfiles/claude/statusline*.sh`): its powerline lozenges and one
//! meaning per attribute. The colours are the roles of the theme being
//! shown, derived by the theme engine and brought down to the terminal's
//! colour depth; under NO_COLOR nothing is coloured and a lozenge is reverse
//! video.
//!
//! Attributes mean one thing each, as on the status line: bold needs you
//! (a pending change), italic is secondary (hints, the `[a]` marker),
//! strikethrough is a value that is going away, underline is unused.

use std::cell::RefCell;

use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

use crate::themes::{self, Background, ColorDepth, Rgb, Roles, Theme, MIN_TEXT};

/// The roles of one theme at one colour depth: what a frame draws with.
#[derive(Debug, Clone)]
pub struct Palette {
    pub roles: Roles,
    pub depth: ColorDepth,
    /// Paint the theme's bg behind every cell.
    pub fill: bool,
}

impl Palette {
    pub fn new(t: &Theme, depth: ColorDepth) -> Palette {
        Palette { roles: Roles::derive(t), depth, fill: t.background == Background::Fill && depth != ColorDepth::None }
    }

    fn color(&self, c: Rgb) -> Color {
        themes::color(c, self.depth).unwrap_or(Color::Reset)
    }
}

impl Default for Palette {
    /// The bundled agent-ways theme in truecolor: what tests draw with.
    fn default() -> Palette {
        Palette::new(&themes::parse(themes::BUNDLED[0].1).expect("bundled agent-ways parses"), ColorDepth::TrueColor)
    }
}

thread_local! {
    static CURRENT: RefCell<Palette> = RefCell::new(Palette::default());
}

/// Make `p` the palette the style functions read, for the frame being drawn.
pub fn set(p: Palette) {
    CURRENT.with(|c| *c.borrow_mut() = p);
}

fn role(f: impl Fn(&Roles) -> Rgb) -> Color {
    CURRENT.with(|c| {
        let p = c.borrow();
        p.color(f(&p.roles))
    })
}

/// NO_COLOR: every colour is the terminal's default.
pub fn colourless() -> bool {
    CURRENT.with(|c| c.borrow().depth == ColorDepth::None)
}

pub fn ok() -> Color {
    role(|r| r.ok)
}
pub fn warn() -> Color {
    role(|r| r.warn)
}
pub fn err() -> Color {
    role(|r| r.err)
}
pub fn info() -> Color {
    role(|r| r.info)
}
/// SL_HOT: warn and err halfway, kept clear of both.
pub fn hot() -> Color {
    role(|r| r.hot)
}
pub fn muted() -> Color {
    role(|r| r.muted)
}
/// SL_RULE: separators, borders.
pub fn rule_color() -> Color {
    role(|r| r.rule)
}
pub fn accent() -> Color {
    role(|r| r.accent)
}
pub fn accent_dim() -> Color {
    role(|r| r.accent_dim)
}
/// The accent's shade on bg: the selected row's ground.
pub fn shade() -> Color {
    role(|r| r.selection_bg)
}
pub fn body() -> Color {
    role(|r| r.body)
}
pub fn bg() -> Color {
    role(|r| r.bg)
}
/// Any colour of the theme, at the frame's depth: a swatch or a slider.
pub fn rgb(c: Rgb) -> Color {
    CURRENT.with(|p| p.borrow().color(c))
}

/// The grounds a lozenge segment sits on. Its text is the theme's ink or
/// text, whichever reads on that ground.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ground {
    Accent,
    AccentDim,
    Ok,
    Warn,
    Err,
    Hot,
    Info,
    Rule,
}

impl Ground {
    fn rgb(self, r: &Roles) -> Rgb {
        match self {
            Ground::Accent => r.accent,
            Ground::AccentDim => r.accent_dim,
            Ground::Ok => r.ok,
            Ground::Warn => r.warn,
            Ground::Err => r.err,
            Ground::Hot => r.hot,
            Ground::Info => r.info,
            Ground::Rule => r.rule,
        }
    }

    pub fn bg(self) -> Color {
        role(|r| self.rgb(r))
    }

    /// The text that reads on this ground.
    pub fn fg(self) -> Color {
        role(|r| themes::text_on(self.rgb(r), &[r.ink, r.text], MIN_TEXT))
    }
}

/// With background=fill, give every cell the frame left on the terminal's
/// default its theme colour instead: bg behind, body for text.
pub fn fill(buf: &mut Buffer) {
    let (on, bg, fg) = CURRENT.with(|c| {
        let p = c.borrow();
        (p.fill, p.color(p.roles.bg), p.color(p.roles.body))
    });
    if !on {
        return;
    }
    for cell in buf.content.iter_mut() {
        if cell.bg == Color::Reset {
            cell.bg = bg;
        }
        if cell.fg == Color::Reset {
            cell.fg = fg;
        }
    }
}

/// Values in the tree, by state.
pub fn changed() -> Style {
    Style::new().fg(warn()).add_modifier(Modifier::BOLD)
}
pub fn non_default() -> Style {
    Style::new().fg(accent_dim())
}
pub fn read_only() -> Style {
    Style::new().fg(muted())
}
pub fn secret(present: bool) -> Style {
    Style::new().fg(if present { ok() } else { muted() })
}
pub fn queued() -> Style {
    Style::new().fg(hot())
}
/// A pending change in review, as it was and as it will be: the old value
/// struck through and muted, the new one green and bold.
pub fn was() -> Style {
    Style::new().fg(muted()).add_modifier(Modifier::CROSSED_OUT)
}
pub fn will() -> Style {
    Style::new().fg(ok()).add_modifier(Modifier::BOLD)
}
pub fn hint() -> Style {
    Style::new().fg(muted()).add_modifier(Modifier::ITALIC)
}
pub fn rule() -> Style {
    Style::new().fg(rule_color())
}
pub fn title() -> Style {
    Style::new().fg(accent_dim()).add_modifier(Modifier::BOLD)
}
/// A modal's border.
pub fn modal_border() -> Style {
    Style::new().fg(accent_dim())
}
/// The selected row: the accent's shade under the row, a block in the accent
/// beside it. The highlight sets no foreground, so a value keeps its state
/// colour; `selected_text` is the row's base for text with none. Without
/// colour the row is reverse video.
pub fn selected() -> Style {
    if colourless() {
        return Style::new().add_modifier(Modifier::BOLD | Modifier::REVERSED);
    }
    Style::new().bg(shade()).add_modifier(Modifier::BOLD)
}
pub fn selected_text() -> Style {
    Style::new().fg(body())
}
/// A highlighted item in a menu or a checklist: text on the accent.
pub fn picked() -> Style {
    if colourless() {
        return Style::new().add_modifier(Modifier::BOLD | Modifier::REVERSED);
    }
    Style::new().fg(Ground::Accent.fg()).bg(Ground::Accent.bg()).add_modifier(Modifier::BOLD)
}
/// A flat badge on a ground, as a flow's state badges are drawn.
pub fn badge(g: Ground) -> Style {
    if colourless() {
        return Style::new().add_modifier(Modifier::REVERSED);
    }
    Style::new().fg(g.fg()).bg(g.bg())
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
    /// Text on `g`, in the colour that reads there.
    pub fn on(text: impl Into<String>, g: Ground) -> Self {
        Seg::new(text, g.fg(), g.bg())
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
    /// the next, the last closing onto the terminal ground. Without colour
    /// the glyphs go and each segment is reverse video.
    pub fn lozenge(&self, segs: &[Seg]) -> Vec<Span<'static>> {
        let mut out = Vec::new();
        let Some(first) = segs.first() else { return out };
        if colourless() {
            for s in segs {
                let st = Style::new().add_modifier(Modifier::REVERSED);
                out.push(Span::styled(s.text.clone(), if s.bold { st.add_modifier(Modifier::BOLD) } else { st }));
            }
            return out;
        }
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
        set(Palette::default());
        let s = Shape::ROUND.lozenge(&[Seg::on(" a ", Ground::Accent), Seg::on(" b ", Ground::AccentDim)]);
        let text: String = s.iter().map(|x| x.content.as_ref()).collect();
        assert_eq!(text, "\u{e0b6} a \u{e0b4} b \u{e0b4}");
        assert_eq!((s[2].style.fg, s[2].style.bg), (Some(accent()), Some(accent_dim())));
        assert_eq!((s[4].style.fg, s[4].style.bg), (Some(accent_dim()), None));
        let plain: String = Shape::PLAIN.lozenge(&[Seg::on(" a ", Ground::Accent)]).iter().map(|x| x.content.as_ref()).collect();
        assert_eq!(plain, " a ");
        assert_eq!(Shape::named("anything"), Shape::ROUND);
    }

    #[test]
    fn without_colour_a_lozenge_is_reverse_video_with_no_glyphs() {
        set(Palette { depth: ColorDepth::None, ..Palette::default() });
        let s = Shape::ROUND.lozenge(&[Seg::on(" a ", Ground::Accent).bold()]);
        assert_eq!(s.len(), 1);
        assert_eq!((s[0].style.fg, s[0].style.bg), (None, None));
        assert!(s[0].style.add_modifier.contains(Modifier::REVERSED | Modifier::BOLD));
        set(Palette::default());
    }
}
