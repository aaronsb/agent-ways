//! The participant strip: who takes part in the channel or DM view shown,
//! on up to three rows above the feed, beneath the shell's tab bar, across
//! the right-hand content area only. The operator view (⋯all) has none.
//!
//! A channel lists what the left column lists under its row: its members,
//! then those it has invited, set back and marked `◌ invited`. `#open`
//! holds every session and the column lists no one under it, so its strip
//! lists the live identities, in the column's order: who would read a post
//! there now. A DM view lists who appears in its messages, senders and
//! addressees, its agent first; the operator's DMs with an agent that holds
//! no message yet list the agent and the person at the chat.
//!
//! Each name is drawn as its member row in the column is
//! ([`super::sidebar::identity_styles`]): the name on the identity colour,
//! the live or idle mark on the darkest step, set back when not live. When
//! the rows cannot hold every name, the third row ends with an ellipsis,
//! `…`, or `...` where the palette is not rich, as other glyphs fall back.
//! The strip only displays; a click on it is any other click.

use agent_identity::{PaletteEntry, Style as IdStyle};
use agent_tui::ratatui::layout::Rect;
use agent_tui::ratatui::style::{Modifier, Style};
use agent_tui::ratatui::text::{Line, Span};
use agent_tui::ratatui::widgets::Paragraph;
use agent_tui::ratatui::Frame;
use agent_tui::theme::{self, Seg};
use agent_tui::wrap::str_width;

use super::sidebar::{channel_items, ChannelItem};
use super::ChatPane;
use crate::chip::{chip_for, Seen};
use crate::groups::{ChannelRow, Invites};
use crate::signal::{Channel, Signal};
use crate::tabs::Tab;

/// The most rows the strip takes from the feed.
pub(super) const MAX_ROWS: usize = 3;

/// What marks an invited name, as the column marks an invited member's row.
const INVITED: &str = "◌ invited";

/// One participant as the strip draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Who {
    /// The membership id: a claude's session id, a human's username.
    pub(super) member: String,
    pub(super) label: String,
    /// The identity's colour and style; `None` for an id the chat does not
    /// know, which is set back.
    pub(super) look: Option<(PaletteEntry, IdStyle)>,
    pub(super) live: bool,
    pub(super) invited: bool,
}

impl Who {
    fn of(s: &Seen) -> Who {
        Who { member: s.member.clone(), label: s.identity.nickname.clone(), look: Some((s.identity.palette, s.identity.style)), live: s.live, invited: false }
    }

    /// An id the chat does not know: set back by its id, not live, as the
    /// column draws one.
    fn unknown(member: &str, label: String) -> Who {
        Who { member: member.to_string(), label, look: None, live: false, invited: false }
    }
}

/// What a channel's strip lists: what the column lists under the channel's
/// row ([`channel_items`]), in its order, participants then invitees.
pub(super) fn channel_who(rows: &[ChannelRow], invites: &Invites, seen: &[Seen], channel: &str) -> Vec<Who> {
    channel_items(rows, invites)
        .into_iter()
        .filter_map(|item| match item {
            ChannelItem::Member { channel: c, member, invite } if c == channel => {
                let who = seen.iter().find(|s| s.member == member).map_or_else(|| Who::unknown(member, member.to_string()), Who::of);
                Some(Who { invited: invite.is_some(), ..who })
            }
            _ => None,
        })
        .collect()
}

/// What `#open`'s strip lists: every live identity, in the column's order.
pub(super) fn open_who(seen: &[Seen]) -> Vec<Who> {
    super::sidebar::agent_order(seen).into_iter().filter(|s| s.live).map(Who::of).collect()
}

/// The person at this chat, for a DM view: their membership id, and how a
/// message they sent or one sent to them names them.
pub(super) struct Person<'a> {
    pub(super) member: &'a str,
    pub(super) from: &'a str,
}

