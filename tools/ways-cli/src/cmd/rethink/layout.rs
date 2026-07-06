//! Screen composition: the fixed-height frame layout, the shared header and status
//! bar both views use, and terminal fitting.

#[cfg(feature = "tui")]
use std::fmt::Write;

#[cfg(feature = "tui")]
use crate::cmd::compositor;
#[cfg(feature = "tui")]
use crate::cmd::render;

#[cfg(feature = "tui")]
use super::model::{Player, View, SPEEDS};

// ── Frame renderer ────────────────────────────────────────────

/// Assemble a **fixed-height** screen so the nav legend is always pinned to the
/// bottom rows, whatever the content length or terminal size:
///
/// - `top` (header) is pinned at the top;
/// - `rows` is the scrollable region, viewported to keep `sel_line` visible and
///   blank-padded so it always fills its share of the height;
/// - `extras` (e.g. the token timeline) are pinned just above the nav, but only
///   when the terminal is tall enough to also show a few rows of the list;
/// - `nav` is pinned as the final lines.
///
/// The result is exactly `drawable` lines — the height `fit_to_terminal` will keep
/// (`term_height − 1`) — so nothing gets pushed off the bottom.
#[cfg(feature = "tui")]
fn compose_screen(
    top: Vec<String>,
    rows: Vec<String>,
    sel_line: usize,
    extras: Vec<String>,
    nav: Vec<String>,
    drawable: usize,
) -> String {
    let mut rows_area = drawable.saturating_sub(top.len() + nav.len()).max(1);

    // Show the extras only if they still leave a few rows of the list visible.
    let show_extras = !extras.is_empty() && rows_area > extras.len() + 3;
    if show_extras {
        rows_area = rows_area.saturating_sub(extras.len());
    }

    // Reserve one row of the list area for the scroll hint when the list overflows.
    let overflow = rows.len() > rows_area;
    let view_h = if overflow { rows_area.saturating_sub(1).max(1) } else { rows_area };
    let scroll = if overflow {
        let max_scroll = rows.len().saturating_sub(view_h);
        // Bottom-anchor the selected line (same behaviour as the drill-down list).
        sel_line.saturating_sub(view_h.saturating_sub(1)).min(max_scroll)
    } else {
        0
    };

    let mut lines: Vec<String> = Vec::with_capacity(drawable);
    lines.extend(top);

    let mut filled = 0;
    for r in rows.iter().skip(scroll).take(view_h) {
        lines.push(r.clone());
        filled += 1;
    }
    if overflow {
        let below = rows.len().saturating_sub(view_h + scroll);
        lines.push(format!("  \x1b[2m⋮ {scroll} above · {below} below · ↑↓\x1b[0m"));
        filled += 1;
    }
    // Pad the list area so the extras/nav stay anchored at the bottom.
    while filled < rows_area {
        lines.push(String::new());
        filled += 1;
    }

    if show_extras {
        lines.extend(extras);
    }
    lines.extend(nav);
    // On a very short terminal the pinned header + nav alone can exceed the height;
    // drop from the TOP so the nav legend is never the thing that gets clipped.
    if lines.len() > drawable {
        lines.drain(0..lines.len() - drawable);
    }
    lines.join("\n")
}

