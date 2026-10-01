//! The theme data: slots, kind, background mode, optional role overrides.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    /// dottheme `blend A B pct` (theme/dottheme:121-129): `pct` percent of
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

/// The ten slots, as dottheme's `THEME_*` names them.
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
}

/// Derived roles a theme may pin to a colour instead of computing.
/// Overrides apply after derivation and skip the contrast lift.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Overrides {
    pub hot: Option<Rgb>,
    pub muted: Option<Rgb>,
    pub ink: Option<Rgb>,
    pub text: Option<Rgb>,
    pub faded_ink: Option<Rgb>,
    pub faded_text: Option<Rgb>,
    pub rule: Option<Rgb>,
    pub track: Option<Rgb>,
    pub track_past: Option<Rgb>,
    pub accent_dim: Option<Rgb>,
    pub selection_bg: Option<Rgb>,
}

impl Overrides {
    pub const NAMES: [&'static str; 11] = [
        "hot", "muted", "ink", "text", "faded_ink", "faded_text", "rule", "track", "track_past", "accent_dim", "selection_bg",
    ];

    pub fn get(&self, name: &str) -> Option<Rgb> {
        match name {
            "hot" => self.hot,
            "muted" => self.muted,
            "ink" => self.ink,
            "text" => self.text,
            "faded_ink" => self.faded_ink,
            "faded_text" => self.faded_text,
            "rule" => self.rule,
            "track" => self.track,
            "track_past" => self.track_past,
            "accent_dim" => self.accent_dim,
            "selection_bg" => self.selection_bg,
            _ => None,
        }
    }

    pub fn set(&mut self, name: &str, v: Rgb) -> bool {
        let slot = match name {
            "hot" => &mut self.hot,
            "muted" => &mut self.muted,
            "ink" => &mut self.ink,
            "text" => &mut self.text,
            "faded_ink" => &mut self.faded_ink,
            "faded_text" => &mut self.faded_text,
            "rule" => &mut self.rule,
            "track" => &mut self.track,
            "track_past" => &mut self.track_past,
            "accent_dim" => &mut self.accent_dim,
            "selection_bg" => &mut self.selection_bg,
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