/// What a DM view's strip lists: its agent `agent` first, then each
/// sender and addressee of `signals` in the order they first appear. A
/// sender the chat has seen is drawn as the column draws it, one it has
/// not as the feed draws its header (`look`), live; an agent or addressee
/// it has not seen is set back by `name`. A message to the person's tray names the person
/// at the chat. With nobody but the agent, `operator` adds the person: the
/// operator's DMs with an agent are theirs before a message is sent.
pub(super) fn dm_who<'a>(
    signals: impl IntoIterator<Item = &'a Signal>,
    seen: &[Seen],
    agent: &str,
    person: &Person,
    operator: bool,
    look: impl Fn(&Signal) -> (String, PaletteEntry, IdStyle),
    name: impl Fn(&str) -> String,
) -> Vec<Who> {
    let mut out: Vec<Who> = Vec::new();
    let known = |m: &str| seen.iter().find(|s| s.member == m).map(Who::of);
    let add = |out: &mut Vec<Who>, who: Who| {
        if !out.iter().any(|w| w.member == who.member) {
            out.push(who);
        }
    };
    add(&mut out, known(agent).unwrap_or_else(|| Who::unknown(agent, name(agent))));
    // The person at the chat, drawn as a message from them would be.
    let me = |seen_one: Option<Who>| {
        seen_one.unwrap_or_else(|| {
            let probe = Signal { from: person.from.to_string(), ..empty() };
            let (label, palette, style) = look(&probe);
            Who { member: person.member.to_string(), label, look: Some((palette, style)), live: true, invited: false }
        })
    };
    for s in signals {
        let to = match &s.channel {
            Channel::Info | Channel::Open | Channel::Group(_) => continue,
            Channel::Person => None,
            Channel::Direct(to) | Channel::Sent(to) | Channel::Elsewhere(to) => Some(to.as_str()),
        };
        if let Some(m) = s.sender_member() {
            let who = known(&m).unwrap_or_else(|| {
                let (label, palette, style) = look(s);
                Who { member: m.clone(), label, look: Some((palette, style)), live: true, invited: false }
            });
            add(&mut out, who);
        }
        match to {
            None => add(&mut out, me(known(person.member))),
            Some(to) => {
                for m in to.split(',').filter(|m| !m.is_empty()) {
                    let who = match known(m) {
                        Some(w) => w,
                        None if m == person.member => me(None),
                        None => Who::unknown(m, name(m)),
                    };
                    add(&mut out, who);
                }
            }
        }
    }
    if operator && out.len() == 1 {
        add(&mut out, me(known(person.member)));
    }
    out
}

/// A signal with nothing in it, to ask how a sender would be drawn.
fn empty() -> Signal {
    Signal { id: String::new(), from: String::new(), project: String::new(), cwd: String::new(), reply_to: None, message: String::new(), ts: 0, channel: Channel::Info, tray: String::new(), beside_post: false }
}

/// How the strip lays its chips out in `width` columns on at most
/// [`MAX_ROWS`] rows: how many chips each row holds, in order, and how
/// many are left out, which the [`cut`] mark at the end of the last row stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Laid {
    pub(super) rows: Vec<usize>,
    pub(super) more: usize,
}

/// What marks names left out: an ellipsis, in ASCII where the palette is
/// not rich.
pub(super) fn cut(depth: agent_theme::ColorDepth) -> &'static str {
    if agent_identity::is_rich(depth) {
        "…"
    } else {
        "..."
    }
}

/// Lay out chips of `widths` in rows of `width` columns, a space before
/// each chip, one row when they fit and up to [`MAX_ROWS`]. A chip wider
/// than a row takes a row of its own; the caller cuts its name to fit.
/// When the last row cannot hold the rest it ends with the cut mark,
/// `mark` columns wide, which the space it needs is kept for.
pub(super) fn lay(widths: &[usize], width: usize, mark: usize) -> Laid {
    let mut rows = Vec::new();
    let mut i = 0;
    while i < widths.len() && rows.len() < MAX_ROWS {
        let last = rows.len() + 1 == MAX_ROWS;
        let start = i;
        let mut used = 0;
        while i < widths.len() && (i == start || used + 1 + widths[i] <= width) {
            used += 1 + widths[i];
            i += 1;
        }
        if last && i < widths.len() {
            // Keep room for the mark, taking back chips until it fits.
            while i > start && used + 1 + mark > width {
                i -= 1;
                used -= 1 + widths[i];
            }
        }
        rows.push(i - start);
    }
    Laid { rows, more: widths.len() - i }
}

