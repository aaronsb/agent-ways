//! Micro-compositor for terminal panels (ADR-154 §2).
//!
//! A tiny layout layer over the ANSI-`String` panels that `cmd/render` already
//! produces. A [`Panel`] is a list of ANSI-styled lines plus its visible width;
//! the helpers place panels side by side ([`hjoin`]), scroll a panel to a
//! fixed-height viewport ([`Panel::viewport`]), and render a [`tab_bar`]. This is
//! the deliberate zero-dependency alternative to ratatui (ADR-154 §2), right-sized
//! for "list-left / detail-right with independent scroll" and "table + status bar"
//! — the shapes the introspect drill-down needs. If the inspector ever grows to
//! need text selection or resizable/mouse panes, the ADR's escape hatch to ratatui
//! applies; short of that, this stays.
//!
//! It owns the canonical ANSI-visible-width primitives ([`visible_len`],
//! [`truncate_visible`], [`pad_visible`]) that `render.rs` (`ansi_pad`/
//! `ansi_visible_len`) and `rethink.rs` (`truncate_visible`) each grew their own
//! copies of. Width is measured in `char`s, ignoring SGR escapes — the same grain
//! those callers already use; East-Asian double-width and combining marks are not
//! accounted for (that would need a new dependency the lean binary declines).

use std::fmt::Write;

// ── ANSI-visible-width primitives ─────────────────────────────

/// Visible length of `s` in `char`s, ignoring ANSI escape sequences (`\x1b[…X`,
/// terminated by an ASCII letter — covers the SGR color codes our panels use).
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

/// Truncate `s` to at most `max` **visible** chars, preserving ANSI escapes
/// (escape bytes don't count toward the budget). If truncation cuts the string
/// mid-style, a reset (`\x1b[0m`) is appended so styling can't bleed past the cut.
pub fn truncate_visible(s: &str, max: usize) -> String {
    let mut result = String::new();
    let mut visible = 0;
    let mut in_escape = false;
    let mut truncated = false;

    for c in s.chars() {
        if in_escape {
            result.push(c);
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
            continue;
        }
        if c == '\x1b' {
            in_escape = true;
            result.push(c);
            continue;
        }
        if visible >= max {
            truncated = true;
            break;
        }
        result.push(c);
        visible += 1;
    }
    if truncated && result.contains('\x1b') {
        result.push_str("\x1b[0m");
    }
    result
}

/// Right-pad `s` with spaces to a fixed visible `width`. Never truncates — a line
/// already wider than `width` is returned unchanged (use [`truncate_visible`]
/// first if a hard cap is needed).
pub fn pad_visible(s: &str, width: usize) -> String {
    let vis = visible_len(s);
    if vis >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - vis))
    }
}

/// Fit `s` to exactly `width` visible chars: truncate if longer, pad if shorter.
pub fn fit_visible(s: &str, width: usize) -> String {
    pad_visible(&truncate_visible(s, width), width)
}

// ── Panel ─────────────────────────────────────────────────────

/// A rectangular block of ANSI-styled text: its `lines` and their common visible
/// `width`. Construction records the width so downstream placement doesn't have to
/// re-measure the whole block.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Panel {
    pub lines: Vec<String>,
    pub width: usize,
}

impl Panel {
    /// A panel from raw lines; `width` is the widest line's visible length.
    pub fn from_lines(lines: Vec<String>) -> Self {
        let width = lines.iter().map(|l| visible_len(l)).max().unwrap_or(0);
        Panel { lines, width }
    }

    /// A panel from a `\n`-separated block (e.g. a `render.rs` buffer). A trailing
    /// newline does not add a blank final line.
    pub fn from_text(text: &str) -> Self {
        Self::from_lines(text.lines().map(str::to_string).collect())
    }

    /// Override the panel's declared `width` — the column width [`hjoin`] pads and
    /// truncates each line to. Use when a column must hold a fixed width regardless
    /// of content, so a side-by-side divider stays put across frames and long lines
    /// are clipped to the column instead of overflowing it.
    pub fn fixed_width(mut self, width: usize) -> Self {
        self.width = width;
        self
    }

    pub fn height(&self) -> usize {
        self.lines.len()
    }

