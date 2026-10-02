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
        let font = match FIGlet::from_content(ANSI_SHADOW_FLF) {
            Ok(f) => f,
            Err(_) => {
                // Fallback: just print the text bold
                println!("\n  {}\n", paint(Style::new().bold(), self.text));
                return;
            }
        };
        let figure = match font.convert(self.text) {
            Some(f) => f,
            None => {
                println!("\n  {}\n", paint(Style::new().bold(), self.text));
                return;
            }
        };

        println!();
        println!("  {}", paint(Style::new().role(Role::Muted).underline(), self.title));
        println!();

        for (i, line) in figure.to_string().lines().enumerate() {
            let color = self.gradient[i % self.gradient.len()];
            println!("{}", paint(color, line));
        }

        if let Some(sub) = self.subtitle {
            println!("  {}", paint(Role::Muted, sub));
        }
        if let Some(ver) = self.version {
            println!("  {}", paint(Role::Muted, ver));
        }
        println!();
    }
}

/// Format a help section with consistent styling.
/// Each entry is (command_name, description).
pub fn print_commands(heading: &str, commands: &[(&str, &str)]) {
    println!("{}", paint(Style::new().bold(), format!("{heading}:")));
    let max_name = commands.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
    for (name, desc) in commands {
        println!("  {}  {}", paint(Style::new().bold(), format!("{name:<max_name$}")), paint(Role::Muted, desc));
    }
}
