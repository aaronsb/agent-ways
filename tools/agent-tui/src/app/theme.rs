//! The look: powerline lozenges and one meaning per attribute, drawn in the
//! roles `agent-theme` derives (ADR-504 §4). A frame draws with one
//! [`Palette`]: a chosen theme's roles at 256 colours or more, else the
//! 16-colour terminal palette, where the status roles are the terminal's
//! own ANSI colours, muted text is dim and selection is reverse video. Under
//! `NO_COLOR` nothing is coloured and a lozenge is reverse video.
//!
//! Attributes mean one thing each: bold needs you (a pending change),
//! italic is secondary (hints, the `[a]` marker), strikethrough is a value
//! that is going away, underline is unused.

use std::cell::Cell;

use agent_theme::{ColorDepth, Painter, Rgb, Role, Roles, Theme, MIN_TEXT};
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

/// What a frame draws with: a painter, which is a chosen theme's roles at
/// a colour depth, or the terminal palette.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub painter: Painter,
}

impl Palette {
    /// `theme` where the depth shows 256 colours or more, else the terminal
    /// palette (ADR-504, note of 2026-10-01). `None` is the terminal palette.
    pub fn new(theme: Option<&Theme>, depth: ColorDepth) -> Palette {
        Palette { painter: Painter::select(theme, depth) }
    }

    pub fn terminal(depth: ColorDepth) -> Palette {
        Palette { painter: Painter::terminal(depth) }
    }

    pub fn depth(&self) -> ColorDepth {
        self.painter.depth()
    }

    /// The derived roles when a theme is drawn, with colour.
    pub fn roles(&self) -> Option<Roles> {
        self.painter.roles().copied().filter(|_| self.depth().has_color())
    }
}

impl Default for Palette {
    /// The terminal palette at 16 colours: what a frame draws with until a
    /// theme is chosen.
    fn default() -> Palette {
        Palette::terminal(ColorDepth::Ansi16)
    }
}

thread_local! {
    static CURRENT: Cell<Palette> = Cell::new(Palette::default());
}

/// Make `p` the palette the style functions read, for the frame being drawn.
pub fn set(p: Palette) {
    CURRENT.with(|c| c.set(p));
}

pub fn current() -> Palette {
    CURRENT.with(Cell::get)
}

/// NO_COLOR: every colour is the terminal's default.
pub fn colourless() -> bool {
    !current().depth().has_color()
}

/// Any colour, brought down to the frame's depth: a swatch or a slider.
pub fn rgb(c: Rgb) -> Color {
    agent_theme::Color::Rgb(c).at(current().depth()).map(agent_theme::ratatui::color).unwrap_or(Color::Reset)
}

fn ansi(n: u8) -> Color {
    agent_theme::Color::Ansi(n).at(current().depth()).map(agent_theme::ratatui::color).unwrap_or(Color::Reset)
}

/// A role as text style: the painter's own drawing of it.
fn role(r: Role) -> Style {
    current().painter.ratatui(r)
}

pub fn ok() -> Style {
    role(Role::Ok)
}
pub fn warn() -> Style {
    role(Role::Warn)
}
pub fn err() -> Style {
    role(Role::Err)
}
pub fn info() -> Style {
    role(Role::Info)
}
/// Warn and err halfway, kept clear of both.
pub fn hot() -> Style {
    role(Role::Hot)
}
pub fn muted() -> Style {
    role(Role::Muted)
}
pub fn accent() -> Style {
    role(Role::Accent)
}
/// The accent stepped back: titles, inactive tabs, values off their default.
/// The terminal palette has no such step and uses blue.
pub fn accent_dim() -> Style {
    match current().roles() {
        Some(r) => Style::new().fg(rgb(r.accent_dim)),
        None => Style::new().fg(ansi(4)),
    }
}
pub fn body() -> Style {
    role(Role::Body)
}
/// The theme's ground, or the terminal's.
pub fn bg_color() -> Color {
    current().roles().map_or(Color::Reset, |r| rgb(r.bg))
}
/// The selected row's ground, or none where selection is reverse video.
pub fn shade_color() -> Color {
    current().roles().map_or(Color::Reset, |r| rgb(r.selection_bg))
}

/// The grounds a lozenge segment sits on. Its text is the colour that reads
/// on that ground.
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

    /// The terminal palette's ground and the text that reads on it, as ANSI
    /// indexes.
    fn ansi(self) -> (u8, u8) {
        match self {
            Ground::Accent => (6, 0),
            Ground::AccentDim => (4, 15),
            Ground::Ok => (2, 0),
            Ground::Warn => (3, 0),
            Ground::Err => (1, 15),
            Ground::Hot => (9, 0),
            Ground::Info => (4, 15),
            Ground::Rule => (8, 15),
        }
    }

    pub fn bg(self) -> Color {
        match current().roles() {
            Some(r) => rgb(self.rgb(&r)),
            None => ansi(self.ansi().0),
        }
    }

    /// The text that reads on this ground.
    pub fn fg(self) -> Color {
        match current().roles() {
            Some(r) => rgb(agent_theme::text_on(self.rgb(&r), &[r.ink, r.text], MIN_TEXT)),
            None => ansi(self.ansi().1),
        }
    }
}

