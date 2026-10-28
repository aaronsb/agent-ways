//! The left column (ADR-405): Agents, every identity the chat has seen,
//! live ones first and the rest set back, those in the channel shown
//! marked, each claude followed by the rows of its DM views while they are
//! listed, the operator's DMs with it and then its own; then Channels,
//! every channel, including empty ones and ones only `_groups.yaml` still
//! names, numbered from 1 for Ctrl+digit, each followed by a row for each
//! of its members (amendment of 2026-10-15): those in its members
//! participate and are drawn as their Agents rows are, then those with a
//! pending invitation there are invited, set back and marked `◌ invited`
//! with the invitation's age where the row has room. An agent invited where
//! it already is participates. `#open` holds everyone and lists no one.
//!
//! A click on a claude's row shows the DMs between the operator and it,
//! and lists that view under the row; a click on a channel shows it. A
//! click on what is shown already opens its menu, as Ctrl+digit does; a
//! human's row and a member's row open the agent's menu at once. A menu
//! docks on the line under its row. `attend.chat.sidebar_key` shows or
//! hides the column, and a terminal too narrow for it beside the feed hides
//! it.

use agent_tui::ratatui::layout::{Constraint, Layout, Rect};
use agent_tui::ratatui::style::{Modifier, Style};
use agent_tui::ratatui::text::{Line, Span};
use agent_tui::ratatui::widgets::Paragraph;
use agent_tui::ratatui::Frame;
use agent_tui::strip::{prefix_match, target};
use agent_theme::{IdentitySteps, Rgb};
use agent_tui::ratatui::style::Color;
use agent_tui::theme::{self, Ground, Seg, Shape};
use agent_tui::wrap::str_width;

use super::ChatPane;
use crate::chip::{color_for, Seen};
use crate::groups::{ChannelRow, Invites};
use crate::helper::{self, HelperMode};
use crate::tabs::Tab;

/// The fewest columns the feed keeps beside the column; narrower, the
/// column hides.
const MIN_FEED: u16 = 50;

/// A row of the column, as a click or the keyboard acts on it: an agent
/// by its membership id (a session id, a human's username), which two
/// agents never share as they can a name; a DM view by its agent's id; a
/// channel by its name. Each DM view of an agent is a row under the
/// agent's: `Dm`, the operator's DMs with it, and `Review`, the agent's own
/// DMs with anyone, whose view offers nothing to send, since attend-chat
/// has no send into another agent's DMs. `Member` is a row under a
/// channel's, by the channel's name and the member's id, the same whether
/// the member participates or is invited, so the cursor keeps to it when
/// an invitation is answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SideHit {
    Agent(String),
    Dm(String),
    Review(String),
    Channel(String),
    Member(String, String),
}

/// A row of the Channels section: a channel, by its index in the world's
/// rows, or a member under it, with the invitation that makes it invited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ChannelItem<'a> {
    Channel(usize),
    Member { channel: &'a str, member: &'a str, invite: Option<&'a attend_groups::Invite> },
}

impl ChannelItem<'_> {
    pub(super) fn hit(&self, rows: &[ChannelRow]) -> SideHit {
        match *self {
            ChannelItem::Channel(i) => SideHit::Channel(rows[i].known.group.name.clone()),
            ChannelItem::Member { channel, member, .. } => SideHit::Member(channel.to_string(), member.to_string()),
        }
    }
}

/// The Channels section's rows in order: each channel, then its members,
/// those in its members first in their order, then those it has invited,
/// by id, leaving out any invited where it already is. `#open` holds every
/// session and lists none.
pub(super) fn channel_items<'a>(rows: &'a [ChannelRow], invites: &'a Invites) -> Vec<ChannelItem<'a>> {
    let mut out = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        out.push(ChannelItem::Channel(i));
        if row.known.is_base {
            continue;
        }
        let channel = row.known.group.name.as_str();
        let members = &row.known.membership.members;
        out.extend(members.iter().map(|m| ChannelItem::Member { channel, member: m, invite: None }));
        let invited = invites.get(channel).into_iter().flatten().filter(|i| !members.contains(&i.member));
        out.extend(invited.map(|i| ChannelItem::Member { channel, member: &i.member, invite: Some(i) }));
    }
    out
}

/// The DM views the column lists under agents' rows, each by its agent's
/// membership id, in the order opened: the operator's DMs with an agent
/// (`dms`), and the agent's own DMs with anyone (`reviews`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Views {
    pub(super) dms: Vec<String>,
    pub(super) reviews: Vec<String>,
}

/// The column's width in a pane `total` columns wide, or `None` when the
/// pane is too narrow to hold it beside the feed.
pub(super) fn width(total: u16) -> Option<u16> {
    let w = (total / 4).clamp(26, 34);
    (total >= w + MIN_FEED).then_some(w)
}

/// Whether the helper row has anything the column does not already show:
/// slash commands, their arguments and hints do; the agent and channel
/// lists are the column's while it is shown.
pub(super) fn helper_needed(text: &str) -> bool {
    !matches!(helper::derive(text), HelperMode::Agents(_) | HelperMode::Groups(_))
}

/// What Tab completes, as the column lists it: every claude, live or
/// quiet, and every channel, swept ones too. A human is listed but not
/// completed: `@name` routes to a session's inbox, and a human has none.
pub(super) fn completion_pool(seen: &[Seen], rows: &[ChannelRow]) -> (Vec<String>, Vec<String>) {
    let agents = agent_order(seen).into_iter().filter(|s| s.identity.is_claude).map(|s| s.identity.nickname.clone()).collect();
    let channels = rows.iter().map(|r| r.known.group.name.clone()).collect();
    (agents, channels)
}

