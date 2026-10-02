//! Roles derived from the slots, and the contrast arithmetic that keeps them
//! readable (ADR-504 §4). The blend formulas follow the dotfiles `dottheme`
//! tool's status-line generator; the `SL_*` names below are its own.

use crate::model::{Kind, Rgb, Theme};
use crate::oklab::{clip_chroma, delta_e, lch, Lch};

/// WCAG 2.x relative luminance.
fn luminance(c: Rgb) -> f64 {
    let lin = |v: u8| {
        let s = v as f64 / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.0) + 0.7152 * lin(c.1) + 0.0722 * lin(c.2)
}

/// WCAG contrast ratio, 1.0 to 21.0.
pub fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

fn passes(c: Rgb, bgs: &[Rgb], min: f64) -> bool {
    bgs.iter().all(|b| contrast(c, *b) >= min)
}

/// `c`, made lighter or darker in OKLCH until it reaches `min` on every
/// background: toward `toward`'s side of the first background, hue kept, and
/// saturation (chroma over lightness) kept as far as sRGB can show it. Returns `c`
/// unchanged when it already reads. When lightness alone cannot get there,
/// it blends toward `toward` in 1% steps, as dottheme does.
///
/// The OKLCH move replaces the blend toward fg as the first resort: on Nord
/// the blend greyed err, hot, warn and accent into one dusty pink.
fn lift(c: Rgb, toward: Rgb, bgs: &[Rgb], min: f64) -> Rgb {
    if passes(c, bgs, min) {
        return c;
    }
    let p = lch(c);
    let up = lch(toward).l >= bgs.first().map_or(0.0, |b| lch(*b).l);
    let step = if up { 0.004 } else { -0.004 };
    let mut l = p.l;
    while (0.0..=1.0).contains(&(l + step)) {
        l += step;
        // Lighter at the same chroma reads paler; chroma grows with
        // lightness so the colour keeps its saturation, as far as sRGB allows.
        let x = clip_chroma(Lch { l, c: p.c * (l / p.l.max(0.01)).max(1.0), ..p });
        if passes(x, bgs, min) {
            return x;
        }
    }
    (0..=100).map(|p| Rgb::blend(toward, c, p)).find(|x| passes(*x, bgs, min)).unwrap_or(toward)
}

/// The first of `prefer` that reads on `bg`, else black or white, whichever
/// reads better.
pub fn text_on(bg: Rgb, prefer: &[Rgb], min: f64) -> Rgb {
    let fallback = [Rgb(0, 0, 0), Rgb(255, 255, 255)];
    if let Some(c) = prefer.iter().find(|c| contrast(**c, bg) >= min) {
        return *c;
    }
    *fallback.iter().max_by(|a, b| contrast(**a, bg).total_cmp(&contrast(**b, bg))).unwrap()
}

/// A lozenge segment: text colour on background colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegPair {
    pub fg: Rgb,
    pub bg: Rgb,
}

/// Every colour the TUI draws.
///
/// Text roles meant for the ground (`body`, `muted`, `accent`, `accent_dim`,
/// `info`, `ok`, `warn`, `err`, `alt`, `hot`) are lifted toward `fg` until
/// they read on both `bg` and `selection_bg`. `ink`, `text`, `faded_ink` and
/// `faded_text` keep dottheme's values: they are lozenge text, read on
/// `dark_seg`. `rule`, `track` and `track_past` are decorative and carry no
/// contrast floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Roles {
    pub bg: Rgb,
    pub body: Rgb,
    pub accent: Rgb,
    pub accent_dim: Rgb,
    pub info: Rgb,
    pub ok: Rgb,
    pub warn: Rgb,
    pub err: Rgb,
    pub alt: Rgb,
    /// SL_HOT: warn and err halfway.
    pub hot: Rgb,
    /// SL_MUTED: dim, lifted to 3:1.
    pub muted: Rgb,
    /// SL_INK: the darker of bg and fg, for text on a bright segment.
    pub ink: Rgb,
    /// SL_TEXT: the lighter of bg and fg, for text on a dark segment.
    pub text: Rgb,
    /// SL_FADED_INK: `light` 75% over dim, lifted to 3:1 on `dark_seg`.
    pub faded_ink: Rgb,
    /// SL_FADED_TEXT: `light` 45% over dim, lifted to 3:1 on `dark_seg`.
    pub faded_text: Rgb,
    /// SL_RULE: dim and subtle halfway.
    pub rule: Rgb,
    /// SL_TRACK: subtle.
    pub track: Rgb,
    /// SL_TRACK_PAST: dim 45% over subtle.
    pub track_past: Rgb,
    /// The accent's shade on bg: the selected row's ground.
    pub selection_bg: Rgb,
    /// The dark segment `text`, `faded_ink` and `faded_text` sit on.
    pub dark_seg: Rgb,
    /// The mode-line badge: text on the accent.
    pub badge: SegPair,
    pub mode_browse: SegPair,
    pub mode_edit: SegPair,
    pub mode_review: SegPair,
    pub mode_apply: SegPair,
}

