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
//! Widths are measured by the ANSI-visible-width primitives in `agent_fmt`
//! (`visible_len`, `clip_visible`, `pad_visible`, `fit_visible`).

// ── Panel ─────────────────────────────────────────────────────

/// A rectangular block of ANSI-styled text: its `lines` and their common visible
/// `width`. Construction records the width so downstream placement doesn't have to
/// re-measure the whole block.
#[cfg(feature = "tui")]
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Panel {
    pub lines: Vec<String>,
    pub width: usize,
}

#[cfg(feature = "tui")]
impl Panel {
    /// A panel from raw lines; `width` is the widest line's visible length.
    pub fn from_lines(lines: Vec<String>) -> Self {
        let width = lines.iter().map(|l| agent_fmt::visible_len(l)).max().unwrap_or(0);
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
#[cfg(feature = "tui")]
pub fn hjoin(panels: &[Panel], gap: usize) -> Vec<String> {
    let rows = panels.iter().map(Panel::height).max().unwrap_or(0);
    let spacer = " ".repeat(gap);
    (0..rows)
        .map(|r| {
            panels
                .iter()
                .map(|p| {
                    let line = p.lines.get(r).map(String::as_str).unwrap_or("");
                    agent_fmt::fit_visible(line, p.width)
                })
                .collect::<Vec<_>>()
                .join(&spacer)
        })
        .collect()
}

/// Two-panel convenience over [`hjoin`].
#[cfg(feature = "tui")]
pub fn hjoin2(left: &Panel, right: &Panel, gap: usize) -> Vec<String> {
    hjoin(&[left.clone(), right.clone()], gap)
}

// ── Tab bar ───────────────────────────────────────────────────

/// A one-line tab bar: each label padded with a space on each side, the `active`
/// index in the selection style (reverse video by default), the rest muted.
/// Out-of-range `active` simply highlights nothing.
#[cfg(feature = "tui")]
pub fn tab_bar(tabs: &[&str], active: usize) -> String {
    let mut out = String::new();
    for (i, label) in tabs.iter().enumerate() {
        if i == active {
            out.push_str(&agent_theme::paint(agent_theme::Role::Selection, format!(" {label} ")));
        } else {
            out.push_str(&agent_theme::paint(agent_theme::Role::Muted, format!(" {label} ")));
        }
    }
    out
}

// ── Tests ─────────────────────────────────────────────────────

#[cfg(all(test, feature = "tui"))]
mod tests {
    use super::*;

    use agent_theme::{ColorDepth, Painter, Role};

    fn pinned() -> Painter {
        Painter::terminal(ColorDepth::TrueColor)
    }

    fn red(s: &str) -> String {
        pinned().paint(Role::Err, s)
    }

    #[test]
    fn panel_from_lines_measures_widest() {
        let p = Panel::from_lines(vec!["a".into(), "abcd".into(), "ab".into()]);
        assert_eq!(p.width, 4);
        assert_eq!(p.height(), 3);
        // ANSI doesn't inflate the measured width.
        let styled = Panel::from_lines(vec![red("abc")]);
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
        let left = Panel::from_lines(vec![red("aa")]);
        let right = Panel::from_lines(vec!["z".into()]);
        let rows = hjoin(&[left, right], 2);
        // Visible width: 2 (left) + 2 (gap) + 1 (right) = 5.
        assert_eq!(agent_fmt::visible_len(&rows[0]), 5);
    }

    #[test]
    fn tab_bar_highlights_active_only() {
        let _g = agent_theme::scoped(pinned());
        let bar = tab_bar(&["Replay", "Why"], 1);
        assert!(bar.contains(&pinned().paint(Role::Selection, " Why ")), "active is reversed");
        assert!(bar.contains(&pinned().paint(Role::Muted, " Replay ")), "inactive is dim");
        // Out-of-range active highlights nothing (no reverse-video sequence).
        assert!(!tab_bar(&["a", "b"], 9).contains(&pinned().sgr(Role::Selection)));
    }
}
