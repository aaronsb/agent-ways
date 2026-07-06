//! Shared rendering for ways list display.
//!
//! Used by both `list` (live session) and `rethink` (replay).
//! All rendering writes to a String buffer; callers handle output.

use std::fmt::Write;

// ── Way trait ─────────────────────────────────────────────────

/// Common accessors for rendering a way row.
pub trait WayRow {
    fn id(&self) -> &str;
    fn epoch_fired(&self) -> u64;
    fn token_pos(&self) -> u64;
    fn trigger(&self) -> &str;
    fn check_fires(&self) -> u64;
    fn depth(&self) -> u64 { 0 }
    fn agent_id(&self) -> &str { "main" }
    /// Per-way re-fire distance in thousands of tokens, derived from
    /// this way's ADR-123 `Curve::refire_delta(REFIRE_FLOOR)`. Replaces
    /// the pre-ADR-123 shared `redisclose_threshold_k` constant — each
    /// way now renders against its own curve.
    fn refire_threshold_k(&self) -> u64;
}

// ── Constants ─────────────────────────────────────────────────

pub const PIN_SYMBOLS: [char; 10] = ['●', '◆', '■', '▲', '◉', '▶', '★', '◈', '♦', '▪'];

pub const PIN_COLORS: [&str; 10] = [
    "\x1b[38;2;99;179;237m",  // blue
    "\x1b[38;2;78;205;196m",  // teal
    "\x1b[38;2;126;211;33m",  // green
    "\x1b[38;2;255;234;167m", // yellow
    "\x1b[38;2;253;203;110m", // orange
    "\x1b[38;2;255;118;117m", // red
    "\x1b[38;2;162;155;254m", // purple
    "\x1b[38;2;253;121;168m", // magenta
    "\x1b[38;2;116;185;255m", // sky
    "\x1b[38;2;85;239;196m",  // mint
];

// ── Layout ───────────────────────────────────────────────────

// Trailing table columns (everything right of the Way column) and the spacing
// model. The header and row renderers both read these constants, so the two
// format strings can never drift out of alignment.
const EPOCH_W: usize = 5;
const DIST_W: usize = 5;
const TRIG_W: usize = 13;
const PIN_W: usize = 1;
const RD_W: usize = 13;
const AGENT_W: usize = 12;
/// Blank columns between every pair of table columns — the breathing room that
/// keeps Epoch/Dist/Trigger/… from reading as one crowded block.
const COL_GAP: usize = 2;
const INDENT: usize = 2;

/// Nominal visible width of the fixed Epoch…Agent block that trails the Way
/// column, inter-column gaps included. It's the reservation the ceiling clamp
/// leaves for the trailing columns — *not* a hard bound: the Trigger and
/// Re-disclosure cells pad but never truncate (`{:<}` / `pad_visible`), so a
/// long trigger (`embed:bash:multi`) or a verbose re-disclosure string can
/// overflow its column and push the block past this width. Header and rows stay
/// mutually aligned for content that fits; over-width content drifts equally in
/// both. (Pre-existing behaviour; documented here because the reservation reads
/// like a guarantee otherwise.)
const TRAILING_W: usize = COL_GAP + EPOCH_W + COL_GAP + DIST_W + COL_GAP + TRIG_W
    + COL_GAP + PIN_W + COL_GAP + RD_W + COL_GAP + AGENT_W;

/// The Way column grows in fixed steps of this many columns: its width snaps up
/// to the next stop past the longest id, so a name a few characters longer than
/// its neighbours never reflows the whole table. The column changes width only
/// when the longest id crosses a stop — "tabbed", and visually stable frame to
/// frame.
const WAY_TAB: usize = 4;
/// Floor for the Way column so a frame of only short ids still reads as a column.
const WAY_MIN: usize = 16;

/// Round `n` up to the next multiple of `step` (the tab-stop snap).
fn snap_up(n: usize, step: usize) -> usize {
    if step == 0 {
        return n;
    }
    n.div_ceil(step) * step
}

/// Visible width of a way's rendered id, including the depth-indent prefix
/// (`  └ `) that `write_way_row_with` prepends for nested agents.
fn display_id_width<W: WayRow>(w: &W) -> usize {
    let prefix = if w.depth() > 0 { w.depth() as usize * 2 + 2 } else { 0 };
    prefix + w.id().chars().count()
}

/// Computed layout dimensions derived from terminal width.
pub struct Layout {
    /// Width of the Way ID column
    pub way_col: usize,
    /// Width of the progress/forecast bar
    pub bar_width: usize,
    /// Total separator width
    pub separator: usize,
}