/// One participant's chip: a lozenge in the chat's shape, the name on the
/// identity colour and the live or idle mark on its darkest step, as a
/// member row in the column; an invited one set back, marked [`INVITED`];
/// an unknown one set back. The name is cut to `max` columns.
fn chip(chat: &ChatPane, who: &Who, max: usize) -> Vec<Span<'static>> {
    let dot = theme::presence()[usize::from(!who.live)];
    let faded = Seg::faded("").style;
    let label = super::sidebar::fit(&who.label, max.max(1));
    let (styles, mark) = match (&who.look, who.invited) {
        (Some((palette, style)), false) => {
            let [name, _, mark] = super::sidebar::identity_styles(chat.palette.depth(), *palette, who.live, style.bold);
            let name = if style.italic { name.add_modifier(Modifier::ITALIC) } else { name };
            ([name, mark], dot.to_string())
        }
        (_, true) => ([faded; 2], format!("{INVITED} {dot}")),
        (None, false) => ([faded; 2], dot.to_string()),
    };
    let segs = [Seg::styled(format!(" {label} "), styles[0]), Seg::styled(format!(" {mark} "), styles[1])];
    chat.shape.lozenge(&segs)
}

fn spans_width(spans: &[Span]) -> usize {
    spans.iter().map(|s| str_width(&s.content)).sum()
}

/// The strip's rows for `who` in `width` columns, ready to draw: one
/// [`Line`] per row, the last ending in the [`cut`] mark when names are
/// left out.
/// None at all when `who` is empty.
pub(super) fn lines(chat: &ChatPane, who: &[Who], width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width);
    // A name is cut so its chip fits a row with its lead space and the mark.
    let mark = cut(chat.palette.depth());
    let max = width.saturating_sub(12);
    let chips: Vec<Vec<Span<'static>>> = who.iter().map(|w| chip(chat, w, max)).collect();
    let widths: Vec<usize> = chips.iter().map(|c| spans_width(c)).collect();
    let laid = lay(&widths, width, str_width(mark));
    let n = laid.rows.len();
    let mut next = chips.iter();
    laid.rows
        .into_iter()
        .enumerate()
        .map(|(r, count)| {
            let mut spans = Vec::new();
            for c in next.by_ref().take(count) {
                spans.push(Span::raw(" "));
                spans.extend(c.iter().cloned());
            }
            if r + 1 == n && laid.more > 0 {
                spans.push(Span::raw(" "));
                spans.push(Span::styled(mark, theme::muted().add_modifier(Modifier::BOLD)));
            }
            Line::from(spans)
        })
        .collect()
}

/// Draw the strip's `lines` in `area`.
pub(super) fn draw(f: &mut Frame, area: Rect, lines: Vec<Line<'static>>) {
    f.render_widget(Paragraph::new(lines).style(Style::new()), area);
}