#[cfg(feature = "tui")]
pub(super) fn render_frame(player: &mut Player) -> String {
    // Clamp the shared selection to this frame before borrowing it.
    let ways_len = player.frames[player.current].ways.len();
    let selected = player.why_selected.min(ways_len.saturating_sub(1));
    player.why_selected = selected;

    // `fit_to_terminal` keeps `term_height − 1` rows (it leaves the last cell empty
    // to avoid a scroll), so that's the height we compose to.
    let drawable = (player.term_height as usize).saturating_sub(1).max(6);
    let context_window_k = player.context_window_k;

    // Footer + header are the shared chrome (built before borrowing the frame).
    let mut nav_buf = String::new();
    render_status_bar(&mut nav_buf, player);
    let nav: Vec<String> = nav_buf.lines().map(str::to_string).collect();

    // Size the ways table to this frame's widest way id so the Way column tracks
    // its content instead of absorbing all the leftover width; the same layout
    // drives the header and every row so they stay aligned.
    let frame = &player.frames[player.current];
    let layout = render::Layout::for_rows(&frame.ways);

    // The Timeline's column header is the shared ways-table header (labels + rule).
    let mut th = String::new();
    render::write_table_header_with(&mut th, &layout);
    let top = header_lines(player, th.lines().map(str::to_string).collect());

    if frame.ways.is_empty() {
        let rows = vec!["  \x1b[2mNo ways triggered yet.\x1b[0m".to_string()];
        return compose_screen(top, rows, 0, Vec::new(), nav, drawable);
    }

    let current_epoch = frame.epoch;
    let current_tokens_k = frame.token_position_k;
    let bar_positions = render::compute_bar_positions(&frame.ways, context_window_k);
    let unique_pos = render::unique_positions(&bar_positions);

    // ── Scrollable, selectable rows ──
    // One selectable unit per way; its rendered block (1 line, or 2 with a
    // check-fires line) is tracked so the viewport can keep the selection visible.
    let mut rows: Vec<String> = Vec::new();
    let mut way_start_line: Vec<usize> = Vec::with_capacity(frame.ways.len());
    for (i, w) in frame.ways.iter().enumerate() {
        way_start_line.push(rows.len());
        // The selection highlight takes visual precedence over new/redisclosed color.
        let (prefix, suffix) = if i == selected {
            ("\x1b[7m", "\x1b[0m")
        } else if w.is_new {
            ("\x1b[1;32m", "\x1b[0m")
        } else if w.is_redisclosed {
            ("\x1b[1;36m", "\x1b[0m")
        } else {
            ("", "")
        };
        let mut block = String::new();
        render::write_way_row_with(
            &mut block, w, current_epoch, current_tokens_k,
            &bar_positions, &unique_pos, i, prefix, suffix, &layout,
        );
        rows.extend(block.lines().map(str::to_string));
    }
    let sel_line = way_start_line.get(selected).copied().unwrap_or(0);

    // ── Extras pinned above the nav (token timeline + new events) ──
    let mut extras: Vec<String> = Vec::new();
    if current_tokens_k > 0 {
        let mut tl = String::new();
        let _ = writeln!(tl);
        render::write_token_timeline(
            &mut tl, &frame.ways, &unique_pos,
            current_tokens_k, context_window_k,
        );
        extras.extend(tl.lines().map(str::to_string));
    }
    if !frame.new_events.is_empty() {
        extras.push(String::new());
        extras.push(format!("  \x1b[1;32m+ {}\x1b[0m", frame.new_events.join(", ")));
    }

    compose_screen(top, rows, sel_line, extras, nav, drawable)
}

/// The unified 2-row footer shared by both views: a full-width rule, then the
/// tab-bar + view-appropriate key hints, distributed evenly across the terminal
/// width (space-between). The `tab`/`esc`/position tail is identical across views so
/// the legend never jumps when toggling.
#[cfg(feature = "tui")]
pub(super) fn render_status_bar(out: &mut String, player: &Player) {
    let width = (player.term_width as usize).max(1);
    let _ = writeln!(out, "\x1b[2m{}\x1b[0m", "─".repeat(width));

    let active = if player.view == View::WhyFired { 1 } else { 0 };

    // Each element is one `[key] label` unit; `justify` spreads them across `width`.
    let mut segs: Vec<String> = vec![compositor::tab_bar(&["Timeline", "Why fired"], active)];
    match player.view {
        View::Timeline => {
            segs.push("\x1b[7m ▲▼ \x1b[0m select".into());
            segs.push("\x1b[7m ⏎ \x1b[0m why".into());
            segs.push("\x1b[7m ◀▶ \x1b[0m frame".into());
            if player.live {
                segs.push(if player.following {
                    "\x1b[7m space \x1b[0m \x1b[1;32m● following\x1b[0m".into()
                } else {
                    "\x1b[7m space \x1b[0m \x1b[1;33m● paused\x1b[0m".into()
                });
            } else {
                segs.push("\x1b[7m space \x1b[0m play".into());
                segs.push(format!("\x1b[7m +- \x1b[0m \x1b[2m{}\x1b[0m", SPEEDS[player.speed_idx].1));
            }
        }
        View::WhyFired => {
            segs.push("\x1b[7m ▲▼ \x1b[0m way".into());
            segs.push("\x1b[7m j/k \x1b[0m read".into());
            segs.push("\x1b[7m ◀▶ \x1b[0m frame".into());
        }
    }
    segs.push("\x1b[7m tab \x1b[0m view".into());
    segs.push("\x1b[7m esc \x1b[0m quit".into());
    segs.push(format!("\x1b[1m{}/{}\x1b[0m", player.current + 1, player.frames.len()));

    out.push_str(&justify(&segs, width));
}

