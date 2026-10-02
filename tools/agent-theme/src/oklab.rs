//! OKLab and OKLCH (Björn Ottosson, 2020): a perceptual space where lightness
//! moves without dragging hue, and where Euclidean distance (ΔE OK) tracks
//! how different two colours look.

use crate::model::Rgb;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lab {
    pub l: f64,
    pub a: f64,
    pub b: f64,
}

/// Lightness 0..1, chroma 0..~0.37, hue in radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lch {
    pub l: f64,
    pub c: f64,
    pub h: f64,
}

fn to_linear(v: u8) -> f64 {
    let s = v as f64 / 255.0;
    if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

fn from_linear(v: f64) -> f64 {
    if v <= 0.0031308 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

pub fn lab(c: Rgb) -> Lab {
    let (r, g, b) = (to_linear(c.0), to_linear(c.1), to_linear(c.2));
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    Lab {
        l: 0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        a: 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        b: 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    }
}

/// Linear sRGB, unclamped: a channel outside 0..1 is out of gamut.
fn linear_of(p: Lab) -> [f64; 3] {
    let l = (p.l + 0.3963377774 * p.a + 0.2158037573 * p.b).powi(3);
    let m = (p.l - 0.1055613458 * p.a - 0.0638541728 * p.b).powi(3);
    let s = (p.l - 0.0894841775 * p.a - 1.2914855480 * p.b).powi(3);
    [
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    ]
}

pub fn in_gamut(p: Lab) -> bool {
    linear_of(p).iter().all(|v| (-1e-4..=1.0 + 1e-4).contains(v))
}

/// The nearest sRGB colour, each channel clamped.
pub fn rgb(p: Lab) -> Rgb {
    let ch = |v: f64| (from_linear(v.clamp(0.0, 1.0)) * 255.0).round() as u8;
    let [r, g, b] = linear_of(p);
    Rgb(ch(r), ch(g), ch(b))
}

pub fn lch(c: Rgb) -> Lch {
    let p = lab(c);
    Lch { l: p.l, c: p.a.hypot(p.b), h: p.b.atan2(p.a) }
}

pub fn lab_of(p: Lch) -> Lab {
    Lab { l: p.l, a: p.c * p.h.cos(), b: p.c * p.h.sin() }
}

/// ΔE OK: Euclidean distance in OKLab. About 0.02 is a just-noticeable
/// difference side by side.
pub fn delta_e(a: Rgb, b: Rgb) -> f64 {
    let (p, q) = (lab(a), lab(b));
    ((p.l - q.l).powi(2) + (p.a - q.a).powi(2) + (p.b - q.b).powi(2)).sqrt()
}

/// `p` at its own lightness and hue with the most chroma, up to its own,
/// that sRGB can show.
pub fn clip_chroma(p: Lch) -> Rgb {
    if in_gamut(lab_of(p)) {
        return rgb(lab_of(p));
    }
    let (mut lo, mut hi) = (0.0, p.c);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if in_gamut(lab_of(Lch { c: mid, ..p })) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    rgb(lab_of(Lch { c: lo, ..p }))
}
