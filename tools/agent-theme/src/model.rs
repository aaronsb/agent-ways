//! The theme data: slots, kind, background mode, optional role overrides.
//! The slot names are the dotfiles palette's `THEME_*` keys, lowercased.
//! A gradient and decoration glyphs are agent-ways' own, and optional.

use std::collections::BTreeSet;
use std::sync::Mutex;

use crate::oklab::{lab, rgb, Lab};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// `#rrggbb` only; no short form, no alpha.
    pub fn from_hex(s: &str) -> Option<Rgb> {
        let h = s.strip_prefix('#')?;
        // from_str_radix accepts a sign, so check the digits first.
        if h.len() != 6 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
        Some(Rgb(p(0)?, p(2)?, p(4)?))
    }

    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }

    /// Hue in degrees 0..360, saturation and lightness in percent.
    pub fn to_hsl(self) -> [f64; 3] {
        let (r, g, b) = (self.0 as f64 / 255.0, self.1 as f64 / 255.0, self.2 as f64 / 255.0);
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let l = (max + min) / 2.0;
        let d = max - min;
        if d == 0.0 {
            return [0.0, 0.0, l * 100.0];
        }
        let s = d / (1.0 - (2.0 * l - 1.0).abs());
        let h = if max == r {
            60.0 * ((g - b) / d).rem_euclid(6.0)
        } else if max == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
        [h, s * 100.0, l * 100.0]
    }

    pub fn from_hsl([h, s, l]: [f64; 3]) -> Rgb {
        let (s, l) = (s.clamp(0.0, 100.0) / 100.0, l.clamp(0.0, 100.0) / 100.0);
        let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
        let hp = h.rem_euclid(360.0) / 60.0;
        let x = c * (1.0 - (hp.rem_euclid(2.0) - 1.0).abs());
        let (r, g, b) = match hp as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let m = l - c / 2.0;
        let ch = |v: f64| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
        Rgb(ch(r), ch(g), ch(b))
    }

    /// The dotfiles `dottheme` helper `blend A B pct`: `pct` percent of
    /// `a` over `b`, per channel, integer division as the shell does it.
    pub fn blend(a: Rgb, b: Rgb, pct: u32) -> Rgb {
        let ch = |x: u8, y: u8| ((x as u32 * pct + y as u32 * (100 - pct)) / 100) as u8;
        Rgb(ch(a.0, b.0), ch(a.1, b.1), ch(a.2, b.2))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Background {
    /// Leave the terminal's own background.
    Terminal,
    /// Paint the theme's bg everywhere.
    Fill,
}

/// The ten slots, as the dotfiles palette's `THEME_*` keys name them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slots {
    pub bg: Rgb,
    pub fg: Rgb,
    pub dim: Rgb,
    pub subtle: Rgb,
    pub accent: Rgb,
    pub info: Rgb,
    pub ok: Rgb,
    pub warn: Rgb,
    pub err: Rgb,
    pub alt: Rgb,
}

impl Slots {
    pub const NAMES: [&'static str; 10] = ["bg", "fg", "dim", "subtle", "accent", "info", "ok", "warn", "err", "alt"];

    pub fn get(&self, name: &str) -> Option<Rgb> {
        Some(match name {
            "bg" => self.bg,
            "fg" => self.fg,
            "dim" => self.dim,
            "subtle" => self.subtle,
            "accent" => self.accent,
            "info" => self.info,
            "ok" => self.ok,
            "warn" => self.warn,
            "err" => self.err,
            "alt" => self.alt,
            _ => return None,
        })
    }

    pub fn set(&mut self, name: &str, v: Rgb) -> bool {
        let slot = match name {
            "bg" => &mut self.bg,
            "fg" => &mut self.fg,
            "dim" => &mut self.dim,
            "subtle" => &mut self.subtle,
            "accent" => &mut self.accent,
            "info" => &mut self.info,
            "ok" => &mut self.ok,
            "warn" => &mut self.warn,
            "err" => &mut self.err,
            "alt" => &mut self.alt,
            _ => return false,
        };
        *slot = v;
        true
    }
}

/// Derived roles a theme may pin to a colour instead of computing, as the
/// agent-ways keys `THEME_HOT`, `THEME_RULE`, `THEME_FADED` and
/// `THEME_SELECTION` name them. A pinned role skips the contrast lift.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Overrides {
    pub hot: Option<Rgb>,
    pub rule: Option<Rgb>,
    pub faded: Option<Rgb>,
    pub selection: Option<Rgb>,
}

impl Overrides {
    pub const NAMES: [&'static str; 4] = ["hot", "rule", "faded", "selection"];

    pub fn get(&self, name: &str) -> Option<Rgb> {
        match name {
            "hot" => self.hot,
            "rule" => self.rule,
            "faded" => self.faded,
            "selection" => self.selection,
            _ => None,
        }
    }

    pub fn set(&mut self, name: &str, v: Rgb) -> bool {
        let slot = match name {
            "hot" => &mut self.hot,
            "rule" => &mut self.rule,
            "faded" => &mut self.faded,
            "selection" => &mut self.selection,
            _ => return false,
        };
        *slot = Some(v);
        true
    }
}

/// The direction a gradient runs: top to bottom, or left to right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Vertical,
    Horizontal,
}