/// The seen identities in the column's order: live ones first, each group
/// in the registry's order (newest sender first).
pub(super) fn agent_order(seen: &[Seen]) -> Vec<&Seen> {
    let mut out: Vec<&Seen> = seen.iter().filter(|s| s.live).collect();
    out.extend(seen.iter().filter(|s| !s.live));
    out
}

/// The Agents section's rows in order, each with its identity: every agent
/// in [`agent_order`], and under each claude the DM views `views` lists for
/// it, the operator's DMs with it first. A human has no DM view: `@name`
/// routes to a session's tray, and a human has none.
pub(super) fn agent_rows<'a>(seen: &'a [Seen], views: &Views) -> Vec<(SideHit, &'a Seen)> {
    let mut out = Vec::new();
    for s in agent_order(seen) {
        out.push((SideHit::Agent(s.member.clone()), s));
        if s.identity.is_claude && views.dms.contains(&s.member) {
            out.push((SideHit::Dm(s.member.clone()), s));
        }
        if s.identity.is_claude && views.reviews.contains(&s.member) {
            out.push((SideHit::Review(s.member.clone()), s));
        }
    }
    out
}

/// How many rows each section gets in `h` rows: each its header and rows
/// when both fit; otherwise channels up to half, agents the rest.
pub(super) fn split(h: u16, agents: usize, channels: usize) -> (u16, u16) {
    let want_a = agents as u16 + 1;
    let want_c = channels as u16 + 1;
    if want_a + want_c <= h {
        return (want_a, h - want_a);
    }
    let c = want_c.min(h / 2).max(h.saturating_sub(want_a));
    (h - c, c)
}

/// A string cut to `max` columns, ending in `…` when cut.
pub(super) fn fit(s: &str, max: usize) -> String {
    if str_width(s) <= max {
        return s.to_string();
    }
    let mut out = String::new();
    for ch in s.chars() {
        if str_width(&out) + str_width(&ch.to_string()) + 1 > max {
            break;
        }
        out.push(ch);
    }
    out.push('…');
    out
}

/// The column in `area`, with a rule on its right edge.
pub(super) fn draw(chat: &mut ChatPane, f: &mut Frame, area: Rect) {
    let text = chat.input.text().to_string();
    let (agent_partial, channel_partial) = match helper::derive(&text) {
        HelperMode::Agents(p) => (p, None),
        HelperMode::Groups(p) => (None, p),
        _ => (None, None),
    };
    let fg = chat.normal_tab();
    let new_dms = chat.new_dms();
    let focus = chat.side_focus();
    let views = chat.views.clone();
    let world = chat.world();
    let agents_list: Vec<(SideHit, Seen)> = agent_rows(&world.seen, &views).into_iter().map(|(h, s)| (h, s.clone())).collect();
    // Who is in the channel shown: a created channel's members, the
    // operator among them. `#open` and the operator view hold everyone.
    let members: Vec<String> = match &fg {
        Tab::Channel(c) => world.rows.iter().find(|r| r.known.group.name == *c && !r.known.is_base).map(|r| r.known.membership.members.clone()).unwrap_or_default(),
        Tab::Merged | Tab::Dm(_) | Tab::Review(_) => Vec::new(),
    };
    let live = world.seen.iter().filter(|s| s.live).count();
    let seen = world.seen.clone();
    let rows = world.rows.clone();
    let live_members = world.live_members.clone();
    let invites = world.invites.clone();
    let items = channel_items(&rows, &invites);
    let [inner, rule] = Layout::horizontal([Constraint::Min(1), Constraint::Length(1)]).areas(area);
    for y in rule.top()..rule.bottom() {
        f.render_widget(Paragraph::new(Span::styled("│", theme::rule())), Rect { y, height: 1, ..rule });
    }
    let (ah, ch) = split(inner.height, agents_list.len(), items.len());
    let agents = Rect { height: ah, ..inner };
    let channels = Rect { y: inner.y + ah, height: ch, ..inner };
    // Each list scrolls on its own, keeping the keyboard's cursor in view.
    let (vis_a, vis_c) = (ah.saturating_sub(1) as usize, ch.saturating_sub(1) as usize);
    let n_a = agents_list.len();
    let follow = focus.filter(|_| chat.side_follow);
    let sa = in_view(chat.side_scroll[0], n_a, vis_a, follow.filter(|f| *f < n_a));
    let sc = in_view(chat.side_scroll[1], items.len(), vis_c, follow.and_then(|f| f.checked_sub(n_a)));
    chat.side_scroll = [sa, sc];
    let below = |r: Rect| Rect { y: r.y + 1, height: r.height.saturating_sub(1), ..r };
    chat.hits.side_lists = [below(agents), below(channels)];
    header(f, agents, &format!("Agents {live}/{}{}", seen.len(), more(sa, vis_a, n_a)));
    let w = inner.width.saturating_sub(1) as usize;
    let mut hits = Vec::new();
    for (k, (i, (hit, s))) in agents_list.iter().enumerate().skip(sa).take(vis_a).enumerate() {
        let r = Rect { y: agents.y + 1 + k as u16, height: 1, ..agents };
        let focused = focus == Some(i);
        let mut line = match hit {
            SideHit::Dm(member) | SideHit::Review(member) => {
                let tab = match hit {
                    SideHit::Review(_) => Tab::Review(member.clone()),
                    _ => Tab::Dm(member.clone()),
                };
                let shown = fg == tab;
                let n = chat.in_view(&tab).filter(|x| x.channel != crate::signal::Channel::Info).count();
                let label = match tab {
                    Tab::Review(_) => format!("{REVIEW_MARK}{} DMs", s.identity.nickname),
                    _ => "DMs".to_string(),
                };
                let mut line = dm_line(chat, &label, n, shown, w, focused);
                if shown && !focused {
                    line.spans[0] = Span::styled(theme::selected_mark(), theme::accent());
                }
                line
            }
            _ => {
                // Marked as Tab would complete it: a claude, not a human.
                let mark = target(Style::new(), s.identity.is_claude && prefix_match(&s.identity.nickname, agent_partial.as_deref()));
                let mut line = agent_line(chat, s, w, mark, focused, new_dms.contains(&s.member));
                if members.contains(&s.member) && !focused {
                    line.spans[0] = Span::styled(theme::selected_mark(), theme::accent());
                }
                line
            }
        };
        if focused {
            line.spans[0] = cursor();
        }
        f.render_widget(Paragraph::new(line), r);
        hits.push((r, hit.clone()));
    }
    header(f, channels, &format!("Channels {}{}", rows.len(), more(sc, vis_c, items.len())));
    let now = chat.now();
    let age_of = |t: Option<std::time::SystemTime>| t.and_then(|t| now.duration_since(t).ok()).map(age_label).unwrap_or_default();
    for (k, (i, item)) in items.iter().enumerate().skip(sc).take(vis_c).enumerate() {
        let r = Rect { y: channels.y + 1 + k as u16, height: 1, ..channels };
        let focused = focus == Some(n_a + i);
        let mut line = match *item {
            ChannelItem::Channel(c) => {
                let row = &rows[c];
                let shown = fg == Tab::Channel(row.known.group.name.clone());
                let mark = target(Style::new(), prefix_match(&row.known.group.name, channel_partial.as_deref()));
                channel_line(chat, row, live_members[c], &age_of(row.last), shown, w, mark, focused, c + 1)
            }
            ChannelItem::Member { member, invite, .. } => {
                let who = seen.iter().find(|s| s.member == member);
                let label = who.map_or(member, |s| s.identity.nickname.as_str());
                member_line(chat, label, who, invite.map(|i| age_of(i.at)).as_deref(), w, focused)
            }
        };
        if focused {
            line.spans[0] = cursor();
        }
        f.render_widget(Paragraph::new(line), r);
        hits.push((r, item.hit(&rows)));
    }
    chat.hits.side = hits;
}

