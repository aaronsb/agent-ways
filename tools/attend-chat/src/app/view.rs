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
use agent_tui::theme::{self, Ground, Seg};
use agent_tui::wrap::str_width;

use super::{Chip, ChatPane, Clock, Hits};
use crate::attach;
use crate::chip::{chip_for, color_for};
use crate::groups::KnownGroup;
use crate::helper::{self, HelperMode};
use crate::legend::{find_trailing_mention, Sigil};
use crate::signal::Signal;
use crate::slash;
use crate::tabs::{self, Slot, Tab};

/// The most rows the compose box grows to before it scrolls.
const INPUT_ROWS: usize = 10;

/// What leads the compose box's first row.
const LEAD: &str = "> ";

/// A box's border: the accent while it has the keyboard, else the rule's
/// subdued colour, as every other border is drawn (ADR-405).
fn border(focused: bool) -> Style {
    if focused {
        theme::accent()
    } else {
        theme::rule()
    }
}

/// The pane in `area`, between the shell's tab bar and its bottom bar.
/// The shell has set the palette and fills the theme's ground after.
pub(super) fn draw(chat: &mut ChatPane, f: &mut Frame, area: Rect) {
    let fg = chat.normal_tab();
    let text = chat.input.text().to_string();
    // The left column (ADR-405), when shown and there is room for it.
    let side_w = chat.side_width(area.width);
    let (side, area) = match side_w {
        Some(w) => {
            let [side, rest] = Layout::horizontal([Constraint::Length(w), Constraint::Min(1)]).areas(area);
            (Some(side), rest)
        }
        None => {
            // Hidden for width: the keyboard cannot stay on what is not drawn.
            if chat.side_focus().is_some() {
                chat.dispatch(super::ix::Ev::ColumnHidden);
            }
            (None, area)
        }
    };
    // While the column lists the agents and channels, the helper row
    // shows only what it does not: commands, their arguments and hints.
    // Its row stays while a command is typed, so the compose box does not
    // jump as the command reaches an `@agent` or `#channel` argument.
    // A view with nothing to send, an agent's own DMs, has no compose box
    // and no helper row: the feed takes the pane.
    let composes = fg.composes();
    let helper = composes && (side.is_none() || super::sidebar::helper_needed(&text));
    let helper_h = u16::from(helper || (composes && text.trim_start().starts_with('/')));
    let attachments = attach::existing_attachments(&text);
    // The compose box: its rows at the box's inner width less the prompt,
    // the attachment row when a dropped file is in the buffer, and the
    // destination flag's row (#392).
    let lead = LEAD;
    let lead_w = str_width(lead) as u16;
    let inner = area.width.saturating_sub(4 + lead_w);
    let rows = chat.input.rows(inner, theme::body());
    let shown = rows.len().clamp(1, INPUT_ROWS);
    let input_h = if composes { shown as u16 + 3 + u16::from(!attachments.is_empty()) } else { 0 };
    let [feed, compose, helper_row] = Layout::vertical([Constraint::Min(3), Constraint::Length(input_h), Constraint::Length(helper_h)]).areas(area);

    chat.hits = Hits { text_width: inner, lead: lead_w, side_area: side.unwrap_or_default(), ..Hits::default() };
    if let Some(side) = side {
        super::sidebar::draw(chat, f, side);
    }
    draw_feed(chat, f, feed, &fg);
    if composes {
        draw_compose(chat, f, compose, lead, rows, &attachments, &fg);
    }
    if helper {
        draw_helper(chat, f, helper_row, &text);
    }
}

/// The operator view's label on the tab bar: everything the chat hears,
/// every channel together. A view of the operator's, not a channel: no `#`,
/// no number, and no channel's menu (ADR-405).
pub(super) const VIEW_LABEL: &str = "⋯all";

