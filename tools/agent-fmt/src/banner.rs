//! Shared banner renderer for agent-ways tools.
//!
//! Usage:
//!   agent_fmt::Banner::new("WAYS")
//!       .subtitle("cognitive steering for AI agents")
//!       .gradient(&agent_fmt::GRADIENT_CORAL)
//!       .print();

use agent_theme::{paint, Color, Role, Style};
use figlet_rs::FIGlet;

/// ANSI Shadow font embedded at compile time.
const ANSI_SHADOW_FLF: &str = include_str!("../fonts/ansi-shadow.flf");

/// Warm coral-to-amber gradient (ways). A banner gradient is not a theme
/// role (ADR-504 §4): it is drawn as fixed xterm-256 colours.
pub const GRADIENT_CORAL: [Color; 7] = [
    Color::Indexed(209),
    Color::Indexed(210),
    Color::Indexed(216),
    Color::Indexed(222),
    Color::Indexed(179),
    Color::Indexed(172),
    Color::Indexed(130),
];

/// Cool teal gradient (attend).
pub const GRADIENT_TEAL: [Color; 7] = [
    Color::Indexed(73),
    Color::Indexed(79),
    Color::Indexed(80),
    Color::Indexed(116),
    Color::Indexed(109),
    Color::Indexed(66),
    Color::Indexed(66),
];

pub struct Banner<'a> {
    text: &'a str,
    title: &'a str,
    subtitle: Option<&'a str>,
    version: Option<&'a str>,
    gradient: &'a [Color],
}

impl<'a> Banner<'a> {
    pub fn new(text: &'a str) -> Self {
        Banner {
            text,
            title: "A G E N T",
            subtitle: None,
            version: None,
            gradient: &GRADIENT_CORAL,
        }
    }

    pub fn title(mut self, t: &'a str) -> Self {
        self.title = t;
        self
    }

    pub fn subtitle(mut self, s: &'a str) -> Self {
        self.subtitle = Some(s);
        self
    }

    pub fn version(mut self, v: &'a str) -> Self {
        self.version = Some(v);
        self
    }

    pub fn gradient(mut self, g: &'a [Color]) -> Self {
        self.gradient = g;
        self
    }

    pub fn print(&self) {
        crate::print_block(&self.render());
    }

    /// The banner as text: the title, the figure a gradient line at a time,
    /// then the subtitle and version.
    pub fn render(&self) -> String {
        let mut out = String::new();
        let font = FIGlet::from_content(ANSI_SHADOW_FLF).ok();
        let Some(figure) = font.as_ref().and_then(|f| f.convert(self.text)) else {
            // Fallback: just the text, bold
            crate::say!(out, "\n  {}\n", paint(Style::new().bold(), self.text));
            return out;
        };

        crate::say!(out);
        crate::say!(out, "  {}", paint(Style::new().role(Role::Muted).underline(), self.title));
        crate::say!(out);

        for (i, line) in figure.to_string().lines().enumerate() {
            let color = self.gradient[i % self.gradient.len()];
            crate::say!(out, "{}", paint(color, line));
        }

        if let Some(sub) = self.subtitle {
            crate::say!(out, "  {}", paint(Role::Muted, sub));
        }
        if let Some(ver) = self.version {
            crate::say!(out, "  {}", paint(Role::Muted, ver));
        }
        crate::say!(out);
        out
    }
}