/// The first row to show of a list of `len` rows, `vis` at a time: `top`
/// held within the list, then moved just enough to show `cursor`.
pub(super) fn in_view(top: usize, len: usize, vis: usize, cursor: Option<usize>) -> usize {
    let mut top = top.min(len.saturating_sub(vis));
    if let Some(c) = cursor {
        if c < top {
            top = c;
        } else if vis > 0 && c >= top + vis {
            top = c + 1 - vis;
        }
    }
    top
}

/// A header's marks for rows out of view: `↑` above, `↓` below.
fn more(top: usize, vis: usize, len: usize) -> String {
    let mut out = String::new();
    if top > 0 {
        out += " ↑";
    }
    if top + vis < len {
        out += " ↓";
    }
    out
}

/// The keyboard's cursor at the left of a row.
fn cursor() -> Span<'static> {
    Span::styled("▸", theme::accent().add_modifier(Modifier::BOLD))
}

/// A row's segment styles drawn focused, as the tab bar draws its focused
/// tab: reversed and bold, so the focus reads as strongly in the column as
/// on the bar, with or without colour.
fn focused_styles(styles: [Style; 3], on: bool) -> [Style; 3] {
    if on {
        styles.map(|s| s.add_modifier(Modifier::REVERSED | Modifier::BOLD))
    } else {
        styles
    }
}

/// A time since, in its largest whole unit: `40s`, `4m`, `3h`, `2d`.
pub(super) fn age_label(d: std::time::Duration) -> String {
    let s = d.as_secs();
    match s {
        0..=59 => format!("{s}s"),
        60..=3599 => format!("{}m", s / 60),
        3600..=86_399 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86_400),
    }
}

fn header(f: &mut Frame, area: Rect, label: &str) {
    if area.height == 0 {
        return;
    }
    f.render_widget(Paragraph::new(Span::styled(format!(" {label}"), theme::muted().add_modifier(Modifier::BOLD))), Rect { height: 1, ..area });
}