/// Lay `segments` out across `width` with even gaps between them (space-between):
/// the first hugs the left edge, the last the right, the rest spread evenly. Falls
/// back to a single-space join when the content is already wider than `width`.
#[cfg(feature = "tui")]
fn justify(segments: &[String], width: usize) -> String {
    let content: usize = segments.iter().map(|s| compositor::visible_len(s)).sum();
    let gaps = segments.len().saturating_sub(1);
    if gaps == 0 || content + gaps >= width {
        return segments.join(" ");
    }
    let total_space = width - content;
    let base = total_space / gaps;
    let extra = total_space % gaps;
    let mut out = String::new();
    for (i, seg) in segments.iter().enumerate() {
        out.push_str(seg);
        if i < gaps {
            out.push_str(&" ".repeat(base + usize::from(i < extra)));
        }
    }
    out
}

/// The unified 4-row header both views share, so toggling never makes the header
/// jump: `Session <id>  <path>`, a metrics line, then the view's own 2-row column
/// header (labels + rule). `col_header` must be exactly 2 lines.
#[cfg(feature = "tui")]
pub(super) fn header_lines(player: &Player, col_header: Vec<String>) -> Vec<String> {
    let frame = &player.frames[player.current];
    // When following the live tail, show how long ago the newest frame fired — a
    // stale "2h ago" flags that this isn't the session you think it is; "3s ago"
    // updating as you work is the proof it's yours.
    let live = if player.live {
        if player.following {
            let ago = seconds_ago(&frame.timestamp)
                .map(|s| format!(" \x1b[2m· {}\x1b[0m", ago_label(s)))
                .unwrap_or_default();
            format!("  \x1b[1;32m● LIVE\x1b[0m{ago}")
        } else {
            "  \x1b[1;33m● LIVE paused\x1b[0m".to_string()
        }
    } else {
        String::new()
    };
    let mut h = vec![
        // Full session id (there's ample room), with the session's project path.
        format!(
            "\x1b[1mSession\x1b[0m {}  \x1b[2m{}\x1b[0m",
            player.session_id, player.project_name
        ),
        // The current frame's wall-clock time anchors "when" you are as you scrub
        // windows/frames — otherwise every window looks alike.
        format!(
            "  \x1b[2mepoch {} · {}K ctx · {} ways · window {}/{} · {}\x1b[0m{live}",
            frame.epoch,
            player.context_window_k,
            frame.ways.len(),
            frame.window,
            player.windows,
            friendly_ts(&frame.timestamp),
        ),
    ];
    h.extend(col_header);
    h
}

/// `2026-07-03T16:52:00Z` → `2026-07-03 16:52` (minute precision, no `T`/`Z`).
#[cfg(feature = "tui")]
fn friendly_ts(ts: &str) -> String {
    let spaced = ts.replace('T', " ");
    spaced.get(..16).unwrap_or(&spaced).to_string()
}

/// Whole seconds between an RFC-3339 UTC timestamp and now (`None` if unparseable,
/// clamped at 0 for a future timestamp / clock skew).
#[cfg(feature = "tui")]
fn seconds_ago(ts: &str) -> Option<u64> {
    let then = parse_rfc3339_unix(ts)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    Some((now - then).max(0) as u64)
}

/// Parse `YYYY-MM-DDTHH:MM:SSZ` to Unix seconds (UTC), via `days_from_civil`
/// (Howard Hinnant). No dependency; the log's timestamps are always UTC `Z`.
#[cfg(feature = "tui")]
fn parse_rfc3339_unix(ts: &str) -> Option<i64> {
    let num = |a: usize, z: usize| ts.get(a..z)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, s) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    let yc = if mo <= 2 { y - 1 } else { y };
    let era = (if yc >= 0 { yc } else { yc - 399 }) / 400;
    let yoe = yc - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + h * 3600 + mi * 60 + s)
}

