//! The ratatui output (ADR-504 §4): the painter's resolution as ratatui
//! styles. Behind the `ratatui` feature, so the engine's core, and every
//! binary on a hook path, carries no ratatui dependency. It maps onto
//! `ratatui-core`, whose types `ratatui` re-exports.

use ratatui_core::style as rs;

use crate::color::Color;
use crate::paint::{Painter, Style};

/// The ANSI colours by index, as ratatui names them.
const NAMED: [rs::Color; 16] = [
    rs::Color::Black,
    rs::Color::Red,
    rs::Color::Green,
    rs::Color::Yellow,
    rs::Color::Blue,
    rs::Color::Magenta,
    rs::Color::Cyan,
    rs::Color::Gray,
    rs::Color::DarkGray,
    rs::Color::LightRed,
    rs::Color::LightGreen,
    rs::Color::LightYellow,
    rs::Color::LightBlue,
    rs::Color::LightMagenta,
    rs::Color::LightCyan,
    rs::Color::White,
];

/// A colour as ratatui takes it. Reduce it to the depth first
/// ([`Color::at`]); this maps the form as given.
pub fn color(c: Color) -> rs::Color {
    match c {
        Color::Ansi(n) => NAMED[(n as usize).min(15)],
        Color::Indexed(n) => rs::Color::Indexed(n),
        Color::Rgb(x) => rs::Color::Rgb(x.0, x.1, x.2),
    }
}

impl Painter {
    /// `style` as a ratatui style: roles drawn, colours reduced to the
    /// depth, attributes as modifiers.
    pub fn ratatui(&self, style: impl Into<Style>) -> rs::Style {
        let r = self.resolve(style);
        let mut out = rs::Style::new();
        if let Some(c) = r.fg {
            out = out.fg(color(c));
        }
        if let Some(c) = r.bg {
            out = out.bg(color(c));
        }
        let mods = [
            (r.bold, rs::Modifier::BOLD),
            (r.dim, rs::Modifier::DIM),
            (r.italic, rs::Modifier::ITALIC),
            (r.underline, rs::Modifier::UNDERLINED),
            (r.reverse, rs::Modifier::REVERSED),
            (r.strike, rs::Modifier::CROSSED_OUT),
        ];
        for (on, m) in mods {
            if on {
                out = out.add_modifier(m);
            }
        }
        out
    }

    /// The style a screen paints every cell with first: the theme's ground
    /// and body text under `THEME_BACKGROUND="fill"`, else nothing, which
    /// keeps the terminal's own.
    pub fn ratatui_base(&self) -> rs::Style {
        match self.roles() {
            Some(r) if self.fills_background() => {
                let bg = Color::Rgb(r.bg).at(self.depth()).map(color);
                let fg = Color::Rgb(r.body).at(self.depth()).map(color);
                let mut s = rs::Style::new();
                if let Some(c) = bg {
                    s = s.bg(c);
                }
                if let Some(c) = fg {
                    s = s.fg(c);
                }
                s
            }
            _ => rs::Style::new(),
        }
    }
}