/// Lozenge segments sized to `w` columns in `shape`: the first two hold
/// `texts[0]` and `texts[1]` cut or padded so the row fills `w`, the last
/// holds `texts[2]` whole. A project too narrow to read is dropped into the
/// name's padding, with the join it would have taken. `bare` is a frame
/// without colour, where the lozenge draws no glyphs at all.
fn sized(shape: Shape, bare: bool, texts: [&str; 3], w: usize) -> [String; 3] {
    // The glyphs `Shape::lozenge` draws for `n` segments: a cap, then a
    // join after each one.
    let glyphs = |n: usize| if bare { 0 } else { usize::from(!shape.cap.is_empty()) + n * usize::from(!shape.join.is_empty()) };
    let last = format!(" {} ", texts[2]);
    // The columns the name has beside the last segment and `n` segments'
    // glyphs, and the name cut to them.
    let room = |n: usize| w.saturating_sub(glyphs(n) + str_width(&last));
    let named = |room: usize| format!(" {} ", fit(texts[0], room.saturating_sub(2).max(1)));
    let name = named(room(3));
    let rest = room(3).saturating_sub(str_width(&name));
    if rest < 4 {
        // Two segments: the name is cut to the room the dropped join leaves.
        let name = named(room(2));
        let pad = room(2).saturating_sub(str_width(&name));
        return [format!("{name}{}", " ".repeat(pad)), String::new(), last];
    }
    let project = format!(" {}", fit(texts[1], rest - 2));
    let pad = rest.saturating_sub(str_width(&project));
    [name, format!("{project}{}", " ".repeat(pad)), last]
}

/// The segments of a sized row, leaving out an empty middle one.
fn segs(texts: [String; 3], styles: [Style; 3]) -> Vec<Seg> {
    texts.into_iter().zip(styles).filter(|(t, _)| !t.is_empty()).map(|(t, st)| Seg::styled(t, st)).collect()
}

/// A colour of the identity palette at the chat's depth.
fn at(c: Rgb, depth: agent_theme::ColorDepth) -> Color {
    agent_theme::Color::Rgb(c).at(depth).map(agent_theme::ratatui::color).unwrap_or(Color::Reset)
}

/// An agent's row, a lozenge in the status line's style: the name on the
/// identity colour, the project on a darker step of it, and the theme's
/// live or idle mark (`●` or `○` by default) on the darkest. While the agent has a DM to the operator not yet
/// shown (`new_dm`) the DM mark, bold, takes the dot's place; the colours
/// still say live or not. An inactive agent wears the desaturated steps.
/// At 16 colours the name keeps its ANSI colour and the rest is set back.
fn agent_line(chat: &ChatPane, s: &Seen, w: usize, mark: Style, focused: bool, new_dm: bool) -> Line<'static> {
    let depth = chat.palette.depth();
    let k = &s.identity;
    let project = agent_identity::cwd_basename(&k.cwd);
    let [live, idle] = theme::presence();
    let dot = match (new_dm, s.live) {
        (true, _) => chat.dm_glyph(),
        (false, true) => live,
        (false, false) => idle,
    };
    let texts = sized(chat.shape, theme::colourless(), [&k.nickname, &project, dot], w);
    let [name, project, dot] = identity_styles(depth, k.palette, s.live, k.style.bold);
    let dot = if new_dm { dot.add_modifier(Modifier::BOLD) } else { dot };
    let styles = focused_styles([name.patch(mark), project, dot], focused);
    let mut spans = vec![Span::raw(" ")];
    spans.extend(chat.shape.lozenge(&segs(texts, styles)));
    Line::from(spans)
}

/// An identity's lozenge styles at `depth`: the name on the identity colour,
/// the project on a darker step of it, the mark on the darkest; an
/// inactive identity wears the desaturated steps. At 16 colours the name
/// keeps its ANSI colour and the rest is set back. The left column's agent
/// rows and the feed's message headers both wear these.
pub(super) fn identity_styles(depth: agent_theme::ColorDepth, palette: agent_identity::PaletteEntry, live: bool, bold: bool) -> [Style; 3] {
    if agent_identity::is_rich(depth) {
        let st = IdentitySteps::of(Rgb(palette.rgb.0, palette.rgb.1, palette.rgb.2), live);
        let cell = |x: agent_theme::Step| Style::new().fg(at(x.fg, depth)).bg(at(x.bg, depth));
        let name = cell(st.name);
        [if live && bold { name.add_modifier(Modifier::BOLD) } else { name }, cell(st.project), cell(st.mark)]
    } else if live {
        let name = Style::new().fg(Color::Black).bg(color_for(palette, depth));
        [name, Seg::faded("").style, Seg::on("", Ground::Rule).style]
    } else {
        [Seg::faded("").style, Seg::faded("").style, Seg::faded("").style]
    }
}

/// What leads an agent's own DM view's row and tab, so it reads apart from
/// the operator's DMs with the agent, on the same ground, in any palette.
pub(super) const REVIEW_MARK: &str = "◈ ";

/// What sets a child row in from its parent's: a DM view's under its
/// agent's, a member's under its channel's.
const INDENT: &str = "  ";

/// What marks an invited member's row, before its live or idle mark.
const INVITED: &str = "◌ invited";

/// [`sized`] with no middle text: two segments, the name taking the
/// middle's columns and the join a third segment would have drawn. A name
/// too long for three is laid out in two already.
fn sized_two(shape: Shape, bare: bool, texts: [&str; 2], w: usize) -> [String; 3] {
    let [name, pad, last] = sized(shape, bare, [texts[0], "", texts[1]], w);
    let join = usize::from(!bare && !shape.join.is_empty() && !pad.is_empty());
    [format!("{name}{pad}{}", " ".repeat(join)), String::new(), last]
}

