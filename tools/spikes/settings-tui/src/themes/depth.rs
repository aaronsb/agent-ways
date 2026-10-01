//! Colour depth: an RGB role down to what the terminal can show.

use ratatui::style::Color;

use super::model::Rgb;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorDepth {
    TrueColor,
    Ansi256,
    Ansi16,
    /// NO_COLOR: no colour at all.
    None,
}

impl ColorDepth {
    /// From the environment: `NO_COLOR` set and non-empty wins; `COLORTERM`
    /// of `truecolor` or `24bit` is truecolor; a `TERM` containing `256` is
    /// 256-colour; anything else is 16.
    pub fn from_env(no_color: Option<&str>, colorterm: Option<&str>, term: Option<&str>) -> ColorDepth {
        if no_color.is_some_and(|v| !v.is_empty()) {
            return ColorDepth::None;
        }
        if colorterm.is_some_and(|v| v == "truecolor" || v == "24bit") {
            return ColorDepth::TrueColor;
        }
        if term.is_some_and(|v| v.contains("256")) {
            return ColorDepth::Ansi256;
        }
        ColorDepth::Ansi16
    }

    pub fn detect() -> ColorDepth {
        let var = |k: &str| std::env::var(k).ok();
        ColorDepth::from_env(var("NO_COLOR").as_deref(), var("COLORTERM").as_deref(), var("TERM").as_deref())
    }
}

/// `None` under NO_COLOR: the caller leaves the style unset.
pub fn color(c: Rgb, depth: ColorDepth) -> Option<Color> {
    match depth {
        ColorDepth::TrueColor => Some(Color::Rgb(c.0, c.1, c.2)),
        ColorDepth::Ansi256 => Some(Color::Indexed(nearest_256(c))),
        ColorDepth::Ansi16 => Some(nearest_16(c)),
        ColorDepth::None => None,
    }
}

fn dist(a: Rgb, b: Rgb) -> u32 {
    let d = |x: u8, y: u8| (x as i32 - y as i32).pow(2) as u32;
    d(a.0, b.0) + d(a.1, b.1) + d(a.2, b.2)
}

const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// Nearest xterm-256 index among the colour cube (16-231) and the grey ramp
/// (232-255). Indices 0-15 are the terminal's own palette and never chosen.
pub fn nearest_256(c: Rgb) -> u8 {
    let level = |v: u8| (0..6).min_by_key(|i| (CUBE[*i] as i32 - v as i32).abs()).unwrap();
    let (r, g, b) = (level(c.0), level(c.1), level(c.2));
    let cube = Rgb(CUBE[r], CUBE[g], CUBE[b]);
    let cube_idx = 16 + 36 * r + 6 * g + b;
    let step = (0..24).min_by_key(|i| (8 + 10 * *i - (c.0 as i32 + c.1 as i32 + c.2 as i32) / 3).abs()).unwrap();
    let grey_v = (8 + 10 * step) as u8;
    let grey = Rgb(grey_v, grey_v, grey_v);
    if dist(c, grey) < dist(c, cube) {
        (232 + step) as u8
    } else {
        cube_idx as u8
    }
}

/// The xterm default RGB of each of the 16 ANSI colours.
const ANSI16: [(Rgb, Color); 16] = [
    (Rgb(0, 0, 0), Color::Black),
    (Rgb(205, 0, 0), Color::Red),
    (Rgb(0, 205, 0), Color::Green),
    (Rgb(205, 205, 0), Color::Yellow),
    (Rgb(0, 0, 238), Color::Blue),
    (Rgb(205, 0, 205), Color::Magenta),
    (Rgb(0, 205, 205), Color::Cyan),
    (Rgb(229, 229, 229), Color::Gray),
    (Rgb(127, 127, 127), Color::DarkGray),
    (Rgb(255, 0, 0), Color::LightRed),
    (Rgb(0, 255, 0), Color::LightGreen),
    (Rgb(255, 255, 0), Color::LightYellow),
    (Rgb(92, 92, 255), Color::LightBlue),
    (Rgb(255, 0, 255), Color::LightMagenta),
    (Rgb(0, 255, 255), Color::LightCyan),
    (Rgb(255, 255, 255), Color::White),
];

pub fn nearest_16(c: Rgb) -> Color {
    ANSI16.iter().min_by_key(|(rgb, _)| dist(c, *rgb)).map(|(_, col)| *col).unwrap()
}
