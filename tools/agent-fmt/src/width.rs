//! ANSI-visible width: measure, truncate, clip and pad styled terminal text.
//!
//! Width is counted in `char`s and an escape sequence counts for nothing. A
//! sequence runs from `ESC` to the first ASCII letter, which ends every CSI
//! sequence the tools emit (SGR colour ends in `m`, erase-line in `K`, cursor
//! moves in `A`-`H`). East-Asian double width and combining marks are not
//! measured; that would need a dependency the lean binaries decline.
//!
//! The table renderer, `ways` render and the rethink compositor all measure
//! through here.

use agent_theme::RESET;

/// Visible length of `s` in `char`s, ignoring ANSI escape sequences.
pub fn visible_len(s: &str) -> usize {
    let mut len = 0;
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if c == '\x1b' {
            in_escape = true;
        } else {
            len += 1;
        }
    }
    len
}

/// Keep at most `max` visible chars of `s`, escapes included, and say whether
/// anything visible was dropped.
fn take_visible(s: &str, max: usize) -> (String, bool) {
    let mut result = String::new();
    let mut visible = 0;
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            result.push(c);
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if c == '\x1b' {
            in_escape = true;
            result.push(c);
        } else {
            if visible >= max {
                return (result, true);
            }
            result.push(c);
            visible += 1;
        }
    }
    (result, false)
}

/// Seal a cut string: text that carries an escape gets a reset so its style
/// cannot bleed past the cut. Plain text stays free of escapes.
fn seal(mut s: String) -> String {
    if s.contains('\x1b') {
        s.push_str(RESET);
    }
    s
}

/// Truncate `s` to a visible width of `max`, ending in `…` when cut. Escapes
/// do not count toward the width.
pub fn truncate_visible(s: &str, max: usize) -> String {
    if visible_len(s) <= max {
        return s.to_string();
    }
    if max <= 1 {
        return "…".to_string();
    }
    let (mut head, _) = take_visible(s, max - 1);
    head.push('…');
    seal(head)
}

/// Cut `s` to at most `max` visible chars with no ellipsis: the hard edge of
/// a panel or a terminal. A cut through styled text is sealed with a reset.
pub fn clip_visible(s: &str, max: usize) -> String {
    match take_visible(s, max) {
        (head, true) => seal(head),
        (whole, false) => whole,
    }
}

/// Right-pad `s` with spaces to a visible `width`. Never truncates: a line
/// already wider is returned unchanged (clip first for a hard cap).
pub fn pad_visible(s: &str, width: usize) -> String {
    let vis = visible_len(s);
    if vis >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - vis))
    }
}

/// Fit `s` to exactly `width` visible chars: clip if longer, pad if shorter.
pub fn fit_visible(s: &str, width: usize) -> String {
    pad_visible(&clip_visible(s, width), width)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_theme::{Color, ColorDepth, Painter, Role};

    fn pinned() -> Painter {
        Painter::terminal(ColorDepth::TrueColor)
    }

    fn red(s: &str) -> String {
        pinned().paint(Role::Err, s)
    }

    #[test]
    fn visible_len_ignores_ansi() {
        assert_eq!(visible_len("abc"), 3);
        assert_eq!(visible_len(&red("abc")), 3);
        assert_eq!(visible_len(""), 0);
        // Truecolor SGR (ends in 'm') is fully skipped.
        assert_eq!(visible_len(&pinned().paint(Color::rgb(99, 179, 237), "●")), 1);
    }

    /// A sequence ends at its first letter, not only at `m`. The table copy
    /// waited for an `m`, so an erase-line (`ESC[2K`) swallowed the text after
    /// it and a row measured short. The fixture is an erase, not a colour, so
    /// it is spelled from the escape char the parser itself matches rather than
    /// drawn through agent-theme, which has no erase sequence.
    #[test]
    fn a_non_sgr_sequence_ends_at_its_letter() {
        let erase = format!("{}[2K", '\x1b');
        assert_eq!(visible_len(&format!("{erase}ab")), 2);
        assert_eq!(truncate_visible(&format!("{erase}abcdef"), 4), format!("{erase}abc…{RESET}"));
    }

    #[test]
    fn plain_text_truncates_with_an_ellipsis_and_no_escapes() {
        let t = truncate_visible("softwaredev/freshness", 6);
        assert_eq!(t, "softw…");
        assert!(!t.contains('\x1b'));
        assert_eq!(truncate_visible("abc", 1), "…");
        assert_eq!(truncate_visible("abc", 3), "abc");
    }

    #[test]
    fn styled_text_truncated_mid_style_is_sealed() {
        let t = truncate_visible(&red("abcdef"), 4);
        assert!(t.ends_with(&format!("…{RESET}")), "{t:?}");
    }

    #[test]
    fn clip_counts_only_visible_and_seals_style() {
        assert_eq!(clip_visible("abcdef", 3), "abc");
        assert_eq!(clip_visible("abc", 10), "abc");
        let t = clip_visible(&red("abcdef"), 3);
        assert_eq!(visible_len(&t), 3);
        assert!(t.ends_with(RESET), "a clip must seal the style: {t:?}");
        // Not clipped → no extra reset beyond the original.
        assert_eq!(clip_visible(&red("ab"), 5), red("ab"));
    }

    #[test]
    fn pad_and_fit_reach_exact_visible_width() {
        assert_eq!(pad_visible("ab", 5), "ab   ");
        assert_eq!(visible_len(&pad_visible(&red("ab"), 5)), 5);
        assert_eq!(pad_visible("abcde", 3), "abcde");
        assert_eq!(visible_len(&fit_visible("abcdef", 4)), 4);
        assert_eq!(visible_len(&fit_visible("ab", 4)), 4);
    }
}