/// A DM view's row, under its agent's: indented, `label` on the info ground
/// the destination flag wears, then the count of the DMs it holds. Bold
/// while shown.
fn dm_line(chat: &ChatPane, label: &str, count: usize, shown: bool, w: usize, focused: bool) -> Line<'static> {
    let texts = sized_two(chat.shape, theme::colourless(), [label, &count.to_string()], w.saturating_sub(INDENT.len()));
    let lead = Seg::on("", Ground::Info).style;
    let styles = focused_styles([if shown { lead.add_modifier(Modifier::BOLD) } else { lead }, Style::new(), Seg::on("", Ground::Rule).style], focused);
    let mut spans = vec![Span::raw(" "), Span::raw(INDENT)];
    spans.extend(chat.shape.lozenge(&segs(texts, styles)));
    Line::from(spans)
}

/// A member's row, under its channel's: indented, as a DM view's row is. A
/// participant (`invited` none) is drawn as its Agents row is, its name on
/// the identity colour and its live or idle mark on the darkest step, with no
/// project. An invitee is set back, `label`, the invitation's age `invited`
/// where the row has room for it, then [`INVITED`] and the mark, so an
/// invitee that has gone reads apart from one that may still answer. `who`
/// is the agent as the column knows it; one it does not know is set back by
/// its id, and is not live.
fn member_line(chat: &ChatPane, label: &str, who: Option<&Seen>, invited: Option<&str>, w: usize, focused: bool) -> Line<'static> {
    let bare = theme::colourless();
    let w = w.saturating_sub(INDENT.len());
    let faded = Seg::faded("").style;
    let dot = theme::presence()[usize::from(!who.is_some_and(|s| s.live))];
    let (texts, styles) = match (invited, who) {
        (Some(age), _) if !age.is_empty() => (sized(chat.shape, bare, [label, age, &format!("{INVITED} {dot}")], w), [faded; 3]),
        (Some(_), _) => (sized_two(chat.shape, bare, [label, &format!("{INVITED} {dot}")], w), [faded; 3]),
        (None, Some(s)) => {
            let [name, _, mark] = identity_styles(chat.palette.depth(), s.identity.palette, s.live, s.identity.style.bold);
            (sized_two(chat.shape, bare, [label, dot], w), [name, Style::new(), mark])
        }
        (None, None) => (sized_two(chat.shape, bare, [label, dot], w), [faded; 3]),
    };
    let mut spans = vec![Span::raw(" "), Span::raw(INDENT)];
    spans.extend(chat.shape.lozenge(&segs(texts, focused_styles(styles, focused))));
    Line::from(spans)
}

/// A channel's row, the same shape on the accent: its number, glyph and
/// name, then its member count and the time since its last message, then
/// the live mark when a member is live (where D5's seal marker will go). The
/// shown channel is marked at the left. A channel with no live member, or
/// none on disk, is set back. `number` counts the channels from 1, for Ctrl+digit;
/// past the ninth a row has none.
#[allow(clippy::too_many_arguments)]
fn channel_line(chat: &ChatPane, row: &ChannelRow, live: usize, age: &str, shown: bool, w: usize, mark: Style, focused: bool, number: usize) -> Line<'static> {
    let g = &row.known.group;
    let active = row.on_disk && (row.known.is_base || live > 0);
    let person = chat.person_glyph();
    let figures = if row.known.is_base { age.to_string() } else { format!("{}{person} {age}", row.known.membership.members.len()).trim_end().to_string() };
    let num = if number <= 9 { format!("{number} ") } else { "  ".to_string() };
    let name = format!("{num}{} #{}", g.glyph, g.name);
    let dot = theme::presence()[usize::from(!active)];
    let texts = sized(chat.shape, theme::colourless(), [&name, &figures, dot], w);
    let styles = if active {
        let name = Seg::on("", Ground::Accent).style;
        [if shown { name.add_modifier(Modifier::BOLD) } else { name }.patch(mark), Seg::on("", Ground::AccentDim).style, Seg::on("", Ground::Rule).style]
    } else {
        [Seg::faded("").style.patch(mark), Seg::faded("").style, Seg::faded("").style]
    };
    let styles = focused_styles(styles, focused);
    let lead = if shown { Span::styled(theme::selected_mark(), theme::accent()) } else { Span::raw(" ") };
    let mut spans = vec![lead];
    spans.extend(chat.shape.lozenge(&segs(texts, styles)));
    Line::from(spans)
}

/// The column's rows of `world` in order: agents, each with the DM views
/// `views` lists for it ([`agent_rows`]), then channels, each with its
/// members ([`channel_items`]).
pub(super) fn side_items_of(seen: &[Seen], rows: &[ChannelRow], invites: &Invites, views: &Views) -> Vec<SideHit> {
    let mut out: Vec<SideHit> = agent_rows(seen, views).into_iter().map(|(h, _)| h).collect();
    out.extend(channel_items(rows, invites).iter().map(|i| i.hit(rows)));
    out
}

impl ChatPane {
    /// Whether the column is drawn in a pane `total` columns wide.
    pub(super) fn side_width(&self, total: u16) -> Option<u16> {
        if self.sidebar {
            width(total)
        } else {
            None
        }
    }

    /// Whether `k` is the key `attend.chat.sidebar_key` names.
    pub(super) fn is_sidebar_key(&self, k: &agent_tui::ratatui::crossterm::event::KeyEvent) -> bool {
        use agent_tui::ratatui::crossterm::event::{KeyCode, KeyModifiers};
        match self.config.sidebar_key.as_str() {
            "ctrl-b" => k.modifiers == KeyModifiers::CONTROL && k.code == KeyCode::Char('b'),
            "f3" => k.code == KeyCode::F(3),
            _ => false,
        }
    }

