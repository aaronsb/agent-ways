//! The replay's ways table and its context lines on agent-tui: the columns
//! `ways session ways` prints (Way, Agent, Epoch, Dist, Trigger, the pin,
//! Re-disclose) as ratatui rows, and the token gauge, forecast and re-disclosure zones
//! below them. The values and their colours come from `cmd::render`, the
//! same as `ways session ways`; only the drawing differs.

use agent_tui::ratatui::layout::{Alignment, Constraint};
use agent_tui::ratatui::style::{Modifier, Style};
use agent_tui::ratatui::text::{Line, Span};
use agent_tui::ratatui::widgets::{Cell, Row};
use agent_tui::theme;

use super::agents::Agents;
use super::model::{ActiveWay, Fate, Frame};
use crate::cmd::render::{self, PIN_SYMBOLS};

/// The Agent column's width in a table `inner` cells wide: the widest name
/// of the session's agents, so it holds still across frames, from the
/// header's five cells to sixteen, while Way keeps eighteen. Narrower,
/// Agent gives up cells to eight, then Way, to its twelve. At 80 columns
/// Agent has ten and Way eighteen.
pub(super) fn agent_width(agents: &Agents, inner: usize) -> u16 {
    let widest = agents.widest();
    let room = inner.saturating_sub(HIGHLIGHT + WAY_KEEP + FIXED + GAPS).max(AGENT_MIN);
    widest.clamp(5, AGENT_MAX).min(room) as u16
}

const WAY_MIN: usize = 12;
const WAY_KEEP: usize = 18;
const AGENT_MIN: usize = 8;
const AGENT_MAX: usize = 16;
/// Epoch, Dist, Trigger, the pin and Re-disclose.
const FIXED: usize = 5 + 5 + 13 + 1 + 13;
/// Two cells between each of the seven columns.
const GAPS: usize = 6 * 2;
/// The selection mark's column.
const HIGHLIGHT: usize = 1;

/// The table's column widths, Way taking what is left.
pub(super) fn widths(agent: u16) -> [Constraint; 7] {
    [
        Constraint::Min(WAY_MIN as u16),
        Constraint::Length(agent),
        Constraint::Length(5),
        Constraint::Length(5),
        Constraint::Length(13),
        Constraint::Length(1),
        Constraint::Length(13),
    ]
}

/// Each fate's mark before a way's name and the name's colour: the one
/// table both come from, so they agree. Without colour the mark carries
/// it; a fired way has none, and a check's fire has its `✓ check` line.
pub(super) fn look(fate: Fate) -> (&'static str, Style) {
    match fate {
        Fate::Injected => ("", theme::ok()),
        Fate::Redisclosed => ("↩ ", theme::accent()),
        Fate::CheckFired => ("", theme::info()),
        Fate::WouldBlock => ("◌ ", theme::warn()),
        Fate::Blocked => ("⊘ ", theme::err()),
        Fate::RefireHeld => ("◷ ", theme::muted()),
        Fate::CapHeld => ("⊟ ", ink(agent_theme::Style::new().role(agent_theme::Role::Alt))),
    }
}

/// A way's name with its fate's mark, in its fate's colour, cut to `width`
/// cells from the left so the leaf stays.
pub(super) fn way_name(w: &ActiveWay, width: usize) -> Span<'static> {
    let (mark, style) = look(Fate::of(w));
    let room = width.saturating_sub(agent_fmt::visible_len(mark));
    Span::styled(format!("{mark}{}", keep_tail(&w.id, room)), style)
}

/// `id` in at most `width` cells: whole trailing segments after `…/`
/// (`…/docs/adr`), or the leaf's tail after `…` when even it does not fit.
pub(super) fn keep_tail(id: &str, width: usize) -> String {
    let len = |s: &str| s.chars().count();
    if len(id) <= width {
        return id.to_string();
    }
    let segs: Vec<&str> = id.split('/').collect();
    let mut tail = String::new();
    for seg in segs.iter().rev() {
        let next = if tail.is_empty() { seg.to_string() } else { format!("{seg}/{tail}") };
        if len(&next) + 2 > width {
            break;
        }
        tail = next;
    }
    if !tail.is_empty() {
        return format!("…/{tail}");
    }
    let keep = width.saturating_sub(1);
    let chars: Vec<char> = id.chars().collect();
    format!("…{}", chars[chars.len().saturating_sub(keep)..].iter().collect::<String>())
}