/// With background=fill, give every cell the frame left on the terminal's
/// default its theme colour instead: bg behind, body for text.
pub fn fill(buf: &mut Buffer) {
    let p = current().painter;
    if !p.fills_background() {
        return;
    }
    let base = p.ratatui_base();
    let (bg, fg) = (base.bg.unwrap_or(Color::Reset), base.fg.unwrap_or(Color::Reset));
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
    warn().add_modifier(Modifier::BOLD)
}
pub fn non_default() -> Style {
    accent_dim()
}
pub fn read_only() -> Style {
    muted()
}
pub fn secret(present: bool) -> Style {
    if present {
        ok()
    } else {
        muted()
    }
}
pub fn queued() -> Style {
    hot()
}
/// A value a settings file holds that its schema rejects, or a file that
/// fails closed.
pub fn finding() -> Style {
    err().add_modifier(Modifier::BOLD)
}
/// A pending change in review, as it was and as it will be: the old value
/// struck through and muted, the new one green and bold.
pub fn was() -> Style {
    muted().add_modifier(Modifier::CROSSED_OUT)
}
pub fn will() -> Style {
    ok().add_modifier(Modifier::BOLD)
}
pub fn hint() -> Style {
    muted().add_modifier(Modifier::ITALIC)
}
pub fn rule() -> Style {
    match current().roles() {
        Some(r) => Style::new().fg(rgb(r.rule)),
        None => role(Role::Rule),
    }
}
pub fn title() -> Style {
    accent_dim().add_modifier(Modifier::BOLD)
}
/// A modal's border.
pub fn modal_border() -> Style {
    accent_dim()
}
/// The selected row: the accent's shade under the row, a block in the accent
/// beside it. The highlight sets no foreground, so a value keeps its state
/// colour; `selected_text` is the row's base for text with none. Without a
/// theme's roles the row is reverse video.
pub fn selected() -> Style {
    if current().roles().is_none() {
        return Style::new().add_modifier(Modifier::BOLD | Modifier::REVERSED);
    }
    Style::new().bg(shade_color()).add_modifier(Modifier::BOLD)
}
pub fn selected_text() -> Style {
    body()
}
/// A highlighted item in a menu or a checklist: text on the accent.
pub fn picked() -> Style {
    if colourless() || current().roles().is_none() {
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

/// One segment of a lozenge: its text and style. The style's background is
/// the segment's ground, which the joins carry on.
pub struct Seg {
    pub text: String,
    pub style: Style,
}

impl Seg {
    pub fn styled(text: impl Into<String>, style: Style) -> Self {
        Seg { text: text.into(), style }
    }
    /// Text on `g`, in the colour that reads there.
    pub fn on(text: impl Into<String>, g: Ground) -> Self {
        Seg::styled(text, Style::new().fg(g.fg()).bg(g.bg()))
    }
    /// A segment set back: muted text on the selection's shade, or a rule
    /// ground, dim, in the terminal palette.
    pub fn faded(text: impl Into<String>) -> Self {
        match current().roles() {
            Some(r) => Seg::styled(text, Style::new().fg(rgb(r.muted)).bg(rgb(r.selection_bg))),
            None => Seg::styled(text, Seg::on("", Ground::Rule).style.add_modifier(Modifier::DIM)),
        }
    }
    pub fn bold(mut self) -> Self {
        self.style = self.style.add_modifier(Modifier::BOLD);
        self
    }
}

/// The segment joins: the cap that opens a lozenge and the glyph that joins
/// segments and closes it. Every shape but `plain` needs a Nerd Font;
/// `plain` lets the backgrounds abut.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shape {
    pub cap: &'static str,
    pub join: &'static str,
}

impl Shape {
    pub const ROUND: Shape = Shape { cap: "\u{e0b6}", join: "\u{e0b4}" };
    pub const PLAIN: Shape = Shape { cap: "", join: "" };
    /// The names [`Shape::named`] takes.
    pub const NAMES: [&'static str; 6] = ["round", "plain", "flame", "arrow", "slant", "pixel"];

    pub fn named(name: &str) -> Shape {
        match name {
            "round" => Shape::ROUND,
            "plain" => Shape::PLAIN,
            "flame" => Shape { cap: "\u{e0c2}", join: "\u{e0c0}" },
            "arrow" => Shape { cap: "", join: "\u{e0b0}" },
            "slant" => Shape { cap: "\u{e0ba}", join: "\u{e0bc}" },
            "pixel" => Shape { cap: "", join: "\u{e0c6}" },
            // An unknown name gets the shape every terminal font can draw.
            _ => Shape::PLAIN,
        }
    }

    /// The name [`Shape::named`] takes for this shape.
    pub fn name(&self) -> &'static str {
        Shape::NAMES.iter().copied().find(|n| Shape::named(n) == *self).unwrap_or("plain")
    }

    /// The shape after this one in [`Shape::NAMES`], wrapping round.
    pub fn next(&self) -> Shape {
        let i = Shape::NAMES.iter().position(|n| *n == self.name()).unwrap_or(0);
        Shape::named(Shape::NAMES[(i + 1) % Shape::NAMES.len()])
    }

    /// Segments as one lozenge: the cap in the first background, each join
    /// carrying one background into the next, the last closing onto the
    /// terminal ground. Without colour the glyphs go and each segment is
    /// reverse video.
    pub fn lozenge(&self, segs: &[Seg]) -> Vec<Span<'static>> {
        let mut out = Vec::new();
        let Some(first) = segs.first() else { return out };
        if colourless() {
            for s in segs {
                let st = Style::new().add_modifier(Modifier::REVERSED);
                let bold = s.style.add_modifier.contains(Modifier::BOLD);
                out.push(Span::styled(s.text.clone(), if bold { st.add_modifier(Modifier::BOLD) } else { st }));
            }
            return out;
        }
        let ground = |s: &Seg| s.style.bg.unwrap_or(Color::Reset);
        if !self.cap.is_empty() {
            out.push(Span::styled(self.cap, Style::new().fg(ground(first))));
        }
        for (i, s) in segs.iter().enumerate() {
            out.push(Span::styled(s.text.clone(), s.style));
            if !self.join.is_empty() {
                let join = Style::new().fg(ground(s));
                out.push(Span::styled(self.join, segs.get(i + 1).map_or(join, |n| join.bg(ground(n)))));
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

    fn nord() -> Theme {
        agent_theme::ThemeSet::bundled().get("nord").cloned().expect("nord is bundled")
    }

    #[test]
    fn a_lozenge_carries_each_background_into_the_next_join() {
        set(Palette::new(Some(&nord()), ColorDepth::TrueColor));
        let s = Shape::ROUND.lozenge(&[Seg::on(" a ", Ground::Accent), Seg::on(" b ", Ground::AccentDim)]);
        let text: String = s.iter().map(|x| x.content.as_ref()).collect();
        assert_eq!(text, "\u{e0b6} a \u{e0b4} b \u{e0b4}");
        assert_eq!((s[2].style.fg, s[2].style.bg), (Some(Ground::Accent.bg()), Some(Ground::AccentDim.bg())));
        assert_eq!((s[4].style.fg, s[4].style.bg), (Some(Ground::AccentDim.bg()), None));
        let plain: String = Shape::PLAIN.lozenge(&[Seg::on(" a ", Ground::Accent)]).iter().map(|x| x.content.as_ref()).collect();
        assert_eq!(plain, " a ");
        assert_eq!(Shape::named("anything"), Shape::PLAIN, "an unknown shape draws no Nerd Font glyphs");
        assert!(Shape::NAMES.iter().all(|n| Shape::named(n).name() == *n), "every name maps to its own shape");
        set(Palette::default());
    }

    #[test]
    fn without_colour_a_lozenge_is_reverse_video_with_no_glyphs() {
        set(Palette::terminal(ColorDepth::NoColor));
        let s = Shape::ROUND.lozenge(&[Seg::on(" a ", Ground::Accent).bold()]);
        assert_eq!(s.len(), 1);
        assert_eq!((s[0].style.fg, s[0].style.bg), (None, None));
        assert!(s[0].style.add_modifier.contains(Modifier::REVERSED | Modifier::BOLD));
        set(Palette::default());
    }

    #[test]
    fn the_terminal_palette_uses_ansi_colours_dim_and_reverse_video() {
        set(Palette::terminal(ColorDepth::TrueColor));
        assert_eq!(ok().fg, Some(Color::Green));
        assert!(muted().add_modifier.contains(Modifier::DIM) && muted().fg.is_none());
        assert!(selected().add_modifier.contains(Modifier::REVERSED));
        assert_eq!((Ground::Accent.bg(), Ground::Accent.fg()), (Color::Cyan, Color::Black));
        // A chosen theme at 16 colours is the terminal palette whole.
        set(Palette::new(Some(&nord()), ColorDepth::Ansi16));
        assert_eq!(ok().fg, Some(Color::Green));
        set(Palette::new(Some(&nord()), ColorDepth::TrueColor));
        assert!(matches!(ok().fg, Some(Color::Rgb(..))));
        assert!(!selected().add_modifier.contains(Modifier::REVERSED));
        set(Palette::default());
    }
}