    /// A fixed-height viewport onto this panel: exactly `height` lines starting
    /// `offset` lines down, blank-padded if the content is shorter or the offset
    /// runs past the end. Width is preserved, so viewports of unequal-length panels
    /// still align when placed side by side. `offset` is clamped to a valid start.
    pub fn viewport(&self, offset: usize, height: usize) -> Panel {
        let offset = offset.min(self.max_scroll(height));
        let mut lines: Vec<String> = self
            .lines
            .iter()
            .skip(offset)
            .take(height)
            .cloned()
            .collect();
        while lines.len() < height {
            lines.push(String::new());
        }
        Panel { lines, width: self.width }
    }

    /// The largest scroll offset that still shows content in a `view_h`-tall
    /// viewport (`0` when the panel fits). Callers clamp their own scroll state to
    /// this so paging can't strand the viewport past the end.
    pub fn max_scroll(&self, view_h: usize) -> usize {
        self.height().saturating_sub(view_h)
    }
}

// ── Placement ─────────────────────────────────────────────────

/// Place `panels` side by side with `gap` spaces between columns, returning the
/// composited lines. Each panel's lines are padded to its own `width` so columns
/// stay aligned; a panel shorter than the tallest contributes blank rows for its
/// missing lines. Give panels equal height first (via [`Panel::viewport`]) when a
/// stable frame is wanted.
pub fn hjoin(panels: &[Panel], gap: usize) -> Vec<String> {
    let rows = panels.iter().map(Panel::height).max().unwrap_or(0);
    let spacer = " ".repeat(gap);
    (0..rows)
        .map(|r| {
            panels
                .iter()
                .map(|p| {
                    let line = p.lines.get(r).map(String::as_str).unwrap_or("");
                    fit_visible(line, p.width)
                })
                .collect::<Vec<_>>()
                .join(&spacer)
        })
        .collect()
}

/// Two-panel convenience over [`hjoin`].
pub fn hjoin2(left: &Panel, right: &Panel, gap: usize) -> Vec<String> {
    hjoin(&[left.clone(), right.clone()], gap)
}

// ── Tab bar ───────────────────────────────────────────────────

/// A one-line tab bar: each label padded with a space on each side, the `active`
/// index shown in reverse video (`\x1b[7m`), the rest dim (`\x1b[2m`). Out-of-range
/// `active` simply highlights nothing.
pub fn tab_bar(tabs: &[&str], active: usize) -> String {
    let mut out = String::new();
    for (i, label) in tabs.iter().enumerate() {
        if i == active {
            let _ = write!(out, "\x1b[7m {label} \x1b[0m");
        } else {
            let _ = write!(out, "\x1b[2m {label} \x1b[0m");
        }
    }
    out
}

// ── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const RED: &str = "\x1b[0;31m";
    const RST: &str = "\x1b[0m";

    #[test]
    fn visible_len_ignores_ansi() {
        assert_eq!(visible_len("abc"), 3);
        assert_eq!(visible_len(&format!("{RED}abc{RST}")), 3);
        assert_eq!(visible_len(""), 0);
        // Truecolor SGR (ends in 'm') is fully skipped.
        assert_eq!(visible_len("\x1b[38;2;99;179;237m●\x1b[0m"), 1);
    }

    #[test]
    fn truncate_visible_counts_only_visible_and_seals_style() {
        assert_eq!(truncate_visible("abcdef", 3), "abc");
        assert_eq!(truncate_visible("abc", 10), "abc"); // shorter → unchanged
        // Styled content truncated mid-style gets a reset appended.
        let t = truncate_visible(&format!("{RED}abcdef{RST}"), 3);
        assert_eq!(visible_len(&t), 3);
        assert!(t.ends_with(RST), "truncation must seal the style: {t:?}");
        // Not truncated → no extra reset beyond the original.
        let whole = truncate_visible(&format!("{RED}ab{RST}"), 5);
        assert_eq!(whole, format!("{RED}ab{RST}"));
    }

    #[test]
    fn pad_and_fit_reach_exact_visible_width() {
        assert_eq!(pad_visible("ab", 5), "ab   ");
        assert_eq!(visible_len(&pad_visible(&format!("{RED}ab{RST}"), 5)), 5);
        assert_eq!(pad_visible("abcde", 3), "abcde"); // wider → unchanged
        // fit both truncates and pads to land exactly on width.
        assert_eq!(visible_len(&fit_visible("abcdef", 4)), 4);
        assert_eq!(visible_len(&fit_visible("ab", 4)), 4);
    }

    #[test]
    fn panel_from_lines_measures_widest() {
        let p = Panel::from_lines(vec!["a".into(), "abcd".into(), "ab".into()]);
        assert_eq!(p.width, 4);
        assert_eq!(p.height(), 3);
        // ANSI doesn't inflate the measured width.
        let styled = Panel::from_lines(vec![format!("{RED}abc{RST}")]);
        assert_eq!(styled.width, 3);
    }

    #[test]
    fn from_text_no_trailing_blank() {
        let p = Panel::from_text("one\ntwo\n");
        assert_eq!(p.lines, vec!["one", "two"]);
    }

    #[test]
    fn fixed_width_overrides_measured_and_hjoin_clips_to_it() {
        // A content-measured width of 4, forced down to 2.
        let p = Panel::from_lines(vec!["abcd".into()]).fixed_width(2);
        assert_eq!(p.width, 2);
        // hjoin then truncates the over-long line to the forced column width.
        let rows = hjoin(&[p], 0);
        assert_eq!(rows[0], "ab");
        // Forced wider than content pads out to the column.
        let wide = Panel::from_lines(vec!["x".into()]).fixed_width(4);
        assert_eq!(hjoin(&[wide], 0)[0], "x   ");
    }

    #[test]
    fn viewport_is_fixed_height_and_clamps_offset() {
        let p = Panel::from_lines((0..5).map(|i| i.to_string()).collect());
        // Window in the middle.
        let v = p.viewport(1, 3);
        assert_eq!(v.lines, vec!["1", "2", "3"]);
        assert_eq!(v.width, 1);
        // Shorter content → blank-padded to the requested height.
        let short = Panel::from_lines(vec!["x".into()]);
        let vs = short.viewport(0, 3);
        assert_eq!(vs.lines, vec!["x", "", ""]);
        // Offset past the end is clamped so content still shows.
        let clamped = p.viewport(99, 2);
        assert_eq!(clamped.lines, vec!["3", "4"]);
    }

    #[test]
    fn max_scroll_bounds_paging() {
        let p = Panel::from_lines((0..5).map(|i| i.to_string()).collect());
        assert_eq!(p.max_scroll(3), 2); // 5 lines, 3-tall view → last start is 2
        assert_eq!(p.max_scroll(5), 0); // exactly fits
        assert_eq!(p.max_scroll(10), 0); // taller than content
    }

    #[test]
    fn hjoin_aligns_columns_and_pads_short_panel() {
        let left = Panel::from_lines(vec!["aa".into(), "b".into()]);
        let right = Panel::from_lines(vec!["1".into(), "22".into(), "3".into()]);
        let rows = hjoin(&[left, right], 1);
        assert_eq!(rows.len(), 3, "tallest panel sets the row count");
        // Left padded to width 2, one space gap, right padded to width 2.
        assert_eq!(rows[0], "aa 1 ");
        assert_eq!(rows[1], "b  22");
        assert_eq!(rows[2], "   3 "); // left ran out → blank of its width
    }

    #[test]
    fn hjoin_keeps_alignment_under_ansi() {
        let left = Panel::from_lines(vec![format!("{RED}aa{RST}")]);
        let right = Panel::from_lines(vec!["z".into()]);
        let rows = hjoin(&[left, right], 2);
        // Visible width: 2 (left) + 2 (gap) + 1 (right) = 5.
        assert_eq!(visible_len(&rows[0]), 5);
    }

    #[test]
    fn tab_bar_highlights_active_only() {
        let bar = tab_bar(&["Replay", "Why"], 1);
        assert!(bar.contains("\x1b[7m Why \x1b[0m"), "active is reversed");
        assert!(bar.contains("\x1b[2m Replay \x1b[0m"), "inactive is dim");
        // Out-of-range active highlights nothing (no reverse-video sequence).
        assert!(!tab_bar(&["a", "b"], 9).contains("\x1b[7m"));
    }
}