impl Layout {
    /// Reactive layout sized to `max_id_width` (the widest rendered way id),
    /// using the detected terminal width. See [`Layout::for_id_width_in`].
    pub fn for_id_width(max_id_width: usize) -> Self {
        Self::for_id_width_in(max_id_width, agent_fmt::terminal_width())
    }

    /// Reactive layout for an explicit `term_w`: the Way column snaps to a tab
    /// stop just past `max_id_width` rather than absorbing all leftover width, so
    /// short names no longer leave a gulf before the Epoch column. It is floored
    /// at `WAY_MIN` and capped at a `ceiling` that reserves room for the trailing
    /// columns.
    ///
    /// The tab-stop guarantee holds only while the snapped width fits under the
    /// ceiling. On a terminal too narrow for a full-width id (`ceiling` binds),
    /// `way_col == ceiling`, which need not land on a stop — the column stops
    /// being "tabbed" and simply takes all the room there is. Header and rows
    /// still agree (both read this one `way_col`), so alignment is preserved;
    /// only the frame-to-frame stability degrades. Taking the width as a
    /// parameter also keeps the layout tests independent of the ambient terminal.
    pub fn for_id_width_in(max_id_width: usize, term_w: usize) -> Self {
        // Never let the Way column crowd the trailing columns off the terminal.
        let ceiling = term_w.saturating_sub(INDENT + TRAILING_W).max(WAY_MIN);
        let way_col = snap_up(max_id_width + 1, WAY_TAB).clamp(WAY_MIN, ceiling);
        let bar_width = term_w.saturating_sub(INDENT + 4).clamp(30, 200);
        let separator = term_w.saturating_sub(INDENT + 2);
        Layout { way_col, bar_width, separator }
    }

    /// Reactive layout sized to the widest way id in `ways` (depth-indent prefix
    /// included). The common entry point for the table renderers.
    pub fn for_rows<W: WayRow>(ways: &[W]) -> Self {
        let max_id = ways.iter().map(display_id_width).max().unwrap_or(0);
        Self::for_id_width(max_id)
    }

    /// Bar-only layout for callers that render the timeline bar but no table rows
    /// (they read `bar_width`/`separator` only); the Way column falls back to its
    /// floor since there is no content to measure.
    pub fn detect() -> Self {
        Self::for_id_width(0)
    }
}

// ── Table rendering ───────────────────────────────────────────

/// Compute bar positions for each way's re-disclosure point. Each way
/// uses its own `refire_threshold_k`, so the resulting positions reflect
/// per-curve schedules instead of a shared step.
pub fn compute_bar_positions<W: WayRow>(
    ways: &[W],
    context_window_k: u64,
) -> Vec<Option<usize>> {
    let bw = Layout::detect().bar_width;
    ways.iter()
        .map(|w| {
            if context_window_k == 0 {
                return None;
            }
            let fire_pos_k = w.token_pos() / 1000;
            let redisclose_at_k = fire_pos_k + w.refire_threshold_k();
            let bar_pos = ((redisclose_at_k * bw as u64) / context_window_k) as usize;
            Some(bar_pos.min(bw - 1))
        })
        .collect()
}

/// Deduplicated sorted positions for cluster assignment.
pub fn unique_positions(bar_positions: &[Option<usize>]) -> Vec<usize> {
    let mut positions: Vec<usize> = bar_positions.iter().filter_map(|p| *p).collect();
    positions.sort();
    positions.dedup();
    positions
}

/// Map a bar position to its cluster index.
pub fn cluster_of(bar_pos: usize, unique_positions: &[usize]) -> usize {
    unique_positions
        .iter()
        .position(|&p| p == bar_pos)
        .unwrap_or(0)
        % PIN_SYMBOLS.len()
}

/// Render pin symbol for a cluster index.
pub fn pin_str(cluster_idx: usize) -> String {
    format!(
        "{}{}\x1b[0m",
        PIN_COLORS[cluster_idx % PIN_COLORS.len()],
        PIN_SYMBOLS[cluster_idx % PIN_SYMBOLS.len()]
    )
}