    /// The wheel over list `list` (0 agents, 1 channels): a row at a time.
    /// The next frame holds it within the list; the cursor, if any, stays
    /// where it is and is not brought back into view until it moves.
    pub(super) fn side_wheel(&mut self, list: usize, up: bool) {
        let top = &mut self.side_scroll[list];
        *top = if up { top.saturating_sub(1) } else { *top + 1 };
        self.side_follow = false;
        self.dirty = true;
    }

    /// Put the focus on row `i` of `items`, as a click on the row puts it
    /// there, the agent's menu the click asks for left unopened.
    #[cfg(test)]
    fn focus_at(&mut self, items: &[SideHit], i: usize) {
        self.dispatch(super::ix::Ev::Click(super::ix::Click::Pane { column: true, feed: false, row: items.get(i).cloned() }));
        self.to_open = None;
    }

    /// Show or hide the column. Hidden, it takes the focus off its rows.
    pub(super) fn toggle_sidebar(&mut self) {
        self.sidebar = !self.sidebar;
        if !self.sidebar {
            self.dispatch(super::ix::Ev::ColumnHidden);
        }
        self.invalidate();
    }

    /// The column's row drawn for `hit` in the last frame, where a menu
    /// docks: `None` while the column is hidden or the row is scrolled out
    /// of view.
    pub(super) fn side_row(&self, hit: &SideHit) -> Option<Rect> {
        self.hits.side.iter().find(|(_, h)| h == hit).map(|(r, _)| *r)
    }

    /// The column's rows in order, agents then channels, as the keyboard
    /// walks them, the world read first.
    #[cfg(test)]
    fn side_items(&mut self) -> Vec<SideHit> {
        self.world().laid.rows.clone()
    }

    /// Whether the column is drawn in the frame now being drawn, at the
    /// width [`agent_tui::Pane::layout`] told before the tab bar; before any
    /// frame, whether it is shown.
    pub(super) fn column_this_frame(&self) -> bool {
        self.width.map_or(self.sidebar, |w| self.side_width(w).is_some())
    }

