//! The theme data: slots, kind, background mode, optional role overrides.
//! The slot names are the dotfiles palette's `THEME_*` keys, lowercased.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// `#rrggbb` only; no short form, no alpha.
    pub fn from_hex(s: &str) -> Option<Rgb> {
        let h = s.strip_prefix('#')?;
        if h.len() != 6 || !h.is_ascii() {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// File-safe id: `[a-z0-9-]+`.
    pub name: String,
    pub label: String,
    pub kind: Kind,
    pub background: Background,
    pub slots: Slots,
    pub overrides: Overrides,
}