/// The Way column's width beside an Agent column `agent_w` wide.
pub(super) fn way_width(inner: usize, agent_w: u16) -> usize {
    inner.saturating_sub(HIGHLIGHT + agent_w as usize + FIXED + GAPS).max(WAY_MIN)
}

/// An agent's name in its colour, cut to `width` display cells: a
/// workflow member keeps the end of its label (`wf·…:judge`), any other the
/// start (`code-revi…`), and a disambiguating short id stays whole
/// (`gene…·a3897`).
pub(super) fn agent_cell(agents: &Agents, id: &str, width: u16) -> Line<'static> {
    let w = width as usize;
    let (base, suffix) = agents.parts(id);
    let suffix = suffix.map(|s| format!("·{s}")).unwrap_or_default();
    let room = w.saturating_sub(agent_fmt::visible_len(&suffix)).max(2);
    let base = match base.strip_prefix("wf·") {
        Some(l) if agent_fmt::visible_len(&base) > room => format!("wf·…{}", tail_cells(l, room.saturating_sub(4))),
        _ => agent_fmt::truncate_visible(&base, room),
    };
    Line::styled(agent_fmt::truncate_visible(&format!("{base}{suffix}"), w), agents.style(id))
}

/// The end of `s` in at most `cells` display cells.
fn tail_cells(s: &str, cells: usize) -> String {
    let mut out: Vec<char> = Vec::new();
    let mut used = 0;
    for c in s.chars().rev() {
        let cw = agent_fmt::visible_len(c.encode_utf8(&mut [0; 4]));
        if used + cw > cells {
            break;
        }
        used += cw;
        out.push(c);
    }
    out.iter().rev().collect()
}


/// An agent-theme style as the frame's palette draws it.
fn ink(s: agent_theme::Style) -> Style {
    theme::current().painter.ratatui(s)
}

pub(super) fn header() -> Row<'static> {
    let right = |t: &'static str| Cell::from(Line::from(t).alignment(Alignment::Right));
    Row::new(vec![Cell::from("Way"), Cell::from("Agent"), right("Epoch"), right("Dist"), Cell::from("Trigger"), Cell::from(render::PIN_HEADER), Cell::from("Re-disclose")])
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

/// One row per active way of `frame` and agent that fired it: two lines
/// when its check has fired, the second naming the agent the check counted
/// against. The name takes its fate's mark and colour ([`look`]); a way
/// fired or re-disclosed in this frame is bold.
pub(super) fn rows(frame: &Frame, agents: &Agents, agent_w: u16, window_k: u64, bar: usize) -> Vec<Row<'static>> {
    let (pos, unique) = clusters(&frame.ways, window_k, bar);
    let way_w = way_width(bar, agent_w);
    frame
        .ways
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let distance = frame.epoch.saturating_sub(w.epoch_fired);
            let (next, next_style) = render::next_cell(w, frame.epoch, frame.token_position_k);
            let pin = pos.get(i).copied().flatten().map_or(Span::raw(" "), |p| pin(render::cluster_of(p, &unique)));
            let mut way = vec![Line::from(way_name(w, way_w))];
            let mut agent = vec![agent_cell(agents, &w.agent, agent_w)];
            if w.check_fires > 0 {
                way.push(Line::styled(format!("  ✓ ×{} decay {:.2}", w.check_fires, decay(w.check_fires)), theme::muted()));
                agent.push(agent_cell(agents, &w.agent, agent_w));
            }
            let height = way.len() as u16;
            let style = if w.is_new || w.is_redisclosed { Style::new().add_modifier(Modifier::BOLD) } else { Style::new() };
            Row::new(vec![
                Cell::from(way),
                Cell::from(agent),
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

/// A check's decay after `fires` fires.
pub(super) fn decay(fires: u64) -> f64 {
    1.0 / (fires as f64 + 1.0)
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
        // The notes keep `↻` in JSON; the screen draws the re-disclosure
        // mark the rows use, which the terminal fonts have.
        let notes: Vec<String> = frame.new_events.iter().map(|e| e.strip_prefix("↻ ").map_or_else(|| e.clone(), |w| format!("↩ {w}"))).collect();
        out.push((2, Line::styled(format!("+ {}", notes.join(", ")), theme::ok().add_modifier(Modifier::BOLD))));
    }
    // #786: the subagent switch held ways back here, one mark each.
    if !frame.suppressed.is_empty() {
        let each: Vec<String> = frame.suppressed.iter().map(|s| s.label()).collect();
        out.push((2, Line::styled(format!("⊝ suppressed: {}", each.join(", ")), theme::warn())));
    }
    out
}
