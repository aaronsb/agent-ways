//! The replay's ways table and its context lines on agent-tui: the columns
//! `ways session ways` prints (Way, Epoch, Dist, Trigger, the pin, Re-disclosure)
//! as ratatui rows, and the token gauge, forecast and re-disclosure zones
//! below them. The values and their colours come from `cmd::render`, the
//! same as `ways session ways`; only the drawing differs.

use agent_tui::ratatui::layout::{Alignment, Constraint};
use agent_tui::ratatui::style::{Modifier, Style};
use agent_tui::ratatui::text::{Line, Span};
use agent_tui::ratatui::widgets::{Cell, Row};
use agent_tui::theme;

use super::model::{ActiveWay, Frame};
use crate::cmd::render::{self, PIN_SYMBOLS};

/// The table's column widths, Way taking what is left.
pub(super) const WIDTHS: [Constraint; 6] = [
    Constraint::Min(12),
    Constraint::Length(5),
    Constraint::Length(5),
    Constraint::Length(13),
    Constraint::Length(1),
    Constraint::Length(13),
];

/// An agent-theme style as the frame's palette draws it.
fn ink(s: agent_theme::Style) -> Style {
    theme::current().painter.ratatui(s)
}

pub(super) fn header() -> Row<'static> {
    let right = |t: &'static str| Cell::from(Line::from(t).alignment(Alignment::Right));
    Row::new(vec![Cell::from("Way"), right("Epoch"), right("Dist"), Cell::from("Trigger"), Cell::from("\u{2316}"), Cell::from("Re-disclosure")])
        .style(Style::new().add_modifier(Modifier::BOLD))
}

/// The pin of a way's cluster, in its categorical colour.
fn pin(cluster: usize) -> Span<'static> {
    let symbol = PIN_SYMBOLS[cluster % PIN_SYMBOLS.len()].to_string();
    match render::pin_color(cluster, theme::current().depth()) {
        Some(c) => Span::styled(symbol, Style::new().fg(agent_theme::ratatui::color(c))),
        None => Span::raw(symbol),
    }
}

/// Where each way's next re-disclosure falls on a bar `bar` cells wide,
/// and the distinct positions, which number the clusters.
pub(super) fn clusters(ways: &[ActiveWay], window_k: u64, bar: usize) -> (Vec<Option<usize>>, Vec<usize>) {
    let pos = render::compute_bar_positions_in(ways, window_k, bar);
    let unique = render::unique_positions(&pos);
    (pos, unique)
}

/// One row per active way of `frame`: two lines when its check has fired.
/// A way new in this frame is green and bold, one re-disclosed the accent
/// in bold.
pub(super) fn rows(frame: &Frame, window_k: u64, bar: usize) -> Vec<Row<'static>> {
    let (pos, unique) = clusters(&frame.ways, window_k, bar);
    frame
        .ways
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let distance = frame.epoch.saturating_sub(w.epoch_fired);
            let (next, next_style) = render::next_cell(w, frame.epoch, frame.token_position_k);
            let pin = pos.get(i).copied().flatten().map_or(Span::raw(" "), |p| pin(render::cluster_of(p, &unique)));
            let mut way = vec![Line::raw(w.id.clone())];
            if w.check_fires > 0 {
                let decay = 1.0 / (w.check_fires as f64 + 1.0);
                way.push(Line::styled(format!("  ✓ check ({} fires, decay={decay:.2})", w.check_fires), theme::muted()));
            }
            let height = way.len() as u16;
            let style = if w.is_new {
                theme::ok().add_modifier(Modifier::BOLD)
            } else if w.is_redisclosed {
                theme::accent().add_modifier(Modifier::BOLD)
            } else {
                Style::new()
            };
            Row::new(vec![
                Cell::from(way),
                Cell::from(Line::from(w.epoch_fired.to_string()).alignment(Alignment::Right)),
                Cell::from(Line::styled(distance.to_string(), ink(render::distance_style(distance, frame.epoch))).alignment(Alignment::Right)),
                Cell::from(render::format_trigger(&w.trigger)),
                Cell::from(pin),
                Cell::from(Span::styled(next, ink(next_style))),
            ])
            .height(height)
            .style(style)
        })
        .collect()
}