/// Render table header against a content-sized layout (`Layout::for_rows`).
pub fn write_table_header_with(out: &mut String, layout: &Layout) {
    let g = " ".repeat(COL_GAP);
    let _ = writeln!(
        out,
        "  \x1b[1m{way:<way_w$}{g}{ep:>ep_w$}{g}{di:>di_w$}{g}{tr:<tr_w$}{g}{pin:^pin_w$}{g}{rd:<rd_w$}{g}Agent\x1b[0m",
        way = "Way", ep = "Epoch", di = "Dist", tr = "Trigger",
        pin = "\u{2316}", rd = "Re-disclosure",
        way_w = layout.way_col, ep_w = EPOCH_W, di_w = DIST_W,
        tr_w = TRIG_W, pin_w = PIN_W, rd_w = RD_W,
    );
    let _ = writeln!(out, "  \x1b[2m{}\x1b[0m", "─".repeat(layout.separator));
}

/// Render a single way row against a content-sized layout (`Layout::for_rows`).
#[allow(clippy::too_many_arguments)]
pub fn write_way_row_with<W: WayRow>(
    out: &mut String,
    w: &W,
    current_epoch: u64,
    current_tokens_k: u64,
    bar_positions: &[Option<usize>],
    unique_pos: &[usize],
    index: usize,
    row_prefix: &str,
    row_suffix: &str,
    layout: &Layout,
) {
    let distance = current_epoch.saturating_sub(w.epoch_fired());
    let next = predict_next(w, current_epoch, current_tokens_k);

    let prefix = if w.depth() > 0 {
        format!("{}{}", "  ".repeat(w.depth() as usize), "└ ")
    } else {
        String::new()
    };
    let display_id = format!("{prefix}{}", w.id());
    let trigger_display = format_trigger(w.trigger());

    let dist_color = if distance == 0 || (current_epoch > 0 && distance < current_epoch / 3) {
        "\x1b[0;32m"
    } else if current_epoch > 0 && distance < current_epoch * 2 / 3 {
        "\x1b[1;33m"
    } else {
        "\x1b[0;31m"
    };

    let pin = if let Some(bar_pos) = bar_positions.get(index).copied().flatten() {
        pin_str(cluster_of(bar_pos, unique_pos))
    } else {
        " ".to_string()
    };

    let agent_display = if w.agent_id() == "main" {
        "\x1b[2mmain\x1b[0m".to_string()
    } else {
        let aid = w.agent_id();
        if aid.len() > 12 { format!("{}…", &aid[..11]) } else { aid.to_string() }
    };

    // Pad re-disclosure to fixed visible width (ANSI-aware)
    let next_padded = crate::cmd::compositor::pad_visible(&next, RD_W);

    let g = " ".repeat(COL_GAP);
    let _ = writeln!(
        out,
        "  {row_prefix}{way:<way_w$}{g}{ep:>ep_w$}{g}{dc}{di:>di_w$}\x1b[0m{g}{tr:<tr_w$}{g}{pin}{g}{rd}{g}{agent}{row_suffix}",
        way = truncate(&display_id, layout.way_col),
        ep = w.epoch_fired(),
        dc = dist_color,
        di = distance,
        tr = trigger_display,
        pin = pin,
        rd = next_padded,
        agent = agent_display,
        way_w = layout.way_col, ep_w = EPOCH_W, di_w = DIST_W, tr_w = TRIG_W,
    );

    if w.check_fires() > 0 {
        let decay = 1.0 / (w.check_fires() as f64 + 1.0);
        let _ = writeln!(
            out,
            "  \x1b[2m  ✓ check ({} fires, decay={:.2})\x1b[0m",
            w.check_fires(),
            decay
        );
    }
}

// ── Token timeline ────────────────────────────────────────────

