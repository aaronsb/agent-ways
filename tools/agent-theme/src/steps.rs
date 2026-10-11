//! The colour steps of an identity lozenge, as the operator's Claude Code
//! status line draws a session (ADR-405): the name on the identity colour,
//! the project on a darker step of it, the state marker on the darkest. An
//! inactive identity takes the desaturated variant: the same hue with its
//! saturation capped, at three falling lightnesses.

use crate::model::Rgb;

/// Each channel of `c` scaled to `num/den`.
pub fn scale(c: Rgb, num: u16, den: u16) -> Rgb {
    let f = |v: u8| (u16::from(v) * num / den) as u8;
    Rgb(f(c.0), f(c.1), f(c.2))
}

/// `c`'s hue at lightness `l`, its saturation clamped to `[smin, smax]`.
/// All three are fractions in 0..=1.
pub fn hsl_at(c: Rgb, l: f64, smin: f64, smax: f64) -> Rgb {
    let (r, g, b) = (f64::from(c.0) / 255.0, f64::from(c.1) / 255.0, f64::from(c.2) / 255.0);
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let d = mx - mn;
    let (mut h, mut s) = (0.0, 0.0);
    if d > 0.0 {
        s = d / (1.0 - (mx + mn - 1.0).abs());
        h = if mx == r {
            ((g - b) / d) % 6.0
        } else if mx == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        h *= 60.0;
        if h < 0.0 {
            h += 360.0;
        }
    }
    let s = s.clamp(smin, smax);
    let chroma = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h / 60.0;
    let x = chroma * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r1, g1, b1) = match hp {
        hp if hp < 1.0 => (chroma, x, 0.0),
        hp if hp < 2.0 => (x, chroma, 0.0),
        hp if hp < 3.0 => (0.0, chroma, x),
        hp if hp < 4.0 => (0.0, x, chroma),
        hp if hp < 5.0 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = l - chroma / 2.0;
    let to = |v: f64| ((v + m) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
    Rgb(to(r1), to(g1), to(b1))
}

/// One segment's text colour on its ground.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub fg: Rgb,
    pub bg: Rgb,
}

/// The three segments of an identity lozenge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdentitySteps {
    pub name: Step,
    pub project: Step,
    pub mark: Step,
}

/// Dark ink on the identity colour, as the status line's xterm 16.
const INK: Rgb = Rgb(0, 0, 0);
/// Light text on the darker steps, as its xterm 255.
const TEXT: Rgb = Rgb(238, 238, 238);
/// Text on an inactive lozenge, as its xterm 250 and 245.
const FADED_INK: Rgb = Rgb(188, 188, 188);
const FADED_TEXT: Rgb = Rgb(138, 138, 138);

impl IdentitySteps {
    /// The steps of identity colour `c`; `live` false gives the
    /// desaturated variant an inactive identity wears.
    pub fn of(c: Rgb, live: bool) -> IdentitySteps {
        if live {
            IdentitySteps {
                name: Step { fg: INK, bg: c },
                project: Step { fg: TEXT, bg: scale(c, 6, 10) },
                mark: Step { fg: c, bg: hsl_at(c, 0.18, 0.4, 1.0) },
            }
        } else {
            IdentitySteps {
                name: Step { fg: FADED_INK, bg: hsl_at(c, 0.34, 0.0, 0.28) },
                project: Step { fg: FADED_TEXT, bg: hsl_at(c, 0.24, 0.0, 0.22) },
                mark: Step { fg: FADED_TEXT, bg: hsl_at(c, 0.15, 0.0, 0.2) },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_dims_each_channel_as_the_status_line_does() {
        // _ss_dim: 60% of each channel, rounded down.
        assert_eq!(scale(Rgb(255, 128, 7), 6, 10), Rgb(153, 76, 4));
    }

    #[test]
    fn hsl_at_matches_the_status_lines_awk() {
        // Values printed by _ss_hsl in statusline-sessions.sh.
        assert_eq!(hsl_at(Rgb(255, 0, 0), 0.5, 0.0, 1.0), Rgb(255, 0, 0));
        assert_eq!(hsl_at(Rgb(255, 0, 0), 0.18, 0.4, 1.0), Rgb(92, 0, 0));
        assert_eq!(hsl_at(Rgb(90, 200, 250), 0.34, 0.0, 0.28), Rgb(62, 96, 111));
        assert_eq!(hsl_at(Rgb(128, 128, 128), 0.24, 0.0, 0.22), Rgb(61, 61, 61), "a grey keeps no hue");
        assert_eq!(hsl_at(Rgb(128, 128, 128), 0.18, 0.4, 1.0), Rgb(64, 28, 28), "smin lends a grey saturation at hue 0");
    }

    #[test]
    fn an_inactive_identity_is_darker_and_greyer_than_a_live_one() {
        let c = Rgb(90, 200, 250);
        let (live, gone) = (IdentitySteps::of(c, true), IdentitySteps::of(c, false));
        assert_eq!(live.name.bg, c);
        let spread = |x: Rgb| x.0.max(x.1).max(x.2) - x.0.min(x.1).min(x.2);
        assert!(spread(gone.name.bg) < spread(live.name.bg));
        let sum = |x: Rgb| u16::from(x.0) + u16::from(x.1) + u16::from(x.2);
        assert!(sum(gone.name.bg) > sum(gone.project.bg) && sum(gone.project.bg) > sum(gone.mark.bg), "three falling steps");
    }
}