/// The lines under the table, most useful first: the token gauge, the
/// re-disclosure zones and what changed in this frame, then the forecast.
/// Each is `width` columns at most; the caller keeps as many as fit.
pub(super) fn context(frame: &Frame, window_k: u64, width: usize) -> Vec<(u8, Line<'static>)> {
    let mut out: Vec<(u8, Line<'static>)> = Vec::new();
    let tokens_k = frame.token_position_k;
    if tokens_k > 0 {
        let pct = (tokens_k * 100).checked_div(window_k).unwrap_or(0).min(100);
        let label = format!(" {pct}% ({tokens_k}K / {window_k}K)");
        let bar = width.saturating_sub(label.chars().count()).max(1);
        let filled = (pct as usize * bar / 100).min(bar);
        let style = ink(render::traffic(pct < 50, pct < 75));
        out.push((0, Line::from(vec![Span::styled(format!("{}{}", "█".repeat(filled), "░".repeat(bar - filled)), style), Span::raw(label)])));

        let (_, unique) = clusters(&frame.ways, window_k, bar);
        let (mut now, mut soon, mut later) = (0, 0, 0);
        let mut future: Vec<(u64, usize)> = Vec::new();
        for w in &frame.ways {
            let threshold = w.refire_threshold_k;
            let at = w.token_pos / 1000 + threshold;
            if tokens_k >= at {
                now += 1;
                continue;
            }
            if threshold > 0 && at - tokens_k <= threshold / 4 {
                soon += 1;
            } else {
                later += 1;
            }
            let full = ((at * bar as u64).checked_div(window_k).unwrap_or(0) as usize).min(bar - 1);
            future.push((at, render::cluster_of(full, &unique)));
        }
        let mut zones: Vec<Span<'static>> = Vec::new();
        for (n, text, style) in [
            (now, "re-disclose now", theme::ok()),
            (soon, "approaching", theme::warn().add_modifier(Modifier::BOLD)),
            (later, "distant", theme::muted()),
        ] {
            if n > 0 {
                let mark = match text {
                    "re-disclose now" => "●",
                    "approaching" => "◐",
                    _ => "○",
                };
                if !zones.is_empty() {
                    zones.push(Span::raw("  "));
                }
                zones.push(Span::styled(format!("{mark} {n} {text}"), style));
            }
        }
        if !zones.is_empty() {
            let th: Vec<u64> = frame.ways.iter().map(|w| w.refire_threshold_k).collect();
            let (lo, hi) = (th.iter().min().copied().unwrap_or(0), th.iter().max().copied().unwrap_or(0));
            let interval = if lo == hi { format!("{lo}K interval") } else { format!("{lo}–{hi}K intervals") };
            zones.push(Span::styled(format!("  │ {interval}"), theme::muted()));
            out.push((1, Line::from(zones)));
        }

        if !future.is_empty() {
            let lo = tokens_k;
            let (min, max) = (future.iter().map(|f| f.0).min().unwrap_or(lo), future.iter().map(|f| f.0).max().unwrap_or(lo));
            let end = (max + (max - min) / 4).min(window_k).max(lo + 1);
            let label = format!(" forecast {lo}K → {end}K");
            let strip = width.saturating_sub(label.chars().count()).max(1);
            let mut cells: Vec<Option<usize>> = vec![None; strip];
            for (at, c) in &future {
                let x = ((at.saturating_sub(lo) * strip as u64) / (end - lo)) as usize;
                let x = x.min(strip - 1);
                if cells[x].is_none() {
                    cells[x] = Some(*c);
                }
            }
            let mut spans: Vec<Span<'static>> = cells.into_iter().map(|c| c.map_or(Span::styled("·", theme::muted()), pin)).collect();
            spans.push(Span::styled(label, theme::muted()));
            out.push((3, Line::from(spans)));
        }
    }
    if !frame.new_events.is_empty() {
        out.push((2, Line::styled(format!("+ {}", frame.new_events.join(", ")), theme::ok().add_modifier(Modifier::BOLD))));
    }
    out
}