/// Render the full token timeline: usage bar, forecast, zone summary.
pub fn write_token_timeline<W: WayRow>(
    out: &mut String,
    ways: &[W],
    unique_pos: &[usize],
    current_tokens_k: u64,
    context_window_k: u64,
) {
    let layout = Layout::detect();
    let bar_width = layout.bar_width;

    let pct = if context_window_k > 0 {
        (current_tokens_k * 100 / context_window_k).min(100)
    } else {
        0
    };
    let filled = (pct as usize * bar_width / 100).min(bar_width);

    struct RdPoint {
        at_k: u64,
        cluster: usize,
        past: bool,
    }
    let mut points: Vec<RdPoint> = Vec::new();
    let mut zone_past = 0u32;
    let mut zone_soon = 0u32;
    let mut zone_later = 0u32;

    for w in ways {
        let threshold_k = w.refire_threshold_k();
        let fire_pos_k = w.token_pos() / 1000;
        let redisclose_at_k = fire_pos_k + threshold_k;
        let past = current_tokens_k >= redisclose_at_k;

        let full_bar_pos = if context_window_k > 0 {
            ((redisclose_at_k * bar_width as u64) / context_window_k) as usize
        } else {
            0
        }
        .min(bar_width - 1);

        let ci = cluster_of(full_bar_pos, unique_pos);

        points.push(RdPoint {
            at_k: redisclose_at_k,
            cluster: ci,
            past,
        });

        if past {
            zone_past += 1;
        } else {
            let dist = redisclose_at_k.saturating_sub(current_tokens_k);
            if threshold_k > 0 && dist <= threshold_k / 4 {
                zone_soon += 1;
            } else {
                zone_later += 1;
            }
        }
    }

    let future_points: Vec<&RdPoint> = points.iter().filter(|p| !p.past).collect();

    let (zoom_start, zoom_end, zoom_span) = if !future_points.is_empty() {
        let min_rd = future_points.iter().map(|p| p.at_k).min().unwrap_or(current_tokens_k);
        let max_rd = future_points.iter().map(|p| p.at_k).max().unwrap_or(context_window_k);
        let zs = current_tokens_k;
        let ze = (max_rd + (max_rd - min_rd) / 4).min(context_window_k);
        (zs, ze, ze.saturating_sub(zs).max(1))
    } else {
        (0, 0, 0)
    };

    // Usage bar
    let bar_color = if pct < 50 {
        "\x1b[0;32m"
    } else if pct < 75 {
        "\x1b[1;33m"
    } else {
        "\x1b[0;31m"
    };

    let zoom_bar_start = if context_window_k > 0 && zoom_span > 0 {
        ((zoom_start * bar_width as u64) / context_window_k) as usize
    } else {
        0
    };
    let zoom_bar_end = if context_window_k > 0 && zoom_span > 0 {
        ((zoom_end * bar_width as u64) / context_window_k) as usize
    } else {
        0
    }
    .min(bar_width.saturating_sub(1));

    let mut bar = String::new();
    for i in 0..bar_width {
        if i < filled {
            bar.push('█');
        } else {
            bar.push('░');
        }
    }
    let _ = writeln!(
        out,
        "  {bar_color}{bar}\x1b[0m {pct}% ({current_tokens_k}K / {context_window_k}K)"
    );

    // Zoom boundary arrows
    if zoom_span > 0 {
        let mut arrow_line = String::from("  ");
        for i in 0..bar_width {
            if i == zoom_bar_start || i == zoom_bar_end {
                arrow_line.push('^');
            } else {
                arrow_line.push(' ');
            }
        }
        let _ = writeln!(out, "\x1b[2m{arrow_line}\x1b[0m");
    }

    // Forecast
    if !future_points.is_empty() {
        let mut zoom_markers: Vec<Option<usize>> = vec![None; bar_width];
        for p in &future_points {
            let offset = p.at_k.saturating_sub(zoom_start);
            let pos = ((offset * bar_width as u64) / zoom_span) as usize;
            let pos = pos.min(bar_width - 1);
            if zoom_markers[pos].is_none() {
                zoom_markers[pos] = Some(p.cluster);
            }
        }

        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "  \x1b[1mForecast\x1b[0m \x1b[2m({zoom_start}K → {zoom_end}K)\x1b[0m"
        );

        let mut marker_str = String::from("  ");
        for marker in &zoom_markers[..bar_width] {
            match marker {
                Some(ci) => marker_str.push_str(&pin_str(*ci)),
                None => marker_str.push('·'),
            }
        }
        let _ = writeln!(out, "{marker_str}");

        // Scale labels
        let mid_k = zoom_start + zoom_span / 2;
        let mid_pos = bar_width / 2;
        let end_label = format!("{zoom_end}K");
        let end_pos = bar_width - end_label.len();
        let mut label_line = String::from("  ");
        let start_label = format!("{zoom_start}K");
        label_line.push_str(&format!("\x1b[2m{start_label}"));
        let pad1 = mid_pos.saturating_sub(start_label.len());
        label_line.push_str(&" ".repeat(pad1));
        let mid_label = format!("{mid_k}K");
        label_line.push_str(&mid_label);
        let pad2 = end_pos.saturating_sub(mid_pos + mid_label.len());
        label_line.push_str(&" ".repeat(pad2));
        label_line.push_str(&end_label);
        label_line.push_str("\x1b[0m");
        let _ = writeln!(out, "{label_line}");
    }

    // Zone summary
    let mut zones = Vec::new();
    if zone_past > 0 {
        zones.push(format!("\x1b[0;32m● {zone_past} re-disclose now\x1b[0m"));
    }
    if zone_soon > 0 {
        zones.push(format!("\x1b[1;33m◐ {zone_soon} approaching\x1b[0m"));
    }
    if zone_later > 0 {
        zones.push(format!("\x1b[2m○ {zone_later} distant\x1b[0m"));
    }

    if !zones.is_empty() {
        // Summarize per-way re-fire intervals. Identical thresholds
        // render as a single "NK interval"; heterogeneous ones render
        // as a "min–max K intervals" range so the per-way curve story
        // stays visible at a glance.
        let thresholds: Vec<u64> = ways.iter().map(|w| w.refire_threshold_k()).collect();
        let interval_label = if thresholds.is_empty() {
            String::from("—")
        } else {
            let min = *thresholds.iter().min().unwrap();
            let max = *thresholds.iter().max().unwrap();
            if min == max {
                format!("{min}K interval")
            } else {
                format!("{min}–{max}K intervals")
            }
        };
        let _ = writeln!(
            out,
            "  {}  \x1b[2m│ {interval_label}\x1b[0m",
            zones.join("  ")
        );
        let _ = writeln!(
            out,
            "  \x1b[2mnow = past threshold, will re-inject on next match  │  approaching = near threshold  │  distant = far from re-injection\x1b[0m"
        );
    }
}

