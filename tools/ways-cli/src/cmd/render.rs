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
    pub fn detect() -> Self {
        let term_w = agent_fmt::terminal_width();
        // Fixed columns: epoch(6) + dist(6) + trigger(12) + pin(2) + redisclosure(14) + agent(14) + spaces(6) = 60
        let fixed_cols = 60;
        let indent = 2;
        // Way column gets everything left after fixed columns
        let way_col = term_w.saturating_sub(indent + fixed_cols).max(20);
        let bar_width = term_w.saturating_sub(indent + 4).clamp(30, 200);
        let separator = term_w.saturating_sub(indent + 2);
        Layout { way_col, bar_width, separator }
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

/// Render table header.
pub fn write_table_header(out: &mut String) {
    let layout = Layout::detect();
    write_table_header_with(out, &layout);
}

/// Render table header with explicit layout.
pub fn write_table_header_with(out: &mut String, layout: &Layout) {
    let _ = writeln!(
        out,
        "  \x1b[1m{:<w$} {:>5} {:>5} {:<11} \u{2316} {:<13} Agent\x1b[0m",
        "Way", "Epoch", "Dist", "Trigger", "Re-disclosure",
        w = layout.way_col
    );
    let _ = writeln!(out, "  \x1b[2m{}\x1b[0m", "─".repeat(layout.separator));
}

/// Render a single way row.
#[allow(clippy::too_many_arguments)]
pub fn write_way_row<W: WayRow>(
    out: &mut String,
    w: &W,
    current_epoch: u64,
    current_tokens_k: u64,
    bar_positions: &[Option<usize>],
    unique_pos: &[usize],
    index: usize,
    row_prefix: &str,
    row_suffix: &str,
) {
    let layout = Layout::detect();
    write_way_row_with(out, w, current_epoch, current_tokens_k,
        bar_positions, unique_pos, index, row_prefix, row_suffix, &layout);
}

/// Render a single way row with explicit layout.
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
    let next_padded = crate::cmd::compositor::pad_visible(&next, 13);

    let _ = writeln!(
        out,
        "  {row_prefix}{:<w$} {:>5} {}{:>5}\x1b[0m {:<11} {} {} {}{row_suffix}",
        truncate(&display_id, layout.way_col),
        w.epoch_fired(),
        dist_color,
        distance,
        trigger_display,
        pin,
        next_padded,
        agent_display,
        w = layout.way_col
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