    /// What takes an agent's live mark while it has a DM to the operator not
    /// yet shown ([`super::dm`]): an envelope, the Nerd Font's in a shape
    /// that needs one.
    pub(super) fn dm_glyph(&self) -> &'static str {
        if self.shape.join.is_empty() {
            "✉"
        } else {
            "\u{f0e0}"
        }
    }

    /// The person glyph a member count carries: a Nerd Font's beside the
    /// lozenge glyphs it already needs, the emoji where the shape is plain.
    pub(super) fn person_glyph(&self) -> &'static str {
        if self.shape.join.is_empty() {
            "👤"
        } else {
            "\u{f007}"
        }
    }

    /// Whether the column was drawn in the last frame drawn: it had room
    /// then, and it has not been hidden since, so a hide takes effect before
    /// the next frame shows it.
    pub(super) fn side_drawn(&self) -> bool {
        self.sidebar && !self.hits.side_area.is_empty()
    }

    /// The key that toggles the column, as the help names it.
    pub(super) fn sidebar_key_label(&self) -> Option<&'static str> {
        match self.config.sidebar_key.as_str() {
            "ctrl-b" => Some("Ctrl+B"),
            "f3" => Some("F3"),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_column_hides_when_the_feed_would_get_too_narrow() {
        assert_eq!(width(80), Some(26));
        assert_eq!(width(120), Some(30));
        assert_eq!(width(200), Some(34));
        assert_eq!(width(75), None, "26 + 50 columns is the least that holds it");
        assert_eq!(width(76), Some(26));
    }

    #[test]
    fn both_sections_get_their_rows_when_they_fit_else_channels_up_to_half() {
        assert_eq!(split(20, 3, 4), (4, 16), "agents take their header and rows; channels the rest");
        assert_eq!(split(10, 12, 12), (5, 5));
        assert_eq!(split(10, 12, 2), (7, 3), "a short channel list leaves the rest to agents");
        assert_eq!(split(10, 2, 12), (3, 7), "a short agent list leaves the rest to channels");
    }

    #[test]
    fn the_helper_row_stays_for_commands_and_goes_for_lists() {
        assert!(!helper_needed(""), "the agent legend is the column's");
        assert!(!helper_needed("hi #de"), "so is the channel list");
        assert!(helper_needed("/he"), "slash commands stay in the row");
    }

    #[test]
    fn a_list_scrolls_to_keep_the_cursor_in_view_and_stays_within_itself() {
        assert_eq!(in_view(0, 15, 5, None), 0);
        assert_eq!(in_view(0, 15, 5, Some(7)), 3, "the cursor at the bottom row");
        assert_eq!(in_view(6, 15, 5, Some(2)), 2, "the cursor at the top row");
        assert_eq!(in_view(40, 15, 5, None), 10, "the wheel stops at the last row");
        assert_eq!(in_view(3, 4, 10, None), 0, "a list that fits does not scroll");
        assert_eq!(more(0, 5, 15), " ↓");
        assert_eq!(more(10, 5, 15), " ↑");
        assert_eq!(more(0, 5, 5), "");
    }

    fn seen(name: &str) -> Seen {
        let identity = crate::chip::KnownIdentity {
            nickname: name.into(),
            cwd: format!("/work/{name}"),
            is_claude: true,
            palette: agent_identity::Identity::for_cwd("/w", agent_theme::ColorDepth::Ansi16).palette,
            style: Default::default(),
        };
        Seen { identity, live: true, member: format!("id-{name}") }
    }

    fn with_agents(c: &mut ChatPane, names: &[&str]) {
        let seen = names.iter().map(|n| seen(n)).collect();
        c.world = Some(super::super::World::new(Vec::new(), seen, Vec::new(), Vec::new(), Default::default(), attend_instances::SnapshotCache::new(), &c.foreground, &c.views));
        c.refocus();
    }

    #[test]
    fn the_cursor_keeps_to_its_row_across_a_refresh_and_holds_within_a_shorter_list() {
        let mut c = ChatPane::new(None, Default::default());
        with_agents(&mut c, &["Ada", "Bo", "Cy"]);
        let items = c.side_items();
        c.focus_at(&items, 1);
        with_agents(&mut c, &["New", "Ada", "Bo", "Cy"]);
        assert_eq!((c.side_focus(), c.ix.row_hit().cloned()), (Some(2), Some(SideHit::Agent("id-Bo".into()))), "on Bo, moved down a row");
        with_agents(&mut c, &["Ada"]);
        assert_eq!((c.side_focus(), c.ix.row_hit().cloned()), (Some(0), Some(SideHit::Agent("id-Ada".into()))), "Bo gone: held within the list");
        with_agents(&mut c, &[]);
        assert_eq!(c.side_focus(), None, "nothing left to be on");
    }

    #[test]
    fn the_wheel_moves_a_focused_list_away_from_its_cursor() {
        let mut c = ChatPane::new(None, Default::default());
        with_agents(&mut c, &["Ada", "Bo"]);
        let items = c.side_items();
        c.focus_at(&items, 0);
        c.side_wheel(0, false);
        assert!(!c.side_follow, "the next frame leaves the list where the wheel put it");
        c.focus_at(&items, 1);
        assert!(c.side_follow, "a move of the cursor brings it back into view");
    }

    #[test]
    fn a_dm_view_is_listed_under_its_claude_and_never_under_a_human() {
        let mut human = seen("aaron");
        human.identity.is_claude = false;
        let list = [seen("Ada"), human, seen("Bo")];
        let views = Views { dms: vec!["id-Bo".to_string(), "id-aaron".to_string()], reviews: vec!["id-aaron".to_string(), "id-Bo".to_string(), "id-Ada".to_string()] };
        let hits: Vec<SideHit> = agent_rows(&list, &views).into_iter().map(|(h, _)| h).collect();
        let a = |m: &str| SideHit::Agent(m.into());
        let r = |m: &str| SideHit::Review(m.into());
        assert_eq!(hits, [a("id-Ada"), r("id-Ada"), a("id-aaron"), a("id-Bo"), SideHit::Dm("id-Bo".into()), r("id-Bo")], "the operator's DMs first, then the agent's own");
    }

    fn row(name: &str, members: &[&str], base: bool) -> ChannelRow {
        let membership = attend_groups::GroupEntry { members: members.iter().map(|m| m.to_string()).collect(), ..Default::default() };
        let group = agent_identity::Group::for_name(name, agent_theme::ColorDepth::Ansi16);
        ChannelRow { known: crate::groups::KnownGroup { group, membership, is_base: base }, last: None, on_disk: true }
    }

    fn invite(channel: &str, member: &str) -> attend_groups::Invite {
        attend_groups::Invite { channel: channel.into(), member: member.into(), by: Some("aaron".into()), at: None }
    }

    /// The Channels section as (row, invited) pairs: `#c` for a channel,
    /// `c/m` for a member.
    fn listed(rows: &[ChannelRow], invites: &Invites) -> Vec<(String, bool)> {
        channel_items(rows, invites)
            .into_iter()
            .map(|i| match i {
                ChannelItem::Channel(c) => (format!("#{}", rows[c].known.group.name), false),
                ChannelItem::Member { channel, member, invite } => (format!("{channel}/{member}"), invite.is_some()),
            })
            .collect()
    }

    #[test]
    fn each_channel_lists_its_members_under_it_participants_then_invitees() {
        let rows = [row("open", &[], true), row("deploy", &["s1", "aaron"], false), row("infra", &["s1"], false), row("empty", &[], false)];
        let invites: Invites = [("deploy".to_string(), vec![invite("deploy", "s2"), invite("deploy", "s1")]), ("infra".to_string(), vec![invite("infra", "s3")])].into();
        let l = |s: &str, invited| (s.to_string(), invited);
        assert_eq!(
            listed(&rows, &invites),
            [l("#open", false), l("#deploy", false), l("deploy/s1", false), l("deploy/aaron", false), l("deploy/s2", true), l("#infra", false), l("infra/s1", false), l("infra/s3", true), l("#empty", false)],
            "#open lists no one; s1, in two channels, under both; s1 invited where it already is participates, listed once; a human is listed"
        );
    }

    #[test]
    fn the_walk_takes_the_member_rows_in_the_order_drawn() {
        let rows = [row("deploy", &["s1"], false)];
        let invites: Invites = [("deploy".to_string(), vec![invite("deploy", "s2")])].into();
        let items = side_items_of(&[seen("Ada")], &rows, &invites, &Views::default());
        let m = |c: &str, id: &str| SideHit::Member(c.into(), id.into());
        assert_eq!(items, [SideHit::Agent("id-Ada".into()), SideHit::Channel("deploy".into()), m("deploy", "s1"), m("deploy", "s2")]);
    }

    #[test]
    fn an_invited_row_is_set_back_and_marked_apart_from_a_participants() {
        let c = ChatPane::new(None, Default::default());
        let text = |l: &Line| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>();
        let ada = seen("Ada");
        let joined = member_line(&c, "Ada", Some(&ada), None, 24, false);
        let invited = member_line(&c, "Ada", Some(&ada), Some("4m"), 24, false);
        let (joined_t, invited_t) = (text(&joined), text(&invited));
        assert!(joined_t.contains('●') && !joined_t.contains(INVITED), "{joined_t:?}");
        assert!(invited_t.contains(&format!("{INVITED} ●")) && invited_t.contains("4m"), "a live invitee: {invited_t:?}");
        let quiet = Seen { live: false, ..seen("Ada") };
        let gone = text(&member_line(&c, "Ada", Some(&quiet), Some("4m"), 24, false));
        assert!(gone.contains(&format!("{INVITED} ○")), "a quiet invitee reads apart: {gone:?}");
        let unknown = text(&member_line(&c, "s9", None, Some(""), 24, false));
        assert!(unknown.contains(&format!("{INVITED} ○")), "one the column does not know is not live: {unknown:?}");
        assert_eq!((joined.width(), invited.width()), (1 + 24, 1 + 24), "both fill the row");
        let faded = Seg::faded("").style;
        assert!(invited.spans.iter().filter(|s| !s.content.trim().is_empty()).all(|s| s.style == faded), "the invited row is set back whole: {invited:?}");
        assert_ne!(joined.spans.iter().find(|s| s.content.contains("Ada")).map(|s| s.style), Some(faded), "a participant wears its identity: {joined:?}");
        let narrow = text(&member_line(&c, "Shannon-alphabet", Some(&ada), Some("4m"), 24, false));
        assert!(narrow.contains(INVITED) && !narrow.contains("4m"), "the age goes first where the row has no room: {narrow:?}");
    }

    #[test]
    fn the_cursor_on_a_member_row_follows_it_and_falls_to_its_channel_when_it_goes() {
        let mut c = ChatPane::new(None, Default::default());
        let world = |c: &mut ChatPane, members: &[&str], invited: &[&str]| {
            let invites: Invites = [("deploy".to_string(), invited.iter().map(|m| invite("deploy", m)).collect())].into();
            let rows = vec![row("deploy", members, false)];
            c.world = Some(super::super::World::new(Vec::new(), vec![seen("Ada")], rows, vec![0], invites, attend_instances::SnapshotCache::new(), &c.foreground, &c.views));
            c.refocus();
        };
        world(&mut c, &[], &["s2"]);
        let items = c.side_items();
        c.focus_at(&items, 2);
        let on = SideHit::Member("deploy".into(), "s2".into());
        world(&mut c, &["s1", "s2"], &[]);
        assert_eq!((c.side_focus(), c.ix.row_hit().cloned()), (Some(3), Some(on.clone())), "invited, then joined: the cursor stays on it, a row down");
        world(&mut c, &["s1"], &[]);
        assert_eq!(c.ix.row_hit().cloned(), Some(SideHit::Channel("deploy".into())), "its row gone: on its channel's");
    }

    #[test]
    fn a_dm_row_fills_its_width() {
        let mut c = ChatPane::new(None, Default::default());
        for shape in [Shape::PLAIN, Shape::ROUND] {
            c.shape = shape;
            for label in ["DMs", "Shannon-alphabet-omnibus DMs"] {
                let line = dm_line(&c, label, 3, false, 28, false);
                assert_eq!(line.width(), 1 + 28, "{shape:?}: {line:?}");
            }
        }
    }


    #[test]
    fn an_age_is_its_largest_whole_unit() {
        let a = |s| age_label(std::time::Duration::from_secs(s));
        assert_eq!([a(5), a(240), a(7200), a(200_000)], ["5s", "4m", "2h", "2d"]);
    }

    #[test]
    fn a_row_fills_its_width_in_any_shape() {
        let width = |t: &[String; 3]| t.iter().map(|x| str_width(x)).sum::<usize>();
        let t = sized(Shape::ROUND, false, ["Laszlo", "alpha", "●"], 22);
        assert_eq!(t, [" Laszlo ".to_string(), " alpha ".into(), " ● ".into()]);
        assert_eq!(width(&t) + 4, 22, "cap and three joins");
        let t = sized(Shape::PLAIN, false, ["Laszlo", "alpha", "●"], 22);
        assert_eq!(width(&t), 22, "plain draws no glyphs");
        let t = sized(Shape::ROUND, false, ["Shannon-alphabet", "omnibus", "○"], 22);
        assert_eq!((t[1].as_str(), width(&t) + 3), ("", 22), "no room to read the project: it goes, and its join with it");
        assert_eq!(t[0], " Shannon-alpha… ", "the name takes the join's column, cut for two segments' glyphs, not three");
        let t = sized(Shape::ROUND, true, ["Laszlo", "alpha", "●"], 22);
        assert_eq!(width(&t), 22, "without colour the lozenge draws no glyphs, in any shape");
    }

    #[test]
    fn fit_cuts_with_an_ellipsis() {
        assert_eq!(fit("agent-ways", 20), "agent-ways");
        assert_eq!(fit("agent-ways", 6), "agent…");
    }
}