// ── Shared helpers ────────────────────────────────────────────

/// Predict when a way will next re-disclose against its own curve.
pub fn predict_next<W: WayRow>(
    w: &W,
    current_epoch: u64,
    current_tokens_k: u64,
) -> String {
    let threshold_k = w.refire_threshold_k();
    let token_pos_k = w.token_pos() / 1000;
    let token_distance_k = current_tokens_k.saturating_sub(token_pos_k);
    let token_pct = if threshold_k > 0 {
        token_distance_k * 100 / threshold_k
    } else {
        0
    };

    if token_pct >= 100 {
        return "\x1b[0;32m● now\x1b[0m".to_string();
    }
    if token_pct >= 75 {
        return format!("\x1b[1;33m◐ {token_pct}%\x1b[0m");
    }
    if token_pct >= 50 {
        return format!("\x1b[2m◔ {token_pct}%\x1b[0m");
    }

    let epoch_distance = current_epoch.saturating_sub(w.epoch_fired());
    if w.check_fires() > 0 {
        let decay = 1.0 / (w.check_fires() as f64 + 1.0);
        let needed_factor = 2.0 / (3.0 * decay);
        let needed_distance = ((needed_factor - 1.0).exp() - 1.0).max(0.0) as u64;
        let next_epoch = w.epoch_fired() + needed_distance;
        if epoch_distance < needed_distance {
            if needed_distance > 500 {
                return format!(
                    "\x1b[2mcheck ~{} (suppressed)\x1b[0m",
                    fmt_epoch(next_epoch)
                );
            }
            return format!("\x1b[2mcheck at epoch ~{next_epoch}\x1b[0m");
        }
    }

    "\x1b[2m─\x1b[0m".to_string()
}

pub fn format_trigger(trigger: &str) -> String {
    match trigger {
        "semantic:embedding" | "semantic" => "embed".to_string(),
        "semantic:embedding:en" => "embed:en".to_string(),
        "semantic:embedding:multi" => "embed:multi".to_string(),
        "semantic:bash:en" => "embed:bash:en".to_string(),
        "semantic:bash:multi" => "embed:bash:multi".to_string(),
        "keyword" => "keyword".to_string(),
        "check-pull" => "check-pull".to_string(),
        "bash" | "file" | "state" => trigger.to_string(),
        _ => trigger.to_string(),
    }
}

pub fn fmt_epoch(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1e}", n as f64)
    } else if n >= 10_000 {
        format!("{}K", n / 1000)
    } else {
        format!("e{n}")
    }
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
}

// ANSI-visible-width helpers (`pad_visible`/`visible_len`) live in `cmd::compositor`
// — the canonical home — so `render`, `rethink`, and the compositor share one copy.

#[cfg(test)]
mod tests {
    use super::*;

    struct MockWay {
        id: &'static str,
        depth: u64,
    }

    impl WayRow for MockWay {
        fn id(&self) -> &str { self.id }
        fn epoch_fired(&self) -> u64 { 2 }
        fn token_pos(&self) -> u64 { 0 }
        fn trigger(&self) -> &str { "keyword" }
        fn check_fires(&self) -> u64 { 0 }
        fn depth(&self) -> u64 { self.depth }
        fn refire_threshold_k(&self) -> u64 { 40 }
    }