pub const MIN_TEXT: f64 = 4.5;
pub const MIN_MUTED: f64 = 3.0;

/// The least ΔE OK between any two status roles. 0.02 is a just-noticeable
/// difference for patches side by side; status colours are read as thin
/// glyphs, one word at a time and rarely beside each other, where colour
/// discrimination is several times worse, so the floor is four JNDs.
pub const MIN_DISTINCT: f64 = 0.08;

/// `c` moved the least it can, in OKLCH, to sit `MIN_DISTINCT` from every
/// colour in `settled` while still reading at `min` on `grounds`: hue turned
/// up to 60°, lightness pushed away from the ground, chroma raised. Returns
/// `c` when it is already clear, and `c` again when nothing in reach is.
fn separate(c: Rgb, settled: &[Rgb], grounds: &[Rgb], min: f64) -> Rgb {
    let clear = |x: Rgb| settled.iter().all(|f| delta_e(x, *f) >= MIN_DISTINCT) && passes(x, grounds, min);
    if clear(c) {
        return c;
    }
    let p = lch(c);
    let away = if p.l >= grounds.first().map_or(0.0, |g| lch(*g).l) { 1.0 } else { -1.0 };
    let mut best: Option<(f64, Rgb)> = None;
    for dh in (-60..=60).step_by(3) {
        for dl in 0..=10 {
            for dc in 0..=5 {
                let q = Lch { l: (p.l + away * 0.015 * dl as f64).clamp(0.0, 1.0), c: p.c + 0.02 * dc as f64, h: p.h + (dh as f64).to_radians() };
                let x = clip_chroma(q);
                let d = delta_e(x, c);
                if best.is_none_or(|b| d < b.0) && clear(x) {
                    best = Some((d, x));
                }
            }
        }
    }
    best.map_or(c, |b| b.1)
}

impl Roles {
    pub fn derive(t: &Theme) -> Roles {
        let s = &t.slots;
        let o = &t.overrides;
        // Dark is the segment-ink end, light the text end.
        let (dark, light) = match t.kind {
            Kind::Dark => (s.bg, s.fg),
            Kind::Light => (s.fg, s.bg),
        };
        let selection_bg = o.selection.unwrap_or_else(|| Rgb::blend(s.accent, s.bg, 13));
        let grounds = [s.bg, selection_bg];
        let up = |c: Rgb, min: f64| lift(c, s.fg, &grounds, min);
        // The status roles, lifted, then settled apart in the order of
        // `Roles::status`: each keeps clear of the ones before it. An override
        // is taken as given, and the roles after it keep clear of it.
        let mut settled: Vec<Rgb> = Vec::new();
        let mut settle = |c: Rgb, pinned: bool| {
            let x = if pinned { c } else { separate(c, &settled, &grounds, MIN_TEXT) };
            settled.push(x);
            x
        };
        let accent = settle(up(s.accent, MIN_TEXT), false);
        let err = settle(up(s.err, MIN_TEXT), false);
        let ok = settle(up(s.ok, MIN_TEXT), false);
        let warn = settle(up(s.warn, MIN_TEXT), false);
        let info = settle(up(s.info, MIN_TEXT), false);
        let hot = settle(o.hot.unwrap_or_else(|| up(Rgb::blend(s.warn, s.err, 50), MIN_TEXT)), o.hot.is_some());

        let ink = dark;
        let text = light;
        let dark_seg = match t.kind {
            Kind::Dark => s.subtle,
            Kind::Light => s.fg,
        };
        let seg = |bg: Rgb| SegPair { fg: text_on(bg, &[ink, text], MIN_TEXT), bg };

        Roles {
            bg: s.bg,
            body: s.fg,
            accent,
            accent_dim: up(Rgb::blend(s.accent, s.bg, 60), MIN_TEXT),
            info,
            ok,
            warn,
            err,
            alt: up(s.alt, MIN_TEXT),
            hot,
            muted: up(s.dim, MIN_MUTED),
            ink,
            text,
            faded_ink: lift(Rgb::blend(light, s.dim, 75), text, &[dark_seg], MIN_MUTED),
            faded_text: o.faded.unwrap_or_else(|| lift(Rgb::blend(light, s.dim, 45), text, &[dark_seg], MIN_MUTED)),
            rule: o.rule.unwrap_or_else(|| Rgb::blend(s.dim, s.subtle, 50)),
            track: s.subtle,
            track_past: Rgb::blend(s.dim, s.subtle, 45),
            selection_bg,
            dark_seg,
            badge: seg(s.accent),
            mode_browse: seg(s.accent),
            mode_edit: seg(s.warn),
            mode_review: seg(s.info),
            mode_apply: seg(s.ok),
        }
    }