/// The tab bar the shell draws, in the order [`tabs::bar`] gives: the common
/// menu's `≡` slot, the created channel shown with what it is for or the
/// DM view shown (`@Name DMs`, or `Name DMs` for an agent's own, led by
/// the mark its row carries, shaded as a view), the `+` slot, then the
/// reserved tabs pinned right, `#open` and the operator view
/// ([`VIEW_LABEL`]), shaded as a view. The left column numbers the
/// channels for Ctrl+digit, and 0 is the operator view ([`ChatPane::jump`]);
/// while the column is hidden, by its key or for want of width, each
/// channel's tab carries the column's number in its place. A channel tab
/// (#393) is led by the channel's glyph in its identity colour, and the
/// created channel shown carries its member count. A `#partial` being typed
/// marks the tabs it completes to.
pub(super) fn tabs(chat: &mut ChatPane) -> Vec<PaneTab> {
    let depth = chat.palette.depth();
    let text = chat.input.text().to_string();
    let partial = find_trailing_mention(&text).filter(|m| m.sigil == Sigil::Group).map(|m| m.partial.to_string());
    let groups = chat.world().groups.clone();
    let numbers = !chat.column_this_frame();
    let order: Vec<String> = chat.world().rows.iter().map(|r| r.known.group.name.clone()).collect();
    let person = chat.person_glyph();
    chat.bar()
        .into_iter()
        .map(|slot| match slot {
            Slot::Common => PaneTab::new("≡").action(),
            Slot::New => PaneTab::new("+").action(),
            Slot::Tab(Tab::Merged) => PaneTab::new(VIEW_LABEL).reserved().view(),
            Slot::Tab(Tab::Dm(member)) => PaneTab::new(format!("@{} DMs", chat.dm_name(&member))).view(),
            // The agent's own DMs, led by the mark its row carries.
            Slot::Tab(Tab::Review(member)) => {
                let mark = Span::styled(super::sidebar::REVIEW_MARK, Style::new().fg(theme::badge(Ground::Info).bg.unwrap_or_default()));
                PaneTab::new(format!("{} DMs", chat.dm_name(&member))).lead(vec![mark]).view()
            }
            Slot::Tab(Tab::Channel(name)) => {
                let k = groups.iter().find(|k| k.group.name == name);
                let mut glyph = Style::new();
                if let Some(k) = k {
                    glyph = glyph.fg(color_for(k.group.palette, depth));
                    if k.is_base || k.group.style.bold {
                        glyph = glyph.add_modifier(Modifier::BOLD);
                    }
                }
                let lead = k.map(|k| vec![Span::styled(format!("{} ", k.group.glyph), glyph)]).unwrap_or_default();
                // The created channel shown says what it is for (#404); the
                // bar cuts the line to fit.
                let created = k.filter(|k| !k.is_base);
                let desc = created.and_then(|k| k.membership.description.clone()).filter(|d| !d.is_empty());
                let mut label = format!("#{name}");
                if let Some(k) = created {
                    label = format!("{label} {}{person}", k.membership.members.len());
                }
                if let Some(d) = desc {
                    label = format!("{label} — {d}");
                }
                if let Some(n) = order.iter().position(|o| *o == name).map(|i| i + 1).filter(|n| numbers && *n <= 9) {
                    label = format!("{n} {label}");
                }
                let tab = PaneTab::new(label).lead(lead).target(prefix_match(&name, partial.as_deref()));
                if tabs::is_reserved(&Tab::Channel(name)) {
                    tab.reserved()
                } else {
                    tab
                }
            }
        })
        .collect()
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

/// One message as a feed entry: a header band in the sender's colour, as
/// the left column draws its row (ADR-410): the channel glyphs of the
/// sender's memberships and the time on the darkest step, the name on the
/// identity colour, the project on the step between. Then the body, with a
/// chip per attached file that exists now (#390). With the entry, the
/// sender's membership id, for a right click on its header
/// ([`ChatPane::context_at`]).
fn entry(chat: &ChatPane, s: &Signal, memberships: &HashMap<&str, Vec<&KnownGroup>>) -> (Entry, Option<String>) {
    let depth = chat.palette.depth();
    let world = chat.world.as_ref().expect("the world is read before the feed");
    let chip = chip_for(&s.from, &s.project, &s.cwd, depth, &world.instances);
    // Claudes are members by session id; humans by their sanitized
    // username (ADR-170), the id `/join` writes.
    let key = s.sender_member();
    // Live as the column has it: a sender the column sets back is set back here.
    let live = key.as_deref().and_then(|k| world.seen.iter().find(|x| x.member == k)).is_none_or(|x| x.live);
    let [name, project, mark] = super::sidebar::identity_styles(depth, chip.palette, live, chip.style.bold);
    let name = if chip.style.italic { name.add_modifier(Modifier::ITALIC) } else { name };
    let glyphs: String = key.as_deref().and_then(|k| memberships.get(k)).into_iter().flatten().map(|g| format!("{} ", g.group.glyph)).collect();
    let mut header = vec![
        Seg::styled(format!(" {glyphs}{} ", when(chat.clock, s.ts)), mark),
        Seg::styled(format!(" {} ", chip.primary), name),
        Seg::styled(format!(" {} ", chip.secondary), project),
    ];
    // A reply or a direct message to the person at the chat (ADR-401, `to`),
    // or any other direct message, with whom it went to.
    match &s.channel {
        crate::signal::Channel::Person => header.push(Seg::styled(" → you ".to_string(), mark)),
        crate::signal::Channel::Sent(to) | crate::signal::Channel::Direct(to) | crate::signal::Channel::Elsewhere(to) => {
            let names: Vec<String> = to.split(',').filter(|m| !m.is_empty()).map(|m| format!("@{}", chat.dm_name(m))).collect();
            if !names.is_empty() {
                header.push(Seg::styled(format!(" → {} ", names.join(" ")), mark));
            }
        }
        _ => {}
    }
    let mut body: Vec<Line<'static>> = s.message.split('\n').map(|l| Line::styled(l.to_string(), theme::body())).collect();
    let files = attach::existing_attachments(&s.message);
    if !files.is_empty() {
        body.push(attachment_line(chat, &files));
    }
    (Entry { header, body }, key)
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
        let (entries, members): (Vec<Entry>, Vec<Option<String>>) =
            chat.signals.iter().filter(|s| tabs::shows(s, fg)).map(|s| entry(chat, s, &memberships)).unzip();
        chat.entries = Some(entries);
        chat.entry_members = members;
    }
    let focused = *chat.ix.focus() == super::focus::Focus::Feed && !chat.ix.overlaid();
    let mut block = Block::bordered().border_type(BorderType::Rounded).border_style(border(focused)).padding(Padding::horizontal(1));
    // A view with nothing to send says so where the compose box would be.
    if !fg.composes() {
        block = block.title_bottom(Line::styled(" read only ", theme::muted()).right_aligned());
    }
    let inner = block.inner(area);
    f.render_widget(block, area);
    chat.feed_rows = inner.height;
    chat.hits.feed = inner;
    chat.hits.feed_box = area;
    let entries = chat.entries.as_deref().unwrap_or_default();
    let feed = Feed::new(entries).shape(chat.shape).rule(theme::rule()).selected(theme::accent()).generation(chat.generation);
    f.render_stateful_widget(feed, inner, &mut chat.feed);
}