impl ChatPane {
    /// Who the strip lists for `tab`: none for the operator view.
    pub(super) fn participants(&mut self, tab: &Tab) -> Vec<Who> {
        let depth = self.palette.depth();
        self.world();
        let world = self.world.as_ref().expect("just read");
        match tab {
            Tab::Merged => Vec::new(),
            Tab::Channel(c) => match world.rows.iter().find(|r| r.known.group.name == *c) {
                Some(row) if row.known.is_base => open_who(&world.seen),
                Some(_) => channel_who(&world.rows, &world.invites, &world.seen, c),
                None => Vec::new(),
            },
            Tab::Dm(member) | Tab::Review(member) => {
                let user = agent_identity::signal::user_name();
                let me = crate::signal::human_member_id();
                let from = format!("external:{user}");
                let person = Person { member: &me, from: &from };
                let instances = &world.instances;
                let look = |s: &Signal| {
                    let c = chip_for(&s.from, &s.project, &s.cwd, depth, instances);
                    (c.primary, c.palette, c.style)
                };
                let name = |m: &str| self.dm_name(m);
                let in_view: Vec<&Signal> = self.in_view(tab).collect();
                dm_who(in_view, &world.seen, member, &person, matches!(tab, Tab::Dm(_)), look, name)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chip::KnownIdentity;
    use agent_theme::ColorDepth;

    fn seen(name: &str, member: &str, live: bool, claude: bool) -> Seen {
        let identity = KnownIdentity {
            nickname: name.into(),
            cwd: format!("/work/{name}"),
            is_claude: claude,
            palette: agent_identity::Identity::for_cwd("/w", ColorDepth::Ansi16).palette,
            style: Default::default(),
        };
        Seen { identity, live, member: member.into() }
    }

    fn row(name: &str, members: &[&str], base: bool) -> ChannelRow {
        let membership = attend_groups::GroupEntry { members: members.iter().map(|m| m.to_string()).collect(), ..Default::default() };
        let group = agent_identity::Group::for_name(name, ColorDepth::Ansi16);
        ChannelRow { known: crate::groups::KnownGroup { group, membership, is_base: base }, last: None, on_disk: true }
    }

    fn labels(who: &[Who]) -> Vec<(String, bool, bool)> {
        who.iter().map(|w| (w.label.clone(), w.live, w.invited)).collect()
    }

    #[test]
    fn a_channel_lists_its_members_then_its_invitees_as_the_column_does() {
        let rows = vec![row("open", &[], true), row("deploy", &["s1", "aaron"], false), row("infra", &["s2"], false)];
        let invite = attend_groups::Invite { channel: "deploy".into(), member: "s2".into(), by: None, at: None };
        let again = attend_groups::Invite { member: "s1".into(), ..invite.clone() };
        let invites: Invites = [("deploy".to_string(), vec![invite, again])].into();
        let seen = vec![seen("Wells", "s1", true, true), seen("Laszlo", "s2", false, true), seen("aaron", "aaron", true, false)];
        let who = channel_who(&rows, &invites, &seen, "deploy");
        assert_eq!(labels(&who), vec![("Wells".into(), true, false), ("aaron".into(), true, false), ("Laszlo".into(), false, true)]);
        // A member the chat does not know is listed by its id, set back.
        let rows = vec![row("x", &["ghost"], false)];
        let who = channel_who(&rows, &Invites::new(), &seen, "x");
        assert_eq!((who[0].label.as_str(), who[0].look.is_none(), who[0].live), ("ghost", true, false));
    }

    #[test]
    fn open_lists_the_live_identities_in_the_columns_order() {
        let seen = vec![seen("Quiet", "q", false, true), seen("Wells", "s1", true, true), seen("aaron", "aaron", true, false)];
        assert_eq!(labels(&open_who(&seen)), vec![("Wells".into(), true, false), ("aaron".into(), true, false)]);
    }

    fn sig(from: &str, channel: Channel) -> Signal {
        Signal { from: from.into(), channel, ..empty() }
    }

    fn look(s: &Signal) -> (String, PaletteEntry, IdStyle) {
        let c = chip_for(&s.from, &s.project, &s.cwd, ColorDepth::Ansi16, &attend_instances::SnapshotCache::new());
        (c.primary, c.palette, c.style)
    }

    #[test]
    fn a_dm_view_lists_who_appears_in_its_messages_two_humans_included() {
        let seen = vec![seen("Wells", "s1", true, true), seen("aaron", "aaron", true, false)];
        let person = Person { member: "aaron", from: "external:aaron@pts" };
        let name = |m: &str| m.to_string();
        // The agent writes to the person; the person writes to the agent
        // and to a second human, bob, whom the chat has not seen.
        let msgs = vec![sig("claude:s1", Channel::Person), sig("external:aaron@pts", Channel::Sent("s1,bob".into()))];
        let who = dm_who(&msgs, &seen, "s1", &person, true, look, name);
        assert_eq!(labels(&who), vec![("Wells".into(), true, false), ("aaron".into(), true, false), ("bob".into(), false, false)]);
        // A second human who writes is drawn in their own colour, live.
        let msgs = vec![sig("external:carol@pts", Channel::Direct("s1".into())), sig("claude:s1", Channel::Person)];
        let who = dm_who(&msgs, &seen, "s1", &person, true, look, name);
        assert_eq!(labels(&who), vec![("Wells".into(), true, false), ("carol".into(), true, false), ("aaron".into(), true, false)]);
        assert!(who[1].look.is_some());
    }

    #[test]
    fn an_empty_dm_view_lists_the_agent_and_the_person_and_a_review_the_agent_alone() {
        let person = Person { member: "aaron", from: "external:aaron@pts" };
        // The agent the chat has not seen is named as `dm_name` names it.
        let name = |m: &str| if m == "s1" { "Wells".to_string() } else { m.to_string() };
        let notice = vec![sig("", Channel::Info)];
        let who = dm_who(&notice, &[], "s1", &person, true, look, name);
        assert_eq!(labels(&who), vec![("Wells".into(), false, false), ("aaron".into(), true, false)]);
        let who = dm_who(&notice, &[], "s1", &person, false, look, name);
        assert_eq!(who.len(), 1);
        // A review lists every counterparty of the agent's DMs.
        let msgs = vec![sig("claude:s1", Channel::Elsewhere("s2,s3".into())), sig("claude:s4", Channel::Elsewhere("s1".into()))];
        let who = dm_who(&msgs, &[], "s1", &person, false, look, name);
        assert_eq!(who.iter().map(|w| w.member.as_str()).collect::<Vec<_>>(), vec!["s1", "s2", "s3", "s4"]);
    }

    #[test]
    fn the_strip_takes_one_row_when_the_names_fit_and_grows_to_three() {
        // One row: 3 chips of 9, each with its lead space, in 40 columns;
        // four fill 40 exactly.
        assert_eq!(lay(&[9; 4], 40, 1), Laid { rows: vec![4], more: 0 });
        assert_eq!(lay(&[9, 9, 9], 40, 1), Laid { rows: vec![3], more: 0 });
        // Two rows.
        assert_eq!(lay(&[9, 9, 9, 9, 9], 35, 1), Laid { rows: vec![3, 2], more: 0 });
        // Three rows, exactly full: no mark, so no room kept for one.
        assert_eq!(lay(&[9; 9], 30, 3), Laid { rows: vec![3, 3, 3], more: 0 });
        // Nobody, no row.
        assert_eq!(lay(&[], 40, 1), Laid { rows: vec![], more: 0 });
    }

    #[test]
    fn names_past_the_third_row_end_it_with_the_cut_mark() {
        // Ten chips of 9 at 30 columns: three a row; the third row gives a
        // chip up for the mark, whichever width it is.
        assert_eq!(lay(&[9; 10], 30, 1), Laid { rows: vec![3, 3, 2], more: 2 });
        assert_eq!(lay(&[9; 10], 30, 3), Laid { rows: vec![3, 3, 2], more: 2 });
        // Room the last row has for the mark is used, not given up: at 32
        // columns three chips and ` …` fit, ` ...` does not.
        assert_eq!(lay(&[9; 10], 32, 1), Laid { rows: vec![3, 3, 3], more: 1 });
        assert_eq!(lay(&[9; 10], 32, 3), Laid { rows: vec![3, 3, 2], more: 2 });
        // A chip wider than a row takes a row of its own.
        assert_eq!(lay(&[50, 5], 40, 1), Laid { rows: vec![1, 1], more: 0 });
    }

    #[test]
    fn the_cut_mark_is_an_ellipsis_in_ascii_where_the_palette_is_not_rich() {
        assert_eq!(cut(ColorDepth::TrueColor), "…");
        assert_eq!(cut(ColorDepth::Ansi16), "...");
    }
}
