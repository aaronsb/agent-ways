//! Drawing the chat: the pane between the shell's bars, and the channel
//! tabs the shell draws on its tab bar, in the palette the chat holds.
//! Every colour is either a theme role, through `agent_tui::theme`, or an
//! identity colour, through [`color_for`]; nothing here picks a colour of
//! its own.

use std::collections::HashMap;
use std::time::{Duration, UNIX_EPOCH};

use agent_tui::feed::{Entry, Feed};
use agent_tui::ratatui::layout::{Constraint, Layout, Rect};
use agent_tui::ratatui::style::{Modifier, Style};
use agent_tui::ratatui::text::{Line, Span};
use agent_tui::ratatui::widgets::{Block, BorderType, Padding, Paragraph};
use agent_tui::ratatui::Frame;
use agent_tui::strip::{prefix_match, target};
use agent_tui::PaneTab;
use agent_tui::theme::{self, Ground};
use agent_tui::wrap::str_width;

use super::{Chip, ChatPane, Clock, Hits};
use crate::attach;
use crate::chip::{chip_for, color_for, CHIP_WIDTH};
use crate::groups::KnownGroup;
use crate::helper::{self, HelperMode};
use crate::legend::{find_trailing_mention, Sigil};
use crate::signal::Signal;
use crate::slash;
use crate::tabs::{self, Tab};

/// The most rows the compose box grows to before it scrolls.
const INPUT_ROWS: usize = 10;

/// The pane in `area`, between the shell's tab bar and its bottom bar.
/// The shell has set the palette and fills the theme's ground after.
pub(super) fn draw(chat: &mut ChatPane, f: &mut Frame, area: Rect) {
    let fg = chat.normal_tab();
    let text = chat.input.text().to_string();
    let attachments = attach::existing_attachments(&text);
    // The compose box: its rows at the box's inner width less the prompt,
    // the attachment row when a dropped file is in the buffer, and the
    // destination flag's row (#392).
    let inner = area.width.saturating_sub(6);
    let rows = chat.input.rows(inner, theme::body());
    let shown = rows.len().clamp(1, INPUT_ROWS);
    let input_h = shown as u16 + 3 + u16::from(!attachments.is_empty());
    let [feed, compose, helper] = Layout::vertical([Constraint::Min(3), Constraint::Length(input_h), Constraint::Length(1)]).areas(area);

    let groups = chat.world().groups.clone();
    chat.hits = Hits { text_width: inner, ..Hits::default() };
    draw_feed(chat, f, feed, &fg);
    draw_compose(chat, f, compose, rows, &attachments, &fg);
    draw_helper(chat, f, helper, &text, &groups);
}

/// The tab bar the shell draws: the common menu's `≡` slot, merged, then
/// `#open` and the channels in use (today every channel: the strip's one
/// seam is `crate::tabs::strip_names`), then the `+` slot. Merged is tab
/// 1, so Ctrl+N and Alt+N keep their numbers; the slots carry none. The
/// channel tabs (#393) are numbered for Alt+N and
/// led by the channel's glyph in its identity colour, so the bar doubles as
/// the channel legend. A `#partial` being typed marks the tabs it completes
/// to.
pub(super) fn tabs(chat: &mut ChatPane) -> Vec<PaneTab> {
    let depth = chat.palette.depth();
    let text = chat.input.text().to_string();
    let partial = find_trailing_mention(&text).filter(|m| m.sigil == Sigil::Group).map(|m| m.partial.to_string());
    let mut out = vec![PaneTab::new("≡").action(), PaneTab::new("merged")];
    for k in &chat.world().groups {
        let mut glyph = Style::new().fg(color_for(k.group.palette, depth));
        if k.is_base || k.group.style.bold {
            glyph = glyph.add_modifier(Modifier::BOLD);
        }
        out.push(
            PaneTab::new(format!("#{}", k.group.name))
                .lead(vec![Span::styled(format!("{} ", k.group.glyph), glyph)])
                .target(prefix_match(&k.group.name, partial.as_deref())),
        );
    }
    out.push(PaneTab::new("+").action());
    out
}

/// What the shown channel is for, set back after the tabs (#404).
pub(super) fn trailer(chat: &mut ChatPane) -> Vec<Span<'static>> {
    let Tab::Channel(g) = chat.normal_tab() else { return Vec::new() };
    let groups = &chat.world().groups;
    match groups.iter().find(|k| k.group.name == g).and_then(|k| k.membership.description.as_deref()) {
        Some(desc) => vec![Span::styled(format!("  — {desc}"), theme::muted())],
        None => Vec::new(),
    }
}

