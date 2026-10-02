//! A colour in one of the three forms a terminal takes, and its reduction to
//! what a given depth can show.

use crate::depth::ColorDepth;
use crate::model::Rgb;

/// A colour as a terminal is told it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Color {
    /// One of the 16 ANSI colours (0-15; 8-15 are the bright half). The
    /// terminal's own palette decides how it looks.
    Ansi(u8),
    /// An xterm 256-colour index.
    Indexed(u8),
    /// 24-bit RGB.
    Rgb(Rgb),
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color::Rgb(Rgb(r, g, b))
    }

    /// The colour as `depth` can show it, or `None` when it shows no colour.
    /// A form the depth supports is kept; a richer one goes to its nearest
    /// neighbour in the poorer palette.
    pub fn at(self, depth: ColorDepth) -> Option<Color> {
        match (depth, self) {
            (ColorDepth::NoColor, _) => None,
            (ColorDepth::TrueColor, c) => Some(c),
            (ColorDepth::Ansi256, Color::Rgb(c)) => Some(Color::Indexed(nearest_256(c))),
            (ColorDepth::Ansi256, c) => Some(c),
            (ColorDepth::Ansi16, Color::Ansi(n)) => Some(Color::Ansi(n)),
            (ColorDepth::Ansi16, Color::Indexed(n)) if n < 16 => Some(Color::Ansi(n)),
            (ColorDepth::Ansi16, Color::Indexed(n)) => Some(Color::Ansi(nearest_16(index_rgb(n)))),
            (ColorDepth::Ansi16, Color::Rgb(c)) => Some(Color::Ansi(nearest_16(c))),
        }
    }

    /// SGR parameters for this colour as a foreground (`bg` false) or
    /// background, with no escape or terminator.
    pub(crate) fn sgr_params(self, bg: bool) -> String {
        let (base, bright, ext) = if bg { (40, 100, 48) } else { (30, 90, 38) };
        match self {
            Color::Ansi(n) if n < 8 => format!("{}", base + n as u16),
            Color::Ansi(n) => format!("{}", bright + (n.min(15) - 8) as u16),
            Color::Indexed(n) => format!("{ext};5;{n}"),
            Color::Rgb(Rgb(r, g, b)) => format!("{ext};2;{r};{g};{b}"),
        }
    }
}

fn dist(a: Rgb, b: Rgb) -> u32 {
    let d = |x: u8, y: u8| (x as i32 - y as i32).pow(2) as u32;
    d(a.0, b.0) + d(a.1, b.1) + d(a.2, b.2)
}

const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// The xterm default RGB of each of the 16 ANSI colours.
pub const ANSI16_RGB: [Rgb; 16] = [
    Rgb(0, 0, 0),
    Rgb(205, 0, 0),
    Rgb(0, 205, 0),
    Rgb(205, 205, 0),
    Rgb(0, 0, 238),
    Rgb(205, 0, 205),
    Rgb(0, 205, 205),
    Rgb(229, 229, 229),
    Rgb(127, 127, 127),
    Rgb(255, 0, 0),
    Rgb(0, 255, 0),
    Rgb(255, 255, 0),
    Rgb(92, 92, 255),
    Rgb(255, 0, 255),
    Rgb(0, 255, 255),
    Rgb(255, 255, 255),
];

/// The RGB an xterm shows for a 256-colour index by default.
pub fn index_rgb(n: u8) -> Rgb {
    match n {
        0..=15 => ANSI16_RGB[n as usize],
        16..=231 => {
            let i = n - 16;
            Rgb(CUBE[(i / 36) as usize], CUBE[(i / 6 % 6) as usize], CUBE[(i % 6) as usize])
        }
        _ => {
            let v = 8 + 10 * (n - 232);
            Rgb(v, v, v)
        }
    }
}

/// Nearest xterm-256 index among the colour cube (16-231) and the grey ramp
/// (232-255). Indices 0-15 are the terminal's own palette and never chosen.
pub fn nearest_256(c: Rgb) -> u8 {
    let level = |v: u8| (0..6).min_by_key(|i| (CUBE[*i] as i32 - v as i32).abs()).unwrap_or(0);
    let (r, g, b) = (level(c.0), level(c.1), level(c.2));
    let cube = Rgb(CUBE[r], CUBE[g], CUBE[b]);
    let cube_idx = 16 + 36 * r + 6 * g + b;
    let avg = (c.0 as i32 + c.1 as i32 + c.2 as i32) / 3;
    let step = (0..24).min_by_key(|i| (8 + 10 * *i - avg).abs()).unwrap_or(0);
    let grey_v = (8 + 10 * step) as u8;
    if dist(c, Rgb(grey_v, grey_v, grey_v)) < dist(c, cube) {
        (232 + step) as u8
    } else {
        cube_idx as u8
    }
}

/// Nearest of the 16 ANSI colours, by their xterm defaults.
pub fn nearest_16(c: Rgb) -> u8 {
    (0..16u8).min_by_key(|i| dist(c, ANSI16_RGB[*i as usize])).unwrap_or(7)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_256_known_values() {
        assert_eq!(nearest_256(Rgb(255, 0, 0)), 196);
        assert_eq!(nearest_256(Rgb(0, 0, 0)), 16);
        assert_eq!(nearest_256(Rgb(255, 255, 255)), 231);
        assert_eq!(nearest_256(Rgb(0x5a, 0xc8, 0xfa)), 81);
        assert_eq!(nearest_256(Rgb(128, 128, 128)), 244);
    }

    #[test]
    fn nearest_16_known_values() {
        assert_eq!(nearest_16(Rgb(250, 10, 10)), 9);
        assert_eq!(nearest_16(Rgb(10, 10, 10)), 0);
        assert_eq!(nearest_16(Rgb(200, 200, 200)), 7);
        assert_eq!(nearest_16(Rgb(0, 200, 0)), 2);
    }

    #[test]
    fn index_rgb_inverts_the_cube_and_ramp() {
        assert_eq!(index_rgb(81), Rgb(95, 215, 255));
        assert_eq!(index_rgb(244), Rgb(128, 128, 128));
        assert_eq!(nearest_256(index_rgb(209)), 209);
    }

    #[test]
    fn depth_selects_the_form() {
        let c = Color::rgb(0x5a, 0xc8, 0xfa);
        assert_eq!(c.at(ColorDepth::TrueColor), Some(c));
        assert_eq!(c.at(ColorDepth::Ansi256), Some(Color::Indexed(81)));
        assert!(matches!(c.at(ColorDepth::Ansi16), Some(Color::Ansi(6 | 14))));
        assert_eq!(c.at(ColorDepth::NoColor), None);
        assert_eq!(Color::Indexed(209).at(ColorDepth::TrueColor), Some(Color::Indexed(209)));
        assert_eq!(Color::Indexed(3).at(ColorDepth::Ansi16), Some(Color::Ansi(3)));
        assert_eq!(Color::Ansi(2).at(ColorDepth::Ansi256), Some(Color::Ansi(2)));
    }

    #[test]
    fn sgr_params_per_form() {
        assert_eq!(Color::Ansi(2).sgr_params(false), "32");
        assert_eq!(Color::Ansi(12).sgr_params(false), "94");
        assert_eq!(Color::Ansi(1).sgr_params(true), "41");
        assert_eq!(Color::Indexed(66).sgr_params(false), "38;5;66");
        assert_eq!(Color::rgb(1, 2, 3).sgr_params(true), "48;2;1;2;3");
    }
}