    /// The roles that carry a status by colour alone, in the order `derive`
    /// settles them.
    pub fn status(&self) -> [(&'static str, Rgb); 6] {
        [("accent", self.accent), ("err", self.err), ("ok", self.ok), ("warn", self.warn), ("info", self.info), ("hot", self.hot)]
    }

    /// Every pair of status roles closer than `MIN_DISTINCT`, with its ΔE OK.
    pub fn too_close(&self) -> Vec<(&'static str, &'static str, f64)> {
        let s = self.status();
        let mut out = Vec::new();
        for (i, (an, a)) in s.iter().enumerate() {
            for (bn, b) in &s[i + 1..] {
                let d = delta_e(*a, *b);
                if d < MIN_DISTINCT {
                    out.push((*an, *bn, d));
                }
            }
        }
        out
    }

    /// Every role below its contrast floor, as (name, colour, ground, ratio, floor).
    pub fn unreadable(&self) -> Vec<(&'static str, Rgb, Rgb, f64, f64)> {
        let mut out = Vec::new();
        for (name, fg, grounds, min) in self.readable() {
            for g in grounds {
                let c = contrast(fg, g);
                if c < min {
                    out.push((name, fg, g, c, min));
                }
            }
        }
        out
    }

    /// Every (name, colour, ground set, minimum ratio) the contrast test and
    /// the swatch sheet check.
    pub fn readable(&self) -> Vec<(&'static str, Rgb, Vec<Rgb>, f64)> {
        let ground = vec![self.bg, self.selection_bg];
        let mut v = vec![
            ("body", self.body, ground.clone(), MIN_TEXT),
            ("accent", self.accent, ground.clone(), MIN_TEXT),
            ("accent_dim", self.accent_dim, ground.clone(), MIN_TEXT),
            ("info", self.info, ground.clone(), MIN_TEXT),
            ("ok", self.ok, ground.clone(), MIN_TEXT),
            ("warn", self.warn, ground.clone(), MIN_TEXT),
            ("err", self.err, ground.clone(), MIN_TEXT),
            ("alt", self.alt, ground.clone(), MIN_TEXT),
            ("hot", self.hot, ground.clone(), MIN_TEXT),
            ("muted", self.muted, ground, MIN_MUTED),
            ("text/seg", self.text, vec![self.dark_seg], MIN_TEXT),
            ("faded_ink/seg", self.faded_ink, vec![self.dark_seg], MIN_MUTED),
            ("faded_text/seg", self.faded_text, vec![self.dark_seg], MIN_MUTED),
        ];
        for (n, p) in [
            ("badge", self.badge),
            ("mode_browse", self.mode_browse),
            ("mode_edit", self.mode_edit),
            ("mode_review", self.mode_review),
            ("mode_apply", self.mode_apply),
        ] {
            v.push((n, p.fg, vec![p.bg], MIN_TEXT));
        }
        v
    }
}
