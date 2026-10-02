//! Terminal colour depth, from the environment (ADR-504 §6, terminal capability).
//!
//! This is the one place in the workspace that decides whether, and how
//! richly, a terminal is coloured. It reads environment variables only; a
//! terminal round-trip would be more exact but costs latency on every hook.

/// How much colour the terminal shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorDepth {
    /// 24-bit RGB.
    TrueColor,
    /// The xterm 256-colour palette.
    Ansi256,
    /// The 16 ANSI colours, as the terminal's own palette defines them.
    Ansi16,
    /// No colour at all: `NO_COLOR`, `TERM=dumb`, or no `TERM`. Text
    /// attributes (bold, dim, reverse) still apply.
    NoColor,
}

impl ColorDepth {
    /// Depth from an environment lookup, so tests can drive every branch
    /// without touching the process environment.
    ///
    /// In order: a non-empty `NO_COLOR` means none (no-color.org); a `TERM`
    /// that is unset, empty or `dumb` means none; a `COLORTERM` of
    /// `truecolor` or `24bit`, or a `TERM` naming `direct`, is truecolor; a
    /// `TERM` containing `256` is 256-colour; any other `TERM` is 16.
    pub fn from_env<F: Fn(&str) -> Option<String>>(get: F) -> ColorDepth {
        if get("NO_COLOR").is_some_and(|v| !v.is_empty()) {
            return ColorDepth::NoColor;
        }
        let term = get("TERM").unwrap_or_default().to_ascii_lowercase();
        if term.is_empty() || term == "dumb" {
            return ColorDepth::NoColor;
        }
        let colorterm = get("COLORTERM").unwrap_or_default().to_ascii_lowercase();
        if colorterm.contains("truecolor") || colorterm.contains("24bit") || term.contains("direct") {
            return ColorDepth::TrueColor;
        }
        if term.contains("256") {
            return ColorDepth::Ansi256;
        }
        ColorDepth::Ansi16
    }

    /// Depth from the process environment.
    pub fn detect() -> ColorDepth {
        ColorDepth::from_env(|k| std::env::var(k).ok())
    }

    /// Whether any colour is shown.
    pub fn has_color(self) -> bool {
        self != ColorDepth::NoColor
    }
}

#[cfg(test)]
mod tests {
    use super::ColorDepth::{self, *};

    fn depth(vars: &[(&str, &str)]) -> ColorDepth {
        ColorDepth::from_env(|k| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string()))
    }

    #[test]
    fn no_color_wins_over_every_rich_signal() {
        assert_eq!(depth(&[("NO_COLOR", "1"), ("COLORTERM", "truecolor"), ("TERM", "xterm-256color")]), NoColor);
    }

    #[test]
    fn an_empty_no_color_is_ignored() {
        assert_eq!(depth(&[("NO_COLOR", ""), ("COLORTERM", "truecolor"), ("TERM", "xterm")]), TrueColor);
    }

    #[test]
    fn dumb_empty_and_missing_term_mean_no_colour() {
        assert_eq!(depth(&[("TERM", "dumb"), ("COLORTERM", "truecolor")]), NoColor);
        assert_eq!(depth(&[("TERM", ""), ("COLORTERM", "truecolor")]), NoColor);
        assert_eq!(depth(&[("COLORTERM", "truecolor")]), NoColor);
        assert_eq!(depth(&[]), NoColor);
    }

    #[test]
    fn truecolor_signals() {
        assert_eq!(depth(&[("TERM", "xterm"), ("COLORTERM", "truecolor")]), TrueColor);
        assert_eq!(depth(&[("TERM", "xterm"), ("COLORTERM", "24bit")]), TrueColor);
        assert_eq!(depth(&[("TERM", "xterm"), ("COLORTERM", "TrueColor")]), TrueColor);
        assert_eq!(depth(&[("TERM", "xterm-direct")]), TrueColor);
    }

    #[test]
    fn term_256_and_16() {
        assert_eq!(depth(&[("TERM", "xterm-256color")]), Ansi256);
        assert_eq!(depth(&[("TERM", "screen-256color"), ("COLORTERM", "")]), Ansi256);
        assert_eq!(depth(&[("TERM", "xterm")]), Ansi16);
        assert_eq!(depth(&[("TERM", "linux")]), Ansi16);
    }
}