impl Axis {
    pub fn name(self) -> &'static str {
        match self {
            Axis::Vertical => "vertical",
            Axis::Horizontal => "horizontal",
        }
    }
}

/// A background gradient (`THEME_GRADIENT`): two to four stops, evenly
/// spaced over the whole frame along `axis`, interpolated in OKLab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gradient {
    pub axis: Axis,
    stops: [Rgb; 4],
    len: u8,
}

impl Gradient {
    pub const MIN_STOPS: usize = 2;
    pub const MAX_STOPS: usize = 4;
    /// Samples per segment the contrast floors check.
    const SAMPLES: usize = 8;

    /// `None` unless there are two to four stops.
    pub fn new(axis: Axis, stops: &[Rgb]) -> Option<Gradient> {
        if !(Gradient::MIN_STOPS..=Gradient::MAX_STOPS).contains(&stops.len()) {
            return None;
        }
        let mut s = [stops[0]; 4];
        s[..stops.len()].copy_from_slice(stops);
        Some(Gradient { axis, stops: s, len: stops.len() as u8 })
    }

    pub fn stops(&self) -> &[Rgb] {
        &self.stops[..self.len as usize]
    }

    /// The colour at `t`, 0.0 at the first stop and 1.0 at the last.
    pub fn at(&self, t: f64) -> Rgb {
        let s = self.stops();
        let span = t.clamp(0.0, 1.0) * (s.len() - 1) as f64;
        let i = (span.floor() as usize).min(s.len() - 2);
        let f = span - i as f64;
        let (a, b) = (lab(s[i]), lab(s[i + 1]));
        rgb(Lab { l: a.l + (b.l - a.l) * f, a: a.a + (b.a - a.a) * f, b: a.b + (b.b - a.b) * f })
    }

    /// The colour of cell `n` of `cells` along the axis.
    pub fn cell(&self, n: u16, cells: u16) -> Rgb {
        self.at(if cells > 1 { n as f64 / (cells - 1) as f64 } else { 0.0 })
    }

    /// Every stop and eight points across each segment: the grounds the
    /// contrast floors hold text to.
    pub fn samples(&self) -> Vec<Rgb> {
        let segments = self.stops().len() - 1;
        let n = segments * Gradient::SAMPLES;
        (0..=n).map(|k| self.at(k as f64 / n as f64)).collect()
    }
}

/// A border's glyphs: one of ratatui's sets by name (`THEME_BORDER`), or
/// eight of the theme's own (`THEME_BORDER_GLYPHS`): top-left, top-right,
/// bottom-left, bottom-right, top edge, bottom edge, left edge, right edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Border {
    Plain,
    Rounded,
    Double,
    Thick,
    Glyphs([&'static str; 8]),
}

impl Border {
    pub const NAMES: [&'static str; 4] = ["plain", "rounded", "double", "thick"];

    pub fn named(name: &str) -> Option<Border> {
        Some(match name {
            "plain" => Border::Plain,
            "rounded" => Border::Rounded,
            "double" => Border::Double,
            "thick" => Border::Thick,
            _ => return None,
        })
    }

    /// The name [`Border::named`] takes; none for the theme's own glyphs.
    pub fn name(&self) -> Option<&'static str> {
        match self {
            Border::Plain => Some("plain"),
            Border::Rounded => Some("rounded"),
            Border::Double => Some("double"),
            Border::Thick => Some("thick"),
            Border::Glyphs(_) => None,
        }
    }
}

/// The glyphs a theme draws its chrome with. Each is one grapheme, one
/// column wide, except the ornament, which may be two. `None` keeps the
/// surface's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Decor {
    pub border: Option<Border>,
    /// `THEME_RULE_GLYPHS`: the rule, and the selected rule.
    pub rules: Option<[&'static str; 2]>,
    /// `THEME_MARK`: the selected row's mark.
    pub mark: Option<&'static str>,
    /// `THEME_PRESENCE`: live, and idle.
    pub presence: Option<[&'static str; 2]>,
    /// `THEME_ORNAMENT`: drawn before a pane title's text.
    pub ornament: Option<&'static str>,
}

impl Decor {
    pub const NONE: Decor = Decor { border: None, rules: None, mark: None, presence: None, ornament: None };
}

/// `s` as a `&'static str`, one allocation per distinct glyph for the life
/// of the process, so [`Decor`] stays `Copy` and ratatui's border sets can
/// borrow it.
pub(crate) fn intern(s: &str) -> &'static str {
    static GLYPHS: Mutex<BTreeSet<&'static str>> = Mutex::new(BTreeSet::new());
    let mut set = GLYPHS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(g) = set.get(s) {
        return g;
    }
    let g: &'static str = Box::leak(s.to_owned().into_boxed_str());
    set.insert(g);
    g
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// File-safe id: `[a-z0-9-]+`.
    pub name: String,
    pub label: String,
    pub kind: Kind,
    pub background: Background,
    pub slots: Slots,
    pub overrides: Overrides,
    /// Painted under `Background::Fill` only.
    pub gradient: Option<Gradient>,
    pub decor: Decor,
}