/// The compose box: `lead` (`> `) then the buffer, a row of attachment
/// chips when the buffer names files, and on the last row the destination
/// flag (#392), where Enter would send it, hidden while a slash command is
/// composed. The box is for chatting alone: nothing else borrows it
/// (ADR-409).
///
/// While the box has the keyboard its border is the accent and its caret is
/// drawn; with the focus elsewhere the border is the rule's subdued colour
/// and no caret shows, so the focus is drawn in one place (ADR-405).
fn draw_compose(chat: &mut ChatPane, f: &mut Frame, area: Rect, lead: &str, rows: Vec<Line<'static>>, files: &[String], fg: &Tab) {
    let focused = chat.ix.owns_text() && !chat.ix.overlaid();
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(border(focused)).padding(Padding::horizontal(1));
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
    let lead_w = str_width(lead) as u16;
    chat.hits.compose = (Rect { x: inner.x + lead_w, y: inner.y, width: inner.width.saturating_sub(lead_w), height: room.min(rows.len()) as u16 }, start);
    let indent = " ".repeat(usize::from(lead_w));
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (i, row) in rows.into_iter().enumerate().skip(start).take(room) {
        let prompt = if i == 0 { lead.to_string() } else { indent.clone() };
        let mut spans = vec![Span::styled(prompt, if focused { theme::accent() } else { theme::muted() })];
        // The caret only where the keyboard is.
        spans.extend(row.spans.into_iter().map(|s| if focused { s } else { Span::styled(s.content, s.style.remove_modifier(Modifier::REVERSED)) }));
        lines.push(Line::from(spans));
    }
    if !files.is_empty() {
        lines.push(attachment_line(chat, files));
    }
    f.render_widget(Paragraph::new(lines), Rect { height: text_h, ..inner });
    let draft = chat.input.text().to_owned();
    if let Some(label) = chat.flag_label(&draft, fg) {
        let flag = format!(" {label} ");
        let w = (str_width(&flag) as u16).min(inner.width);
        let at = Rect { x: inner.x + inner.width - w, y: inner.y + text_h, width: w, height: flag_h };
        f.render_widget(Paragraph::new(Span::styled(flag, theme::badge(Ground::Info))), at);
        // A click on it opens the menu of the channel it names.
        chat.hits.flag = at;
    }
}