    #[test]
    fn snap_up_rounds_to_next_stop() {
        assert_eq!(snap_up(1, 4), 4);
        assert_eq!(snap_up(4, 4), 4);
        assert_eq!(snap_up(5, 4), 8);
        assert_eq!(snap_up(13, 4), 16);
        assert_eq!(snap_up(7, 0), 7); // degenerate step is a no-op, not a panic
    }

    #[test]
    fn display_id_width_counts_depth_prefix() {
        assert_eq!(display_id_width(&MockWay { id: "adr", depth: 0 }), 3);
        // depth 1 prepends "  └ " → 4 columns before the id.
        assert_eq!(display_id_width(&MockWay { id: "adr", depth: 1 }), 7);
    }

    #[test]
    fn way_col_is_tab_snapped_past_the_longest_id() {
        // A 25-char id snaps to the next stop above 25 (+1 gap) → 28, a multiple
        // of WAY_TAB and strictly wider than the id, so there's always a gap. Pin
        // the terminal width so the ceiling clamp doesn't bind (see `for_id_width_in`).
        let layout = Layout::for_id_width_in(25, 200);
        assert_eq!(layout.way_col % WAY_TAB, 0, "way_col lands on a tab stop");
        assert!(layout.way_col > 25, "column is wider than the longest id");
        assert!(layout.way_col <= 28, "but only snaps to the next stop, not beyond");
    }

    #[test]
    fn narrow_terminal_clamps_way_col_to_the_ceiling() {
        // When the terminal can't fit a full-width id, `way_col` clamps to the
        // ceiling (which reserves room for the trailing columns) rather than
        // snapping — the "tabbed" property yields to "use all the room there is".
        let layout = Layout::for_id_width_in(50, 80);
        let ceiling = 80 - (INDENT + TRAILING_W);
        assert_eq!(layout.way_col, ceiling, "clamped to the ceiling, not the snap");
        assert!(layout.way_col >= WAY_MIN, "still at least the floor");
    }

    #[test]
    fn way_col_never_collapses_below_the_floor() {
        // An empty frame (max id 0) still renders a real column, even on a narrow
        // terminal where the ceiling itself bottoms out at the floor.
        assert_eq!(Layout::for_id_width_in(0, 200).way_col, WAY_MIN);
        assert_eq!(Layout::for_id_width_in(0, 40).way_col, WAY_MIN);
    }

    #[test]
    fn header_and_row_columns_align() {
        // The Epoch column must begin at the same visible offset in the header and
        // in a data row — that's the guarantee the shared constants exist to keep.
        let layout = Layout::for_id_width_in(20, 200);
        let ways = [MockWay { id: "documentation/adr", depth: 0 }];
        let bar_positions = compute_bar_positions(&ways, 1000);
        let unique_pos = unique_positions(&bar_positions);

        let mut header = String::new();
        write_table_header_with(&mut header, &layout);
        let header_line = header.lines().next().unwrap();

        let mut row = String::new();
        write_way_row_with(
            &mut row, &ways[0], 2, 0, &bar_positions, &unique_pos, 0, "", "", &layout,
        );
        let row_line = row.lines().next().unwrap();

        // The Epoch field spans the same visible columns in both lines: it starts
        // `way_col + COL_GAP` columns in (after the 2-space indent) and is EPOCH_W
        // wide. Header holds the left-aligned label; the row holds the right-
        // aligned value — same span, which is exactly what alignment means.
        let epoch_at = 2 + layout.way_col + COL_GAP;
        assert_eq!(visible_substr(header_line, epoch_at, EPOCH_W), "Epoch");
        assert_eq!(visible_substr(row_line, epoch_at, EPOCH_W).trim(), "2");

        // The way names occupy the same left span, floored/padded to way_col.
        assert_eq!(visible_substr(header_line, 2, 3), "Way");
        assert_eq!(visible_substr(row_line, 2, 17), "documentation/adr");
    }

    /// The plain (ANSI-stripped) visible characters of `s` in `[start, start+len)`.
    fn visible_substr(s: &str, start: usize, len: usize) -> String {
        let mut out = String::new();
        let mut visible = 0;
        let mut in_escape = false;
        for c in s.chars() {
            if in_escape {
                if c == 'm' { in_escape = false; }
            } else if c == '\x1b' {
                in_escape = true;
            } else {
                if visible >= start && visible < start + len {
                    out.push(c);
                }
                visible += 1;
            }
        }
        out
    }
}