/// The time a cell shows: one format across every attend surface
/// (`agent_fmt::compact_time`), nothing when the file's time was
/// unreadable (`ts == 0`).
fn when(clock: Clock, ts: u64) -> String {
    if ts == 0 {
        return String::new();
    }
    let t = UNIX_EPOCH + Duration::from_secs(ts);
    match clock {
        Clock::Live => agent_fmt::compact_time(t, std::time::SystemTime::now()),
        Clock::Pinned { now, offset } => agent_fmt::compact_time_with_offset(t, now, offset),
    }
}

/// One message as a feed entry: the sender's chip (name, scope, then the
/// channel glyphs of the sender's memberships and the time) beside the
/// body, with a chip per attached file that exists now (#390).
fn entry(chat: &ChatPane, s: &Signal, memberships: &HashMap<&str, Vec<&KnownGroup>>) -> Entry {
    let depth = chat.palette.depth();
    let world = chat.world.as_ref().expect("the world is read before the feed");
    let chip = chip_for(&s.from, &s.project, &s.cwd, depth, &world.instances);
    let mut name = Style::new().fg(color_for(chip.palette, depth));
    if chip.style.bold {
        name = name.add_modifier(Modifier::BOLD);
    }
    if chip.style.italic {
        name = name.add_modifier(Modifier::ITALIC);
    }
    // Claudes are members by session id; humans by their sanitized
    // username (ADR-170), the id `/join` writes.
    let key: Option<String> = chip.session_id.clone().or_else(|| {
        s.from
            .strip_prefix("external:")
            .map(|rest| agent_identity::sanitize_id_component(rest.split('@').next().unwrap_or(rest)))
    });
    let mut third: Vec<Span<'static>> = Vec::new();
    for g in key.as_deref().and_then(|k| memberships.get(k)).into_iter().flatten() {
        third.push(Span::styled(format!("{} ", g.group.glyph), Style::new().fg(color_for(g.group.palette, depth))));
    }
    third.push(Span::styled(when(chat.clock, s.ts), theme::muted()));
    let label = vec![Line::styled(chip.primary, name), Line::styled(chip.secondary, theme::muted()), Line::from(third)];
    let mut body: Vec<Line<'static>> = s.message.split('\n').map(|l| Line::styled(l.to_string(), theme::body())).collect();
    let files = attach::existing_attachments(&s.message);
    if !files.is_empty() {
        body.push(attachment_line(chat, &files));
    }
    Entry { label, body }
}

/// A chip per attached file: its name on the rule's ground, so a file
/// reads as an object rather than text.
fn attachment_line(chat: &ChatPane, files: &[String]) -> Line<'static> {
    // No-break spaces inside a chip: wrapping moves a chip whole and keeps
    // its padding at the end of a row.
    let glyph = if agent_identity::is_rich(chat.palette.depth()) { "⎘\u{a0}" } else { "file:\u{a0}" };
    let mut spans = Vec::new();
    for p in files {
        spans.push(Span::styled(format!("\u{a0}{glyph}{}\u{a0}", attach::attachment_name(p).replace(' ', "\u{a0}")), theme::badge(Ground::Rule)));
        spans.push(Span::raw(" "));
    }
    Line::from(spans)
}

/// The feed of the foreground tab (#393): merged shows the whole stream,
/// a channel tab its own traffic and local notices.
fn draw_feed(chat: &mut ChatPane, f: &mut Frame, area: Rect, fg: &Tab) {
    if chat.entries.is_none() {
        chat.world();
        let groups = &chat.world.as_ref().expect("just read").groups;
        let mut memberships: HashMap<&str, Vec<&KnownGroup>> = HashMap::new();
        for kg in groups {
            for m in &kg.membership.members {
                memberships.entry(m.as_str()).or_default().push(kg);
            }
        }
        let entries: Vec<Entry> =
            chat.signals.iter().filter(|s| tabs::visible_in(&s.channel, fg)).map(|s| entry(chat, s, &memberships)).collect();
        chat.entries = Some(entries);
    }
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(theme::rule()).padding(Padding::horizontal(1));
    let inner = block.inner(area);
    f.render_widget(block, area);
    chat.feed_rows = inner.height;
    chat.hits.feed = inner;
    let entries = chat.entries.as_deref().unwrap_or_default();
    let feed = Feed::new(entries).label_width(CHIP_WIDTH).border(theme::rule()).selected(theme::accent()).generation(chat.generation);
    f.render_stateful_widget(feed, inner, &mut chat.feed);
}