impl ChatPane {
    /// The destination flag for `text` in `fg`: where Enter would send it,
    /// or `None` while a slash command is composed. In a DM view an
    /// unaddressed message goes to its agent.
    pub(super) fn flag_label(&mut self, text: &str, fg: &Tab) -> Option<String> {
        let world = self.world();
        let quiet = super::keys::quiet_names(&world.seen);
        let swept = super::keys::swept_names(&world.rows);
        if let Tab::Dm(member) = fg {
            let quiet_agent = world.seen.iter().find(|s| s.member == *member).is_some_and(|s| !s.live);
            return super::keys::dm_destination_label(text, &self.dm_name(member), quiet_agent, &quiet, &swept);
        }
        let in_channel = matches!(fg, Tab::Channel(_));
        super::destination_label(text, &tabs::send_scope(fg), in_channel, &quiet, &swept)
    }
}

/// The helper row: what the buffer is reaching for (`crate::helper`) —
/// the agent legend, the channel legend, the slash commands, a level of
/// subcommands, or a free token's hint. Names Tab would complete to are
/// marked. The legends list what Tab completes over, as the column does:
/// quiet agents and swept channels too, set back.
fn draw_helper(chat: &mut ChatPane, f: &mut Frame, area: Rect, text: &str) {
    let depth = chat.palette.depth();
    let mut chips: Vec<Span<'static>> = Vec::new();
    // What a click on each chip completes to, in the chips' order.
    let mut picks: Vec<Chip> = Vec::new();
    match helper::derive(text) {
        HelperMode::Agents(p) => {
            // In the column's order, which is Tab's cycle.
            for s in super::sidebar::agent_order(&chat.world().seen) {
                let k = &s.identity;
                picks.push(Chip::Agent(k.nickname.clone()));
                let mut st = if k.is_claude && !s.live { theme::muted() } else { Style::new().fg(color_for(k.palette, depth)) };
                if k.style.bold {
                    st = st.add_modifier(Modifier::BOLD);
                }
                if k.style.italic {
                    st = st.add_modifier(Modifier::ITALIC);
                }
                // Marked as Tab would complete it: a claude, not a human.
                chips.push(Span::styled(format!("@{}", k.nickname), target(st, k.is_claude && prefix_match(&k.nickname, p.as_deref()))));
            }
        }
        HelperMode::Groups(p) => {
            for row in &chat.world().rows {
                let k = &row.known;
                picks.push(Chip::Group(k.group.name.clone()));
                let mut st = if row.on_disk { Style::new().fg(color_for(k.group.palette, depth)) } else { theme::muted() };
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