/// A compact "N ago" label: seconds, minutes, hours, then days.
#[cfg(feature = "tui")]
fn ago_label(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s ago")
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}

/// Fit rendered output to terminal dimensions.
/// Uses \r\n because raw mode requires explicit carriage return.
#[cfg(feature = "tui")]
pub(super) fn fit_to_terminal(output: &str, width: usize, height: usize) -> String {
    let mut result = String::new();
    let max_lines = height.saturating_sub(1);

    for (line_count, line) in output.lines().enumerate() {
        if line_count >= max_lines {
            break;
        }
        result.push_str(&crate::cmd::compositor::truncate_visible(line, width));
        result.push_str("\r\n");
    }
    result
}

#[cfg(all(test, feature = "tui"))]
mod tests {
    use super::*;

    #[test]
    fn justify_spreads_segments_to_full_width() {
        let segs = vec!["a".to_string(), "bb".to_string(), "c".to_string()];
        let line = justify(&segs, 20);
        assert_eq!(compositor::visible_len(&line), 20, "fills the full width");
        assert!(line.starts_with('a'), "first segment hugs the left");
        assert!(line.ends_with('c'), "last segment hugs the right");
        // Content wider than the width → single-space join, no panic.
        let tight = justify(&segs, 3);
        assert_eq!(tight, "a bb c");
    }

    #[test]
    fn friendly_ts_trims_to_minute() {
        assert_eq!(friendly_ts("2026-07-03T16:52:00Z"), "2026-07-03 16:52");
        assert_eq!(friendly_ts("bogus"), "bogus"); // too short → returned as-is
    }

    #[test]
    fn parse_rfc3339_unix_matches_known_epochs() {
        assert_eq!(parse_rfc3339_unix("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339_unix("2000-01-01T00:00:00Z"), Some(946_684_800));
        assert_eq!(parse_rfc3339_unix("2026-07-03T00:00:00Z"), Some(1_783_036_800));
        assert_eq!(parse_rfc3339_unix("bad"), None);
    }

    #[test]
    fn ago_label_scales_units() {
        assert_eq!(ago_label(5), "5s ago");
        assert_eq!(ago_label(125), "2m ago");
        assert_eq!(ago_label(7_200), "2h ago");
        assert_eq!(ago_label(172_800), "2d ago");
    }

    #[test]
    fn compose_screen_is_fixed_height_with_nav_pinned() {
        let top = vec!["h1".to_string(), "h2".to_string()];
        let nav = vec!["sep".to_string(), "nav".to_string()];

        // Long list, short screen → exactly `drawable` lines, nav on the last line.
        let rows: Vec<String> = (0..50).map(|i| format!("row{i}")).collect();
        let out = compose_screen(top.clone(), rows, 40, vec![], nav.clone(), 20);
        let lines: Vec<&str> = out.split('\n').collect();
        assert_eq!(lines.len(), 20, "composes to exactly the drawable height");
        assert_eq!(lines[0], "h1", "header pinned at the top");
        assert_eq!(lines[19], "nav", "nav pinned to the last line even when overflowing");

        // Short list → the middle is blank-padded so the nav still sits at the bottom.
        let out2 = compose_screen(top, vec!["only".to_string()], 0, vec![], nav, 20);
        let l2: Vec<&str> = out2.split('\n').collect();
        assert_eq!(l2.len(), 20);
        assert_eq!(l2[2], "only");
        assert_eq!(l2[19], "nav", "nav stays anchored to the bottom, not floating up");
        assert_eq!(l2[10], "", "the list area is padded with blanks");

        // Tiny terminal where the pinned header+nav alone exceed the height: the nav
        // must survive (the header is dropped instead of the footer).
        let tiny = compose_screen(
            vec!["h1".into(), "h2".into(), "h3".into()],
            vec!["r".into()],
            0,
            vec![],
            vec!["sep".into(), "nav".into()],
            4,
        );
        let tl: Vec<&str> = tiny.split('\n').collect();
        assert_eq!(tl.len(), 4, "never exceeds the drawable height");
        assert_eq!(tl[3], "nav", "footer survives; the header is clipped instead");
    }
}