/// The compose box: `> ` then the buffer with its cursor, a row of
/// attachment chips when the buffer names files, and on the last row the
/// destination flag (#392), where Enter would send it, hidden while a
/// slash command is composed.
fn draw_compose(chat: &mut ChatPane, f: &mut Frame, area: Rect, rows: Vec<Line<'static>>, files: &[String], fg: &Tab) {
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(theme::accent()).padding(Padding::horizontal(1));
    let inner = block.inner(area);
    f.render_widget(block, area);
    // Keep the cursor's row in view once the buffer outgrows the box.
    // The rows the box really has, after layout, less the flag's row and
    // an attachment row: a short terminal gives fewer than INPUT_ROWS.
    let flag_h = 1;
    let text_h = inner.height.saturating_sub(flag_h);
    let room = (text_h as usize).saturating_sub(usize::from(!files.is_empty())).clamp(1, INPUT_ROWS);
    let cursor_row = rows.iter().position(|l| l.spans.iter().any(|s| s.style.add_modifier.contains(Modifier::REVERSED))).unwrap_or(0);
    let start = cursor_row.saturating_sub(room - 1);
    // The text's rows after the prompt, and the first of them shown.
    chat.hits.compose = (Rect { x: inner.x + 2, y: inner.y, width: inner.width.saturating_sub(2), height: room.min(rows.len()) as u16 }, start);
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (i, row) in rows.into_iter().enumerate().skip(start).take(room) {
        let prompt = if i == 0 { "> " } else { "  " };
        let mut spans = vec![Span::styled(prompt, theme::accent())];
        spans.extend(row.spans);
        lines.push(Line::from(spans));
    }
    if !files.is_empty() {
        lines.push(attachment_line(chat, files));
    }
    f.render_widget(Paragraph::new(lines), Rect { height: text_h, ..inner });
    let scope = tabs::send_scope(fg);
    if let Some(label) = super::destination_label(chat.input.text(), &scope) {
        let flag = format!(" {label} ");
        let w = (str_width(&flag) as u16).min(inner.width);
        let at = Rect { x: inner.x + inner.width - w, y: inner.y + text_h, width: w, height: flag_h };
        f.render_widget(Paragraph::new(Span::styled(flag, theme::badge(Ground::Info))), at);
    }
}

/// The helper row: what the buffer is reaching for (`crate::helper`) —
/// the agent legend, the channel legend, the slash commands, a level of
/// subcommands, or a free token's hint. Names Tab would complete to are
/// marked.
fn draw_helper(chat: &mut ChatPane, f: &mut Frame, area: Rect, text: &str, groups: &[KnownGroup]) {
    let depth = chat.palette.depth();
    let mut chips: Vec<Span<'static>> = Vec::new();
    // What a click on each chip completes to, in the chips' order.
    let mut picks: Vec<Chip> = Vec::new();
    match helper::derive(text) {
        HelperMode::Agents(p) => {
            for k in &chat.world().known {
                picks.push(Chip::Agent(k.nickname.clone()));
                let mut st = Style::new().fg(color_for(k.palette, depth));
                if k.style.bold {
                    st = st.add_modifier(Modifier::BOLD);
                }
                if k.style.italic {
                    st = st.add_modifier(Modifier::ITALIC);
                }
                chips.push(Span::styled(format!("@{}", k.nickname), target(st, prefix_match(&k.nickname, p.as_deref()))));
            }
        }
        HelperMode::Groups(p) => {
            for k in groups {
                picks.push(Chip::Group(k.group.name.clone()));
                let mut st = Style::new().fg(color_for(k.group.palette, depth));
                // The base channel is always bold: the commons (ADR-124 §4).
                if k.is_base || k.group.style.bold {
                    st = st.add_modifier(Modifier::BOLD);
                }
                if k.group.style.italic {
                    st = st.add_modifier(Modifier::ITALIC);
                }
                chips.push(Span::styled(format!("{} #{}", k.group.glyph, k.group.name), target(st, prefix_match(&k.group.name, p.as_deref()))));
            }
        }
        HelperMode::Slash(p) => {
            let legend = slash::legend(p.as_deref());
            picks = legend.iter().map(|c| Chip::Slash(c.label.trim_start_matches('/').to_string())).collect();
            chips = command_chips(legend);
        }
        HelperMode::SubCommands { choices, partial } => {
            let legend = slash::sub_legend(choices, partial.as_deref());
            picks = legend.iter().map(|c| Chip::Sub(c.label.clone())).collect();
            chips = command_chips(legend);
        }
        HelperMode::FreeText(hint) => chips.push(Span::styled(hint.to_string(), theme::hint())),
    }
    let mut spans = vec![Span::raw(" ")];
    let mut x = area.x + 1;
    for (i, c) in chips.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
            x += 1;
        }
        let w = c.width() as u16;
        if let Some(pick) = picks.get(i) {
            let r = Rect { x, y: area.y, width: w, height: 1 }.intersection(area);
            if !r.is_empty() {
                chat.hits.chips.push((r, pick.clone()));
            }
        }
        x = x.saturating_add(w);
        spans.push(c);
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Command chips: a ready command in the accent, a planned one set back.
fn command_chips(items: Vec<slash::LegendChip>) -> Vec<Span<'static>> {
    items
        .into_iter()
        .map(|c| Span::styled(c.label, target(if c.ready { theme::accent() } else { theme::muted() }, c.target)))
        .collect()
}
