//! The tab menu: what a tab's menu offers and what each item does, the
//! question a destructive item waits on, and the compose box lent to a
//! name or a description. Every item runs the slash command that
//! does the same thing ([`keys::run_slash`]); the commands stay.
//!
//! - `≡`, the common menu: Theme ▸ (this session's look), Keybinding set ▸
//!   (presets over `attend.chat.tabs.*`), the mouse at start, Hide column
//!   or Show column (the left column, as `attend.chat.sidebar_key` toggles
//!   it), Settings ▸ (the chat's and the attend mod's settings, each set
//!   where the operator chooses, [`super::settings_view`]), Quit (as
//!   Ctrl-C, asking first over a draft; Esc never quits, ADR-409).
//! - the operator view (`⋯all`, every channel together): Hide messages
//!   here. It is not a channel, so it offers no channel's items (ADR-405).
//! - `#open`: Hide messages here, Delete read messages for everyone…
//!   ([`super::cleanup`]), and Undo delete after a delete there.
//! - a named channel: Add agent ▸ (adds a live peer that is not in it, an
//!   invited one among them, as the agent menu's Add agent to chat does),
//!   Invite agent ▸ (invites a live peer that is neither in it nor invited
//!   there; it joins itself), Remove agent ▸, Describe…, Hide messages here (and Show hidden messages while it has
//!   a mark), Delete read messages for everyone… (and Undo delete), Leave
//!   (Join for a channel the human is not in), Seal, Delete channel.
//! - the `+` slot and Ctrl+N: no menu, straight to the name of a new
//!   channel, typed in an entry dialog docked under `+`. Describe… asks its
//!   line in the same dialog, docked beside its item. The compose box is
//!   for chatting and is never borrowed (ADR-409): its draft stays as it
//!   is. A name the dialog refuses is said inside it, the text kept.
//! - an agent's row in the left column, or its row under a channel: for a
//!   claude first Show DMs, or Hide DMs while its DM view is listed
//!   ([`super::dm`]), then Address @Name (puts `@Name ` in the compose
//!   box), Who is this (what `/whois` says of the session the row is, or
//!   what the chat knows of one the roster lacks, then each invitation it
//!   has not answered, who sent it and when), Add agent to chat ▸ (a
//!   picker, `add to channel`, of the channels the agent is not in; the
//!   pick adds it, as Add agent ▸ does), then for a claude Hide messages here and Delete read messages for
//!   everyone… on its DMs with the operator, then Remove from #channel
//!   when the channel lists the agent, live or quiet: the channel of the
//!   member row the menu was opened from, else the channel shown; and last
//!   Tombstone… for a claude that is not live. The operator's own row
//!   offers Leave #channel in Remove's place: Remove never takes out the
//!   human at the keyboard, here or in the Remove agent picker.
//! - a DM view, by its row or its tab: Hide DMs, Hide messages here,
//!   Delete read messages for everyone…; an agent's own DM view: Hide
//!   review, and Tombstone… for an agent that is not live.
//! - a message in the feed, by Enter on it: Reply (its channel shown and
//!   the keyboard in the compose box; a direct message is answered by
//!   addressing its sender, and one the chat sent by showing its DM view),
//!   Quote (its lines put in the box as `> ` lines), Address @Name (as
//!   the agent menu's), Copy (the message to the terminal's clipboard by
//!   OSC 52).
//!
//! Every item that drops something asks first: Hide messages here, Delete
//! read messages for everyone… (once its age is picked), Leave, Delete
//! channel, Remove and Tombstone…. The question is the shell's
//! ([`agent_tui::Open::Ask`]), on the bottom bar with its two answers as
//! lozenges: `y` or a click on the first goes ahead; `n`, Esc, a click
//! anywhere else or any other key keeps things as they are. A click opens
//! a menu with the first item under the cursor, so an Enter just after the
//! click must not drop anything: Enter only answers no.
//!
//! A menu docks on the line under the row it belongs to: a channel's or an
//! agent's row in the left column, else its tab ([`agent_tui::Anchor`]).
//! Where the lines below cannot hold it, it ends on the line above the row;
//! where neither side can, it sits at the foot of the pane. A menu an item
//! leads to (Theme ▸, Add agent ▸) docks beside that item's row. A
//! picker marks the current value. An add and an invitation go through
//! attend-membership, as `attend channel add` and `/invite` do (ADR-404
//! D1): an add enrols the agent with no answer from it, tells it, and hands
//! it the room's recent history to read; an invitation asks it to join,
//! and its join keeps that history to read.
//!
//! One item waits on attend work and is drawn set back with what it waits
//! on, passed over by the highlight and deaf to a click: Seal (ADR-404 D4),
//! never the first item.

use agent_theme::ColorDepth;
use agent_tui::ratatui::layout::Rect;
use agent_tui::{Anchor, Item, Open, Question};

use crate::chip::Seen;
use crate::groups::Invites;
use crate::peers::Peer;

use super::cleanup::{Count, DeleteAsk, DELETE_READ, HIDE_HERE, SHOW_HIDDEN, TOMBSTONE, UNDO_DELETE};
use super::dm::{Kind, HIDE_DMS, HIDE_REVIEW, REVIEW_DMS, SHOW_DMS};
use crate::attend_cli::Scope;
use super::keys::{self, EnterAction};
use super::ChatPane;
use crate::groups::BASE_CHANNEL_NAME;
use crate::slash::SlashOutcome;
use crate::tabs::{Slot, Tab};

pub(super) const ADD_AGENT: &str = "Add agent ▸";
pub(super) const INVITE_AGENT: &str = "Invite agent ▸";
pub(super) const SEAL: &str = "Seal";
pub(super) const REMOVE_AGENT: &str = "Remove agent ▸";
pub(super) const DESCRIBE: &str = "Describe…";
pub(super) const LEAVE: &str = "Leave";
pub(super) const JOIN: &str = "Join";
pub(super) const DELETE: &str = "Delete channel";
pub(super) const THEME: &str = "Theme ▸";
pub(super) const KEYS: &str = "Keybinding set ▸";
pub(super) const MOUSE_ON: &str = "Mouse on at start";
pub(super) const MOUSE_OFF: &str = "Mouse off at start";
pub(super) const SETTINGS: &str = "Settings ▸";
pub(super) const HIDE_COLUMN: &str = "Hide column";
pub(super) const ADDRESS: &str = "Address";
pub(super) const WHO: &str = "Who is this";
pub(super) const REMOVE_FROM: &str = "Remove from";
pub(super) const SHOW_COLUMN: &str = "Show column";
pub(super) const QUIT: &str = "Quit";
pub(super) const ADD_TO_CHAT: &str = "Add agent to chat ▸";
pub(super) const REPLY: &str = "Reply";
pub(super) const QUOTE: &str = "Quote";
pub(super) const COPY: &str = "Copy";

/// The entry dialogs' ids: a new channel's name, and a channel's line.
const NEW_CHANNEL: &str = "new-channel";
const DESCRIBE_ID: &str = "describe:";
pub(super) const CUSTOM_KEYS: &str = "custom";

/// The keybinding presets: a label, then the `attend.chat.tabs.*` values.
pub(super) const PRESETS: &[(&str, &str, &str)] = &[
    ("ctrl: Ctrl+1-9, F2, Ctrl+T", "ctrl", "both"),
    ("alt: Alt+1-9, F2, Ctrl+T", "alt", "both"),
    ("fallback only: F2, Ctrl+T, Tab", "none", "both"),
    ("auto: Ctrl+1-9 where it arrives, else Alt+1-9", "auto", "both"),
];

/// What the ids of the chat's questions start with
/// ([`agent_tui::Open::Ask`]). Each question has an id of its own, so an
/// answer acts on the question it answers and on no other.
const ASK: &str = "ask:";

/// A step that asks first, waiting for its answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Step {
    /// Hide messages here, on the tab and its label; the bus keeps every
    /// message.
    HideHere(Tab, String),
    /// Leave the channel (`/leave`).
    Leave(String),
    /// Delete read messages for everyone (`attend message delete`): its
    /// question waits for the count its pick started.
    DeleteRead(DeleteAsk),
    /// Tombstone the dead agent of this session id, and its name.
    Tombstone(String, String),
    /// Delete the channel (`/dissolve`).
    Delete(String),
    /// Take a member out of the channel (`/kick`).
    Remove(String, Member),
}

impl Step {
    /// The question under `id`: what it asks, and what yes and no do, as
    /// the answers name them. A question that removes something for
    /// everyone expires `life` after it is ready, as a no.
    fn question(&self, id: String, life: std::time::Duration) -> Question {
        let (text, yes, no) = match self {
            Step::HideHere(_, label) => (format!("hide the messages so far in {label}? nothing is deleted"), "hide", "keep"),
            Step::Leave(c) => (format!("leave #{c}?"), "leave", "stay"),
            Step::DeleteRead(d) => return d.question(id, life),
            Step::Tombstone(_, name) => (format!("tombstone @{name}? its messages and its Agents row go away"), "tombstone", "keep"),
            Step::Delete(c) => (format!("delete #{c} and its history for everyone?"), "delete", "keep"),
            Step::Remove(c, m) => (format!("remove @{} from #{c}?", m.label), "remove", "keep"),
        };
        let q = Question::new(id, text, yes, no);
        match self {
            Step::Tombstone(..) | Step::Delete(_) | Step::Remove(..) => q.expires(agent_tui::Expiry::new(life)),
            _ => q,
        }
    }

    /// What the status line says when its question, which lived `life`,
    /// expired unanswered.
    fn expired(&self, life: std::time::Duration) -> Option<String> {
        let action = match self {
            Step::DeleteRead(_) => "delete".to_string(),
            Step::Tombstone(_, name) => format!("tombstone @{name}"),
            Step::Delete(c) => format!("delete #{c}"),
            Step::Remove(c, m) => format!("remove @{} from #{c}", m.label),
            Step::HideHere(..) | Step::Leave(_) => return None,
        };
        Some(format!("{action} cancelled: no answer in {} s", life.as_secs()))
    }
}

/// A channel member a Remove can take out: its membership id, the name the
/// status line uses, whether it is live, and a claude's root, where the
/// notice of its removal goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Member {
    pub id: String,
    pub label: String,
    pub live: bool,
    pub root: Option<String>,
}

impl Member {
    /// How a picker lists it: a member that is not live says so.
    pub fn option(&self) -> String {
        if self.live {
            self.label.clone()
        } else {
            format!("{} (quiet)", self.label)
        }
    }
}

/// The members `ids` of a channel, in its order, named as the chat names
/// them: a live peer by its roster name, one the chat has seen by the
/// column's name, and any other by its id.
pub(super) fn members(ids: &[String], roster: &[Peer], seen: &[Seen]) -> Vec<Member> {
    ids.iter()
        .map(|id| {
            if let Some(p) = roster.iter().find(|p| p.session_id == *id) {
                return Member { id: id.clone(), label: p.display.clone(), live: true, root: Some(p.root.clone()) };
            }
            match seen.iter().find(|s| s.member == *id) {
                Some(s) => Member {
                    id: id.clone(),
                    label: s.identity.nickname.clone(),
                    live: s.live,
                    root: s.identity.is_claude.then(|| attend_presence::session::normalize_origin(&s.identity.cwd)),
                },
                None => Member { id: id.clone(), label: id.clone(), live: false, root: None },
            }
        })
        .collect()
}

/// The members a Remove can take out: all but the operator, `me`, who
/// leaves with Leave instead.
pub(super) fn removable(members: Vec<Member>, me: &str) -> Vec<Member> {
    members.into_iter().filter(|m| m.id != me).collect()
}

/// The items of an agent row's menu: for an agent that has DM views, its
/// DMs with the operator, then its own DMs (Review DMs), each shown or
/// hidden (`views`: whether the column lists each); address it, who it is,
/// add it to a channel; for an agent with DM views, the cleanup items
/// (`cleanup`: hide, show, delete) on its DMs with the operator; then for the
/// channel that lists it (`listed`: the member row's, else the one shown),
/// Remove, or Leave on the operator's own row; last, Tombstone… for an agent that is not live
/// (`dead`). `add` is why the agent cannot be added to a channel, when it
/// cannot: Add agent to chat is then set back with the reason.
pub(super) fn agent_items(name: &str, views: Option<(bool, bool)>, listed: Option<&str>, operator: bool, add: Result<(), String>, cleanup: Vec<Item>, dead: bool) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    if let Some((dms, review)) = views {
        out.push(Item::new(if dms { HIDE_DMS } else { SHOW_DMS }));
        out.push(Item::new(if review { HIDE_REVIEW } else { REVIEW_DMS }));
    }
    out.extend([Item::new(format!("{ADDRESS} @{name}")), Item::new(WHO)]);
    out.push(match add {
        Ok(()) => Item::new(ADD_TO_CHAT),
        Err(why) => Item::disabled(ADD_TO_CHAT, why),
    });
    if views.is_some() {
        out.extend(cleanup);
    }
    if let Some(c) = listed {
        out.push(Item::new(if operator { format!("{LEAVE} #{c}") } else { format!("{REMOVE_FROM} #{c}") }));
    }
    if views.is_some() && dead {
        out.push(Item::new(TOMBSTONE));
    }
    out
}

/// The channels an agent can be added to: those on disk it is not in,
/// `#open` left out (every agent is already there). A channel it is
/// invited to is offered: the add spends the invitation.
pub(super) fn addable(rows: &[crate::groups::ChannelRow], member: &str) -> Vec<String> {
    rows.iter()
        .filter(|r| r.on_disk && !r.known.is_base && !r.known.membership.members.iter().any(|m| m == member))
        .map(|r| r.known.group.name.clone())
        .collect()
}

/// The items of a tab's menu, by the channel it shows (`None`: merged):
/// a named channel offers Leave to a member and Join to anyone else.
/// `hide` is the tab's hide items ([`ChatPane::hide_items`]): Hide messages
/// here, and Show hidden messages while the tab has a watermark; `delete`
/// is the delete items for the channel,
/// Delete read messages for everyone… and Undo delete after one
/// ([`ChatPane::delete_items`]). Seal
/// waits on attend (ADR-404 D4) and says so; it is not first, so the
/// highlight never starts on it.
pub(super) fn items(channel: Option<&str>, joined: bool, hide: Vec<Item>, delete: Vec<Item>) -> Vec<Item> {
    let membership = if joined { LEAVE } else { JOIN };
    match channel {
        None => hide,
        Some(BASE_CHANNEL_NAME) => hide.into_iter().chain(delete).collect(),
        Some(_) => {
            let mut out: Vec<Item> = vec![ADD_AGENT.into(), INVITE_AGENT.into(), REMOVE_AGENT.into(), DESCRIBE.into()];
            out.extend(hide);
            out.extend(delete);
            out.extend([membership.into(), Item::disabled(SEAL, "needs ADR-404 D4"), DELETE.into()]);
            out
        }
    }
}

/// The items of a message's menu: reply where it was said, quote it,
/// address its sender `name`, copy it. A local notice has no channel to
/// reply in, and a message with no sender the column knows has no one to
/// address. A view with no compose box (`composes` false: an agent's own
/// DMs) has nowhere to quote into.
pub(super) fn message_items(name: Option<&str>, notice: bool, composes: bool) -> Vec<Item> {
    vec![
        if notice { Item::disabled(REPLY, "a notice of the chat's") } else { Item::new(REPLY) },
        if composes { Item::new(QUOTE) } else { Item::disabled(QUOTE, "read only: no compose box here") },
        match name {
            Some(n) => Item::new(format!("{ADDRESS} @{n}")),
            None => Item::disabled(ADDRESS, "no sender to address"),
        },
        Item::new(COPY),
    ]
}

/// The most bytes Copy sends: base64 makes it about 100 kB on the wire,
/// past which many terminals drop an OSC 52 copy whole.
pub(super) const COPY_MAX: usize = 74 * 1024;

/// What Copy sends of `message`: all of it, or its first [`COPY_MAX`] bytes
/// cut at a character's edge, and whether it was cut.
pub(super) fn copy_text(message: &str) -> (String, bool) {
    if message.len() <= COPY_MAX {
        return (message.to_string(), false);
    }
    let end = (0..=COPY_MAX).rev().find(|i| message.is_char_boundary(*i)).unwrap_or(0);
    (message[..end].to_string(), true)
}

/// A message's lines as a quote in the compose box: each led by `> `,
/// then a line to write on.
pub(super) fn quoted(message: &str) -> String {
    message.lines().map(|l| format!("> {l}\n")).collect()
}

/// The labels of `items`, in order.
#[cfg(test)]
fn labels(items: &[Item]) -> Vec<&str> {
    items.iter().map(|i| i.label.as_str()).collect()
}

/// The live peers Add agent ▸ or Invite agent ▸ offers for a channel with
/// `members`, by display name: those not in it, and for an invitation not
/// in `invited` either. Add passes no invitations: it offers an invited
/// peer, and spends the invitation.
pub(super) fn outside(roster: &[Peer], members: &[String], invited: &[String]) -> Vec<String> {
    roster.iter().filter(|p| !members.contains(&p.session_id) && !invited.contains(&p.session_id)).map(|p| p.display.clone()).collect()
}

/// `member`'s pending invitations as Who is this says them, by channel:
/// `invited to #infra by aaron 4m ago`, the inviter named as the column
/// names it, and either part left out when the invitation does not record
/// it. Empty when it has none.
pub(super) fn invitations_line(member: &str, invites: &Invites, seen: &[Seen], now: std::time::SystemTime) -> String {
    let mut mine: Vec<&attend_groups::Invite> = invites.values().flatten().filter(|i| i.member == member).collect();
    mine.sort_by(|a, b| a.channel.cmp(&b.channel));
    let name = |id: &str| seen.iter().find(|s| s.member == id).map_or(id.to_string(), |s| s.identity.nickname.clone());
    let said: Vec<String> = mine
        .iter()
        .map(|i| {
            let by = i.by.as_deref().map(|b| format!(" by {}", name(b))).unwrap_or_default();
            let ago = i.at.and_then(|t| now.duration_since(t).ok()).map(|d| format!(" {} ago", super::sidebar::age_label(d))).unwrap_or_default();
            format!("invited to #{}{by}{ago}", i.channel)
        })
        .collect();
    said.join(" · ")
}

/// What Who is this says of `s`, its pending invitations `invited`
/// ([`invitations_line`]) first after its name, where the status line
/// shows them however long its root is. A live claude the roster holds
/// under its session id is told as `/whois` tells it; the id, not the
/// name, finds it, since twins share a name and `/whois @name` would pick
/// one of them. A live claude with no sessions record, a quiet one and a
/// human are told from what the chat itself has seen; a quiet claude with
/// how long ago its last heartbeat was, `beat`, when it left one.
pub(super) fn who_line(s: &Seen, roster: &[Peer], invited: &str, beat: Option<std::time::Duration>) -> String {
    let (name, rest) = if !s.identity.is_claude {
        (s.identity.nickname.clone(), format!("a human · from {} · {}", s.identity.cwd, if s.live { "here" } else { "away" }))
    } else if let Some(p) = roster.iter().find(|p| s.live && p.session_id == s.member) {
        (p.display.clone(), format!("session {} · root {}", p.session_id, p.root))
    } else {
        let how = match (s.live, beat) {
            (true, _) => "live: no sessions record".to_string(),
            (false, Some(ago)) => format!("quiet: last heartbeat {} ago", super::sidebar::age_label(ago)),
            (false, None) => "quiet: no heartbeat".to_string(),
        };
        (s.identity.nickname.clone(), format!("session {} · root {} · {how}", s.member, attend_presence::session::normalize_origin(&s.identity.cwd)))
    };
    let invited = if invited.is_empty() { String::new() } else { format!("{invited} · ") };
    format!("@{name} — {invited}{rest}")
}

impl ChatPane {
    /// The menu of `channel`, or merged's for `None`: from its tab, or its
    /// row in the left column, which lists channels that have no tab. It
    /// docks under `at` when given (the destination flag), else under the
    /// channel's row in the column, else its tab.
    pub(super) fn channel_menu(&mut self, channel: Option<String>, at: Option<Rect>) {
        self.dirty = true;
        let title = channel.as_ref().map_or(super::view::VIEW_LABEL.to_string(), |c| format!("#{c}"));
        let id = format!("menu:{}", channel.as_deref().unwrap_or(""));
        let me = crate::signal::human_member_id();
        let joined = channel.as_deref().is_some_and(|c| self.world().rows.iter().any(|r| r.known.group.name == c && r.known.membership.members.contains(&me)));
        // Under the channel's row in the column, else under its tab.
        let row = at.or_else(|| channel.clone().and_then(|c| self.side_row(&super::sidebar::SideHit::Channel(c))));
        let tab = channel.clone().map_or(Tab::Merged, Tab::Channel);
        let anchor = match row {
            Some(r) => Anchor::Row(r),
            None => self.slot_of(&tab).map_or(Anchor::Center, Anchor::Tab),
        };
        let delete = self.delete_items(&Scope::channel(channel.as_deref().unwrap_or(BASE_CHANNEL_NAME)));
        let hide = self.hide_items(&tab);
        self.to_open = Some(Open::Pick { id, title, items: items(channel.as_deref(), joined, hide, delete), multi: false, anchor });
    }

    pub(super) fn common_menu(&mut self) {
        let mouse = if self.config.mouse { MOUSE_OFF } else { MOUSE_ON };
        let column = if self.sidebar { HIDE_COLUMN } else { SHOW_COLUMN };
        let mut items: Vec<Item> = self.stop_item().map(Item::new).into_iter().collect();
        items.extend([THEME, KEYS, mouse, column, SETTINGS, QUIT].into_iter().map(Item::from));
        self.to_open = Some(Open::Pick { id: "common".into(), title: "attend-chat".into(), items, multi: false, anchor: Anchor::Tab(0) });
    }

    /// The keybinding preset the chat's settings match, if any.
    fn preset(&self) -> Option<&'static str> {
        PRESETS.iter().find(|(_, jump, focus)| self.config.jump == *jump && self.config.focus_key == *focus).map(|(l, _, _)| *l)
    }

    /// An item of the `≡` menu.
    fn common_item(&mut self, item: &str) {
        match item {
            THEME => {
                let set = agent_theme::ThemeSet::load(agent_theme::user_dir().as_deref());
                let mut names = vec!["terminal".to_string()];
                names.extend(set.list().map(|(t, _)| t.name.clone()));
                let items = names.into_iter().map(|n| Item::new(n.clone()).current(n == self.theme)).collect();
                self.to_open = Some(Open::Pick { id: "theme".into(), title: "theme for this session".into(), items, multi: false, anchor: Anchor::Parent });
            }
            KEYS => {
                let current = self.preset();
                // Keys no preset matches are named on a first row, current
                // and not pickable, so nothing below reads as what is in use
                // and an Enter at once replaces nothing.
                let mut items: Vec<Item> = Vec::new();
                if current.is_none() {
                    items.push(Item::disabled(CUSTOM_KEYS, format!("jump={}, focus={}", self.config.jump, self.config.focus_key)).current(true));
                }
                items.extend(PRESETS.iter().map(|(l, _, _)| Item::new(*l).current(current == Some(*l))));
                self.to_open = Some(Open::Pick { id: "keys".into(), title: "keybinding set".into(), items, multi: false, anchor: Anchor::Parent });
            }
            MOUSE_ON | MOUSE_OFF => {
                let on = item == MOUSE_ON;
                self.run("config", SlashOutcome::Config { key: Some("mouse".into()), value: Some(on.to_string()) });
            }
            HIDE_COLUMN | SHOW_COLUMN => self.toggle_sidebar(),
            SETTINGS => self.settings_list(Anchor::Parent),
            // The shell's quit, as Ctrl-C: at once, or the exit guard over
            // a draft.
            QUIT => self.to_open = Some(Open::Quit),
            _ if item.starts_with(super::cleanup::STOP) => self.stop_removing(),
            _ => {}
        }
    }

    /// The session's look: `terminal` is the terminal's own palette. Kept
    /// for this session; the stored choice is ways' `theme.active`, which
    /// the chat reads and does not write (ADR-504).
    fn theme_picked(&mut self, name: &str) {
        match self.use_theme((name != "terminal").then_some(name)) {
            Some(w) => self.say(format!("theme: {w}"), true),
            None => self.say(format!("theme {name} for this session; `ways settings set theme.active {name}` keeps it"), false),
        }
    }

    /// Draw with the theme `active` names (`None`: the terminal's own
    /// palette) at the chat's depth, and remember the theme in use for the
    /// Theme menu to mark: the name kept is what the screen shows. A theme
    /// is drawn only where its roles are, so a theme that did not load (a
    /// warning returned), one below 256 colours, or any where no theme
    /// directory exists, keeps `terminal`; a rejected file shadowing a
    /// bundled theme keeps the name, the bundled one being drawn.
    pub(super) fn use_theme(&mut self, active: Option<&str>) -> Option<String> {
        let (palette, warning) = crate::theme::palette(active, self.palette.depth());
        let drawn = palette.roles().is_some();
        self.theme = active.filter(|_| drawn).unwrap_or("terminal").to_string();
        self.palette = palette;
        self.invalidate();
        warning
    }

    /// A menu item, or a submenu's peer, was chosen; or a question was
    /// answered.
    pub(super) fn menu_picked(&mut self, id: &str, value: &str) {
        self.dirty = true;
        match id {
            _ if id.starts_with(ASK) => return self.answered(id, value == "y"),
            "common" => return self.common_item(value),
            "theme" => return self.theme_picked(value),
            "keys" => {
                if let Some((_, jump, focus)) = PRESETS.iter().find(|(l, _, _)| *l == value) {
                    if self.dry_run {
                        return self.say("dry run: /config not run", false);
                    }
                    let dir = std::env::current_dir().unwrap_or_default();
                    let pairs = [("tabs.jump", *jump), ("tabs.focus_key", *focus)];
                    match crate::settings::set_many(&pairs, &attend_config::user_path(), &dir) {
                        Ok(_) => {
                            self.reload_settings();
                            let said = format!("keybinding set: {value}{}", self.jump_warning());
                            self.say(said, false);
                        }
                        Err(e) => self.say(e, true),
                    }
                }
                return;
            }
            _ => {}
        }
        if self.setting_picked(id, value) {
            return;
        }
        if let Some(c) = id.strip_prefix("menu:") {
            return self.item(c, value);
        }
        if let Some(member) = id.strip_prefix("agent:") {
            return self.agent_item(member, value);
        }
        if self.view_item(id, value) {
            return;
        }
        if let Some(m) = id.strip_prefix("message:") {
            return self.message_item(m, value);
        }
        if self.older_item(id, value) {
            return;
        }
        if id == NEW_CHANNEL {
            return self.new_channel_named(value);
        }
        if let Some(c) = id.strip_prefix(DESCRIBE_ID) {
            return self.described(c, value);
        }
        if let Some(member) = id.strip_prefix("addto:") {
            return self.add_to(member, value.trim_start_matches('#'));
        }
        // A peer from Add agent, Invite agent or Remove agent: the label's
        // first word is the display name the commands resolve.
        let peer = value.split_whitespace().next().unwrap_or(value).to_string();
        if let Some(c) = id.strip_prefix("add:") {
            self.add_peer(c, |p| p.display == peer);
        } else if let Some(c) = id.strip_prefix("invite:") {
            let invited = self.run("invite", SlashOutcome::Invite { member: peer, channel: Some(c.into()) });
            if invited && !self.dry_run {
                let said = format!("{} — they join themselves", self.status);
                self.say(said, false);
            }
        } else if let Some(c) = id.strip_prefix("remove:") {
            if let Some(m) = self.removable.iter().find(|m| m.option() == value).cloned() {
                self.ask(Step::Remove(c.into(), m));
            }
        }
    }

    /// The column's agent whose membership id is `member`.
    fn seen_member(&mut self, member: &str) -> Option<Seen> {
        self.world().seen.iter().find(|s| s.member == member).cloned()
    }

    /// The menu of an agent's row in the left column, the agent named by
    /// its membership id: two agents can share a name, never an id. It
    /// docks under `at` when given (a sender's name), else under the row.
    /// Remove (or Leave) is for `channel`, the channel of the member row
    /// the menu was opened from, else for the channel shown.
    pub(super) fn agent_menu(&mut self, member: String, channel: Option<String>, at: Option<Rect>) {
        self.dirty = true;
        let Some(s) = self.seen_member(&member) else {
            return self.say("that agent is not in the column any more", true);
        };
        let name = s.identity.nickname.clone();
        let me = crate::signal::human_member_id();
        let channel = channel.or_else(|| match self.normal_tab() {
            Tab::Channel(c) => Some(c),
            Tab::Merged | Tab::Dm(_) | Tab::Review(_) => None,
        });
        let listed = channel.and_then(|c| self.channel_members(&c).into_iter().find(|m| m.id == member).map(|m| (c, m)));
        let add = self.can_add(&s);
        let views = self.has_dms(&member).then(|| (self.lists(Kind::Dm, &member), self.lists(Kind::Review, &member)));
        let mut cleanup = self.hide_items(&Tab::Dm(member.clone()));
        cleanup.extend(self.delete_items(&Scope::Dm(member.clone())));
        let dead = self.is_dead_claude(&member);
        let items = agent_items(&name, views, listed.as_ref().map(|(c, _)| c.as_str()), member == me, add, cleanup, dead);
        self.removable = removable(listed.map(|(_, m)| m).into_iter().collect(), &me);
        let anchor = at.or_else(|| self.side_row(&super::sidebar::SideHit::Agent(member.clone()))).map_or(Anchor::Center, Anchor::Row);
        self.to_open = Some(Open::Pick { id: format!("agent:{member}"), title: format!("@{name}"), items, multi: false, anchor });
    }

    /// The feed's message with the id `id`, while the feed holds it, and the
    /// nickname of its sender, as the column knows it. A message is found by
    /// what it is: the feed drops its oldest past [`super::MAX_SIGNALS`], and
    /// an index would then name its neighbour.
    fn feed_message(&mut self, id: &str) -> Option<(crate::signal::Signal, Option<String>)> {
        let shown = self.normal_tab();
        let (i, sig) = self.in_view(&shown).enumerate().find(|(_, s)| s.id == id)?;
        let sig = sig.clone();
        let member = self.entry_members.get(i).cloned().flatten();
        let name = member.and_then(|m| self.seen_member(&m)).map(|s| s.identity.nickname);
        Some((sig, name))
    }

    /// Enter on the feed: the selected message's menu, docked under its
    /// header. With no message selected, the newest is selected first.
    pub(super) fn message_menu(&mut self) {
        let n = self.feed.len();
        let Some(i) = self.feed.selected.or(n.checked_sub(1)) else {
            return self.say("no message to act on", false);
        };
        self.feed.reveal(i);
        let shown = self.normal_tab();
        let Some(id) = self.in_view(&shown).nth(i).map(|s| s.id.clone()) else { return };
        let Some((sig, name)) = self.feed_message(&id) else { return };
        let feed = self.hits.feed;
        let anchor = self.feed.header_row(i).map_or(Anchor::Center, |r| Anchor::Row(Rect { x: feed.x, y: feed.y + r, width: feed.width, height: 1 }));
        let title = name.as_ref().map_or("message".to_string(), |n| format!("message from @{n}"));
        let items = message_items(name.as_deref(), sig.channel == crate::signal::Channel::Info, shown.composes());
        self.to_open = Some(Open::Pick { id: format!("message:{}", sig.id), title, items, multi: false, anchor });
    }

    /// An item of the menu of the message with the id `id`. A message the
    /// feed has dropped since the menu opened is acted on no more.
    fn message_item(&mut self, id: &str, item: &str) {
        let Some((sig, name)) = self.feed_message(id) else {
            return self.say("that message is not in the feed any more", true);
        };
        let sender = sig.sender_member();
        let address = |c: &mut ChatPane, n: &str| {
            c.show_a_box(sender.as_deref());
            c.complete(super::Chip::Agent(n.to_string()))
        };
        match item {
            REPLY => match (&sig.channel, name.as_deref()) {
                (crate::signal::Channel::Group(g), _) => self.set_tab(Tab::Channel(g.clone())),
                (crate::signal::Channel::Open, _) => self.set_tab(Tab::Channel(BASE_CHANNEL_NAME.into())),
                (crate::signal::Channel::Direct(_) | crate::signal::Channel::Person | crate::signal::Channel::Elsewhere(_), Some(n)) => address(self, n),
                (crate::signal::Channel::Sent(to), _) => match to.split(',').find(|m| self.has_dms(m)).map(str::to_string) {
                    Some(m) => self.open_dm(m),
                    None => return self.say("no DM view to reply in", true),
                },
                _ => return self.say("no channel to reply in", true),
            },
            QUOTE => {
                self.input.insert_str(&quoted(&sig.message));
                self.tab_cycle = None;
            }
            COPY => {
                let (text, cut) = copy_text(&sig.message);
                self.to_open = Some(Open::Copy { text });
                let said = if cut { format!("copied the first {} kB of {} kB to the clipboard, where the terminal allows OSC 52", COPY_MAX / 1024, sig.message.len().div_ceil(1024)) } else { "copied to the clipboard, where the terminal allows OSC 52".to_string() };
                return self.say(said, false);
            }
            _ if item.starts_with(ADDRESS) => match name {
                Some(n) => address(self, &n),
                None => return,
            },
            _ => return,
        }
        // What the item put in the box, or where it points the reply, is
        // written next: the keyboard goes to the box.
        self.dispatch(super::ix::Ev::Compose);
    }

    /// Before an item writes into the compose box: in a view with none, an
    /// agent's own DMs, show the operator's DMs with `member` when it has
    /// them, else the operator view, so the text lands in a box shown.
    fn show_a_box(&mut self, member: Option<&str>) {
        if self.foreground.composes() {
            return;
        }
        match member.filter(|m| self.has_dms(m)) {
            Some(m) => self.open_dm(m.to_string()),
            None => self.set_tab(Tab::Merged),
        }
    }

    fn agent_item(&mut self, member: &str, item: &str) {
        if item == SHOW_DMS {
            self.open_view(Kind::Dm, member.to_string());
        } else if item == HIDE_DMS {
            self.hide_view(Kind::Dm, member);
        } else if item == REVIEW_DMS {
            self.open_view(Kind::Review, member.to_string());
        } else if item == HIDE_REVIEW {
            self.hide_view(Kind::Review, member);
        } else if item.starts_with(ADDRESS) {
            match self.seen_member(member) {
                Some(s) => {
                    self.show_a_box(Some(member));
                    self.complete(super::Chip::Agent(s.identity.nickname))
                }
                None => self.say("that agent is not in the column any more", true),
            }
        } else if item == HIDE_HERE {
            self.ask_hide(Tab::Dm(member.to_string()));
        } else if item == SHOW_HIDDEN {
            self.show_hidden(&Tab::Dm(member.to_string()));
        } else if item == DELETE_READ {
            self.delete_menu(&Scope::Dm(member.to_string()));
        } else if item == UNDO_DELETE {
            self.undo_delete();
        } else if item == TOMBSTONE {
            self.ask_tombstone(member);
        } else if item == WHO {
            self.whois(member);
        } else if item == ADD_TO_CHAT {
            self.add_menu(member);
        } else if let Some(c) = item.strip_prefix(REMOVE_FROM).map(|c| c.trim().trim_start_matches('#').to_string()) {
            if let Some(m) = self.removable.first().cloned() {
                self.ask(Step::Remove(c, m));
            }
        } else if let Some(c) = item.strip_prefix(LEAVE).map(|c| c.trim().trim_start_matches('#').to_string()) {
            self.ask(Step::Leave(c));
        }
    }

    /// Who an agent of the column is ([`who_line`]): the roster is read
    /// only for a live claude, the one it can hold.
    fn whois(&mut self, member: &str) {
        let Some(s) = self.seen_member(member) else {
            return self.say("that agent is not in the column any more", true);
        };
        let roster = if s.identity.is_claude && s.live { crate::peers::roster(ColorDepth::detect(), &attend_instances::SnapshotCache::new()) } else { Vec::new() };
        let now = self.now();
        let w = self.world();
        let invited = invitations_line(member, &w.invites, &w.seen, now);
        let beat = attend_presence::heartbeat::last_seen(member).map(|t| now.duration_since(t).unwrap_or_default());
        self.say(who_line(&s, &roster, &invited, beat), false);
    }

    /// The members of `channel`, from its membership: live or not, the
    /// operator among them.
    fn channel_members(&mut self, channel: &str) -> Vec<Member> {
        let ids = attend_groups::load_groups(&crate::signal::signals_base()).get(channel).map(|e| e.members.clone()).unwrap_or_default();
        let roster = crate::peers::roster(ColorDepth::detect(), &attend_instances::SnapshotCache::new());
        members(&ids, &roster, &self.world().seen)
    }

    fn item(&mut self, channel: &str, item: &str) {
        let chan = || (!channel.is_empty()).then(|| channel.to_string());
        match item {
            HIDE_HERE => self.ask_hide(chan().map_or(Tab::Merged, Tab::Channel)),
            SHOW_HIDDEN => self.show_hidden(&chan().map_or(Tab::Merged, Tab::Channel)),
            DELETE_READ => self.delete_menu(&Scope::channel(chan().as_deref().unwrap_or(BASE_CHANNEL_NAME))),
            UNDO_DELETE => self.undo_delete(),
            DELETE => {
                if let Some(c) = chan() {
                    self.ask(Step::Delete(c));
                }
            }
            LEAVE => {
                if let Some(c) = chan() {
                    self.ask(Step::Leave(c));
                }
            }
            JOIN => {
                if let Some(c) = chan() {
                    self.run("join", SlashOutcome::Join(c));
                }
            }
            DESCRIBE => self.describe(channel),
            ADD_AGENT | INVITE_AGENT | REMOVE_AGENT => self.peer_menu(channel, item),
            _ => {}
        }
    }

    /// The picker of the channel menu's `item`: Add agent ▸ and Invite
    /// agent ▸ list the live peers [`outside`] `channel`; Remove agent ▸
    /// every member of it but the operator, live or quiet: a quiet member is
    /// removed by its membership id; the operator leaves with Leave.
    fn peer_menu(&mut self, channel: &str, item: &str) {
        let options: Vec<String> = if item == REMOVE_AGENT {
            let members = self.channel_members(channel);
            self.removable = removable(members, &crate::signal::human_member_id());
            self.removable.iter().map(Member::option).collect()
        } else {
            let base = crate::signal::signals_base();
            let members = attend_groups::load_groups(&base).get(channel).map(|e| e.members.clone()).unwrap_or_default();
            let invited: Vec<String> = match item {
                INVITE_AGENT => attend_groups::load_invites(&base).remove(channel).unwrap_or_default().into_iter().map(|i| i.member).collect(),
                _ => Vec::new(),
            };
            let roster = crate::peers::roster(ColorDepth::detect(), &attend_instances::SnapshotCache::new());
            outside(&roster, &members, &invited)
        };
        let (id, title, none) = match item {
            ADD_AGENT => ("add", "add to", "no live agent outside"),
            INVITE_AGENT => ("invite", "invite to", "no live agent to invite to"),
            _ => ("remove", "remove from", "no member in"),
        };
        if options.is_empty() {
            return self.say(format!("{none} #{channel}"), true);
        }
        let items = options.into_iter().map(Item::from).collect();
        self.to_open = Some(Open::Pick { id: format!("{id}:{channel}"), title: format!("{title} #{channel}"), items, multi: false, anchor: Anchor::Parent });
    }

    /// Whether `s` can be added to a channel from its menu, and if not why:
    /// an add tells the agent in its project's tray, so it needs a live
    /// claude the roster holds, and a channel it is not in.
    fn can_add(&mut self, s: &Seen) -> Result<(), String> {
        if !s.identity.is_claude {
            return Err("a human joins with /join".into());
        }
        if !s.live {
            return Err("quiet: no live session to add".into());
        }
        if addable(&self.world().rows, &s.member).is_empty() {
            return Err("already in every channel".into());
        }
        Ok(())
    }

    /// Add agent to chat: a picker of the channels the agent is not in,
    /// docked beside the item.
    fn add_menu(&mut self, member: &str) {
        let channels = addable(&self.world().rows, member);
        if channels.is_empty() {
            return self.say("already in every channel", true);
        }
        let items = channels.into_iter().map(|c| Item::new(format!("#{c}"))).collect();
        // Under the row its menu was opened from, the agent's own or its row
        // under a channel, which the keyboard is on after the click; beside
        // the closed menu only when the row is not drawn.
        let from = match self.ix.row_hit() {
            Some(hit @ super::sidebar::SideHit::Member(_, m)) if m == member => hit.clone(),
            _ => super::sidebar::SideHit::Agent(member.to_string()),
        };
        let anchor = self.side_row(&from).map_or(Anchor::Parent, Anchor::Row);
        self.to_open = Some(Open::Pick { id: format!("addto:{member}"), title: "add to channel".into(), items, multi: false, anchor });
    }

    /// The channel picked for an agent: it is added there, by its session,
    /// as Add agent ▸ adds.
    fn add_to(&mut self, member: &str, channel: &str) {
        self.add_peer(channel, |p| p.session_id == member);
    }

    /// Add the live peer `which` finds to `channel` ([`keys::add_peer`]).
    /// A dry run (`--snap`) adds nothing.
    fn add_peer(&mut self, channel: &str, which: impl Fn(&Peer) -> bool) {
        if self.dry_run {
            return self.say("dry run: nothing added", false);
        }
        let roster = crate::peers::roster(ColorDepth::detect(), &attend_instances::SnapshotCache::new());
        let Some(peer) = roster.into_iter().find(which) else {
            return self.say("that agent has no live session to add", true);
        };
        let action = keys::add_peer(channel, &peer);
        self.apply(action);
    }

    /// Where the new channel's dialog docks: under the `+` slot.
    fn plus_anchor(&mut self) -> Anchor {
        self.bar().iter().position(|s| *s == Slot::New).map_or(Anchor::Center, Anchor::Tab)
    }

    /// Ask for a new channel's name, from the `+` slot or Ctrl+N, in an
    /// entry dialog docked under `+`, opened on `value` with what was wrong
    /// with it, if anything. The compose box and its draft are left as they
    /// are.
    pub(super) fn new_channel(&mut self) {
        self.ask_name(String::new(), None);
    }

    fn ask_name(&mut self, value: String, error: Option<String>) {
        self.dirty = true;
        let anchor = self.plus_anchor();
        self.to_open = Some(Open::Entry { id: NEW_CHANNEL.into(), title: "new channel name".into(), value, error, anchor });
    }

    /// The name entered for a new channel: created and shown, or the dialog
    /// again with the name and what is wrong with it.
    fn new_channel_named(&mut self, text: &str) {
        let name = text.trim().trim_start_matches('#').to_string();
        if let Err(e) = attend_groups::validate_group_name(&name) {
            return self.ask_name(text.to_string(), Some(e.to_string()));
        }
        if !self.run("channels create", SlashOutcome::CreateChannel { name: name.clone(), description: None }) {
            let why = self.status.clone();
            return self.ask_name(text.to_string(), Some(why));
        }
        if !self.dry_run {
            self.stale();
            self.set_tab(Tab::Channel(name));
        }
    }

    /// Describe…: the channel's line, in an entry dialog beside the item,
    /// opened on the line it has now.
    fn describe(&mut self, channel: &str) {
        let now = self.world().groups.iter().find(|k| k.group.name == channel).and_then(|k| k.membership.description.clone()).unwrap_or_default();
        self.to_open = Some(Open::Entry { id: format!("{DESCRIBE_ID}{channel}"), title: format!("describe #{channel}"), value: now, error: None, anchor: Anchor::Parent });
    }

    /// The line entered for `channel`: set, or cleared when empty.
    fn described(&mut self, channel: &str, text: &str) {
        self.run("channels describe", SlashOutcome::DescribeChannel { name: channel.into(), description: text.trim().to_string() });
    }

    /// What to add to a status line when the jump setting asks for Ctrl+digits
    /// in a terminal that does not report them.
    pub(super) fn jump_warning(&self) -> &'static str {
        if self.enhanced || !matches!(self.config.jump.as_str(), "ctrl" | "both") {
            return "";
        }
        if self.config.jump == "both" {
            " · Ctrl+digits do not arrive in this terminal; Alt+1-9 and F2 do"
        } else {
            " · Ctrl+digits do not arrive in this terminal; F2 reaches the tabs"
        }
    }

    /// Ask before `step` runs: the shell shows the question and its answers
    /// ([`agent_tui::Open::Ask`]) under an id of the question's own, and the
    /// answer comes back through [`ChatPane::answered`].
    /// Its id comes back, for work the question waits on.
    pub(super) fn ask(&mut self, step: Step) -> String {
        self.asks += 1;
        let id = format!("{ASK}{}", self.asks);
        self.to_open = Some(Open::Ask(step.question(id.clone(), self.delete_question_life)));
        self.asked = Some((id.clone(), step));
        id
    }

    /// The answer to the question `id`: yes runs its step, no keeps things
    /// as they are and says so. An answer to any question but the one
    /// pending does nothing.
    pub(super) fn answered(&mut self, id: &str, yes: bool) {
        let Some(step) = self.asked.take_if(|(pending, _)| pending == id).map(|(_, step)| step) else { return };
        self.dirty = true;
        if !yes {
            // No to a delete's question stops its count if it still runs.
            self.stop_count(id);
            // Dismissed, a refusal stays on the status line as the error it
            // is, and a count of nothing as what it found.
            return match step {
                Step::DeleteRead(DeleteAsk { count: Count::Refused(why), .. }) => self.say(why, true),
                Step::DeleteRead(DeleteAsk { count: Count::Nothing(said), .. }) => self.say(said, false),
                _ => self.say("kept", false),
            };
        }
        match step {
            Step::HideHere(tab, _) => self.hide_here(&tab),
            Step::Leave(c) => {
                self.run("leave", SlashOutcome::Leave(Some(c)));
            }
            // The shell lets yes through only once the count is shown.
            Step::DeleteRead(d) => {
                if let Some(asked) = d.counted() {
                    self.run_delete(asked);
                }
            }
            Step::Tombstone(member, name) => self.tombstone(&member, &name),
            Step::Delete(c) => {
                self.run("dissolve", SlashOutcome::Dissolve(Some(c)));
            }
            Step::Remove(c, m) => {
                if self.dry_run {
                    self.say("dry run: /kick not run", false);
                } else {
                    let action = keys::kick_member(&c, &m.id, &m.label, m.root.as_deref(), &mut self.releases);
                    self.apply(action);
                }
            }
        }
    }

    /// The question `id` expired unanswered ([`agent_tui::Expiry`]): the
    /// shell closed it as a no, nothing ran, and the status line says so.
    pub(super) fn answered_expired(&mut self, id: &str) {
        let Some(step) = self.asked.take_if(|(pending, _)| pending == id).map(|(_, step)| step) else { return };
        self.dirty = true;
        if let Some(said) = step.expired(self.delete_question_life) {
            self.say(said, false);
        }
    }

    /// The shell could not open what the chat asked for, something else
    /// being open: a question dropped so will never be answered, so it is
    /// let go, and a count it waited on is stopped.
    pub(super) fn question_dropped(&mut self, open: Open) {
        let Open::Ask(Question { id, .. }) = open else { return };
        if self.asked.take_if(|(pending, _)| *pending == id).is_some() {
            self.dirty = true;
            self.stop_count(&id);
        }
    }

    /// Run a slash command for a menu item and show what it did, the
    /// compose box left as it was. A dry run (`--snap`) runs nothing.
    /// True when it succeeded.
    fn run(&mut self, name: &str, outcome: SlashOutcome) -> bool {
        if self.dry_run {
            self.say(format!("dry run: /{name} not run"), false);
            return true;
        }
        let fg = self.normal_tab();
        let action = keys::run_slash(outcome, &fg, &mut self.releases);
        self.apply(action)
    }

    /// Show what a command did, and read the settings and the world again.
    /// True when it succeeded.
    fn apply(&mut self, action: EnterAction) -> bool {
        let ok = match action {
            EnterAction::None => true,
            EnterAction::ClearWithStatus(s) => {
                self.say(s, false);
                true
            }
            EnterAction::ClearWithStatusAndEcho { status, echo } => {
                self.say(status, false);
                for s in echo {
                    self.push(s);
                }
                true
            }
            EnterAction::StatusOnly(s) => {
                self.say(s, true);
                false
            }
            // A menu's command sends nothing, so it makes no send-now offer.
            EnterAction::Quiet(name) => {
                self.say(format!("@{name} is quiet (no heartbeat)"), true);
                false
            }
            EnterAction::ClearWithStatusAndFocus { status, focus } => {
                self.say(status, false);
                self.set_tab(focus);
                true
            }
            EnterAction::HideHere => {
                let fg = self.normal_tab();
                self.ask_hide(fg);
                true
            }
            EnterAction::DeleteRead(c) => {
                self.delete_read(Scope::channel(&c), "all");
                true
            }
        };
        self.reload_settings();
        self.stale();
        ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Copy sends at most COPY_MAX bytes, cut where a character ends, and
    /// says when it cut.
    #[test]
    fn copy_is_capped_at_a_characters_edge() {
        assert_eq!(copy_text("short"), ("short".to_string(), false));
        let long = "é".repeat(COPY_MAX);
        let (text, cut) = copy_text(&long);
        assert!(cut && text.len() <= COPY_MAX && text.len() > COPY_MAX - 2, "{}", text.len());
        assert!(text.chars().all(|c| c == 'é'), "no character split");
        let exact = "x".repeat(COPY_MAX);
        assert_eq!(copy_text(&exact), (exact.clone(), false), "exactly the cap is whole");
    }

    #[test]
    fn each_tab_offers_what_its_channel_allows() {
        let del = || vec![Item::new(DELETE_READ)];
        let hide = || vec![Item::new(HIDE_HERE)];
        assert_eq!(labels(&items(None, false, hide(), del())), [HIDE_HERE], "the operator view hides, and deletes nothing for everyone");
        assert_eq!(labels(&items(Some("open"), true, hide(), del())), [HIDE_HERE, DELETE_READ], "the harmless item first");
        assert_eq!(labels(&items(Some("deploy"), true, hide(), del())), [ADD_AGENT, INVITE_AGENT, REMOVE_AGENT, DESCRIBE, HIDE_HERE, DELETE_READ, LEAVE, SEAL, DELETE]);
        assert_eq!(labels(&items(Some("deploy"), false, hide(), del())), [ADD_AGENT, INVITE_AGENT, REMOVE_AGENT, DESCRIBE, HIDE_HERE, DELETE_READ, JOIN, SEAL, DELETE], "Join to a non-member");
        assert_eq!(labels(&items(Some("open"), true, hide(), vec![Item::new(DELETE_READ), Item::new(UNDO_DELETE)])), [HIDE_HERE, DELETE_READ, UNDO_DELETE], "Undo delete after a delete");
        assert_eq!(labels(&items(Some("deploy"), true, vec![Item::new(HIDE_HERE), Item::new(SHOW_HIDDEN)], del())), [ADD_AGENT, INVITE_AGENT, REMOVE_AGENT, DESCRIBE, HIDE_HERE, SHOW_HIDDEN, DELETE_READ, LEAVE, SEAL, DELETE], "a hidden channel shows again");
        let deploy = items(Some("deploy"), true, hide(), del());
        let waiting: Vec<(&str, Option<&str>)> = deploy.iter().filter(|i| !i.enabled).map(|i| (i.label.as_str(), i.reason.as_deref())).collect();
        assert_eq!(waiting, [(SEAL, Some("needs ADR-404 D4"))], "what waits on attend says so; Add agent adds and Invite agent invites");
    }

    #[test]
    fn add_offers_every_live_peer_outside_and_invite_leaves_the_invited_out() {
        let peer = |id: &str| Peer { session_id: id.into(), root: format!("/work/{id}"), display: format!("P{id}") };
        let roster = [peer("s1"), peer("s2"), peer("s3")];
        let (members, invited) = (vec!["s1".to_string()], vec!["s2".to_string()]);
        assert_eq!(outside(&roster, &members, &[]), ["Ps2", "Ps3"], "Add: the invited s2 among them, its invitation spent by the add");
        assert_eq!(outside(&roster, &members, &invited), ["Ps3"], "Invite: neither a member nor one invited already");
    }

    #[test]
    fn add_runs_the_add_and_a_dry_run_writes_nothing() {
        let mut c = ChatPane::new(None, Default::default());
        c.dry_run = true;
        c.menu_picked("add:deploy", "Ada");
        assert_eq!(c.status, "dry run: nothing added", "Add agent ▸'s pick");
        c.status.clear();
        c.menu_picked("addto:s1", "#deploy");
        assert_eq!(c.status, "dry run: nothing added", "Add agent to chat ▸'s pick");
        c.status.clear();
        c.menu_picked("invite:deploy", "Ada");
        assert_eq!(c.status, "dry run: /invite not run", "Invite agent ▸'s pick still invites");
    }

    #[test]
    fn who_is_this_names_each_invitation_its_inviter_and_its_age() {
        let now = std::time::UNIX_EPOCH + std::time::Duration::from_secs(10_000);
        let inv = |c: &str, by: Option<&str>, ago: Option<u64>| attend_groups::Invite {
            channel: c.into(),
            member: "s2".into(),
            by: by.map(str::to_string),
            at: ago.map(|a| now - std::time::Duration::from_secs(a)),
        };
        let invites: Invites = [("infra".to_string(), vec![inv("infra", Some("s1"), Some(240))]), ("deploy".to_string(), vec![inv("deploy", None, None)])].into();
        let known = [seen("Ada", "s1", true, true)];
        let invited = invitations_line("s2", &invites, &known, now);
        assert_eq!(invited, "invited to #deploy · invited to #infra by Ada 4m ago", "an agent inviter by its name; a record from before by: and at: says neither");
        assert_eq!(invitations_line("s1", &invites, &known, now), "", "no invitation, nothing said");
        assert_eq!(who_line(&seen("Bo", "s2", false, true), &[], &invited, None), "@Bo — invited to #deploy · invited to #infra by Ada 4m ago · session s2 · root /work/Bo · quiet: no heartbeat", "first after the name, before a root that can run off the status line");
    }

    fn seen(name: &str, member: &str, live: bool, claude: bool) -> Seen {
        let identity = crate::chip::KnownIdentity {
            nickname: name.into(),
            cwd: format!("/work/{name}"),
            is_claude: claude,
            palette: agent_identity::Identity::for_cwd("/w", ColorDepth::Ansi16).palette,
            style: Default::default(),
        };
        Seen { identity, live, member: member.into() }
    }

    #[test]
    fn a_channels_members_are_listed_live_or_quiet() {
        let roster = [Peer { session_id: "s1".into(), root: "/work/a".into(), display: "Ada-x".into() }];
        let seen = [seen("Bo", "s2", false, true), seen("cy", "cy", true, false)];
        let ids: Vec<String> = ["s1", "s2", "cy", "s9"].iter().map(|s| s.to_string()).collect();
        let m = members(&ids, &roster, &seen);
        let options: Vec<String> = m.iter().map(Member::option).collect();
        assert_eq!(options, ["Ada-x", "Bo (quiet)", "cy", "s9 (quiet)"], "a quiet member is listed, by the column's name");
        assert_eq!(m[1].root.as_deref(), Some("/work/Bo"), "a quiet claude is still told");
        assert_eq!(m[2].root, None, "a human has no inbox");
    }

    #[test]
    fn every_item_that_drops_something_asks_first() {
        let mut c = ChatPane::new(None, Default::default());
        c.dry_run = true;
        let id = |c: &ChatPane| c.asked.as_ref().map(|(id, _)| id.clone()).expect("a question pending");
        for (item, ask) in [(HIDE_HERE, "hide the messages so far in #deploy?"), (LEAVE, "leave #deploy?"), (DELETE, "delete #deploy")] {
            c.item("deploy", item);
            assert!(matches!(c.to_open.take(), Some(Open::Ask(Question { question, .. })) if question.contains(ask)), "{item} asks the shell");
            assert_eq!(c.status, "", "{item} ran nothing yet");
            let asked = id(&c);
            c.menu_picked(&asked, "n");
            assert_eq!(c.status, "kept");
            c.status.clear();
        }
        c.item("deploy", LEAVE);
        let asked = id(&c);
        c.menu_picked(&asked, "y");
        assert_eq!(c.status, "dry run: /leave not run", "y goes ahead");
    }

    /// Each question has its own id, and an answer acts only on the question
    /// it answers: an answer that arrives for an earlier question, after a
    /// later one took its place, does nothing.
    #[test]
    fn an_answer_acts_only_on_its_own_question() {
        let mut c = ChatPane::new(None, Default::default());
        c.dry_run = true;
        c.item("deploy", LEAVE);
        let first = c.asked.as_ref().map(|(id, _)| id.clone()).unwrap();
        c.item("deploy", HIDE_HERE);
        let second = c.asked.as_ref().map(|(id, _)| id.clone()).unwrap();
        assert_ne!(first, second, "two questions, two ids");
        c.menu_picked(&first, "y");
        assert_eq!(c.status, "", "the leave's y does not run the hide");
        assert!(c.asked.as_ref().is_some_and(|(id, _)| *id == second), "the hide still waits for its own answer");
        c.menu_picked(&second, "y");
        assert_eq!(c.status, "dry run: nothing hidden");
    }

    #[test]
    fn remove_never_offers_the_operator_and_their_own_row_offers_leave() {
        let del = || vec![Item::new(HIDE_HERE), Item::new(DELETE_READ)];
        let l = |items: Vec<Item>| items.into_iter().map(|i| i.label).collect::<Vec<_>>();
        assert_eq!(l(agent_items("Wells", Some((false, false)), Some("deploy"), false, Ok(()), del(), false)), [SHOW_DMS, REVIEW_DMS, "Address @Wells", WHO, ADD_TO_CHAT, HIDE_HERE, DELETE_READ, "Remove from #deploy"]);
        assert_eq!(l(agent_items("aaron", None, Some("deploy"), true, Ok(()), del(), true)), ["Address @aaron", WHO, ADD_TO_CHAT, "Leave #deploy"], "the operator leaves, never removes themself; a human has no DM view, nothing to delete, no tombstone");
        assert_eq!(l(agent_items("Lyra", Some((true, false)), None, false, Ok(()), del(), false)), [HIDE_DMS, REVIEW_DMS, "Address @Lyra", WHO, ADD_TO_CHAT, HIDE_HERE, DELETE_READ], "not listed: nothing to remove; its DMs listed: hide them");
        assert_eq!(l(agent_items("Lyra", Some((false, true)), None, false, Ok(()), del(), true)), [SHOW_DMS, HIDE_REVIEW, "Address @Lyra", WHO, ADD_TO_CHAT, HIDE_HERE, DELETE_READ, TOMBSTONE], "its own DMs listed: hide them; not live: Tombstone… last");
        let m = |id: &str| Member { id: id.into(), label: id.into(), live: true, root: None };
        let left: Vec<String> = removable(vec![m("s1"), m("aaron"), m("s2")], "aaron").into_iter().map(|m| m.id).collect();
        assert_eq!(left, ["s1", "s2"], "the Remove agent picker leaves the operator out");
        let mut c = ChatPane::new(None, Default::default());
        c.dry_run = true;
        c.menu_picked("agent:aaron", "Leave #deploy");
        assert_eq!(c.asked.map(|(_, step)| step), Some(Step::Leave("deploy".into())), "Leave asks first, as the channel menu's does");
    }

    /// A pane whose world is `seen`, shown on merged.
    fn pane_with(seen: Vec<Seen>) -> ChatPane {
        let mut c = ChatPane::new(None, Default::default());
        c.dry_run = true;
        c.world = Some(super::super::World::new(Vec::new(), seen, Vec::new(), Vec::new(), Default::default(), attend_instances::SnapshotCache::new(), &c.foreground, &c.views));
        c
    }

    #[test]
    fn add_agent_to_chat_is_set_back_with_why_when_nothing_can_be_added() {
        let human = seen("aaron", "aaron", true, false);
        let quiet = seen("Bo", "s2", false, true);
        let everywhere = seen("Ada", "s1", true, true);
        let mut c = pane_with(vec![human.clone(), quiet.clone(), everywhere.clone()]);
        assert_eq!(c.can_add(&human), Err("a human joins with /join".into()));
        assert_eq!(c.can_add(&quiet), Err("quiet: no live session to add".into()));
        assert_eq!(c.can_add(&everywhere), Err("already in every channel".into()), "no channel it is not in");
        let items = agent_items("Ada", None, None, false, c.can_add(&everywhere), Vec::new(), false);
        let add = items.iter().find(|i| i.label == ADD_TO_CHAT).expect("the item is listed");
        assert!(!add.enabled && add.reason.as_deref() == Some("already in every channel"), "{add:?}");
    }

    #[test]
    fn who_is_a_live_twin_is_found_by_its_session_id_not_its_name() {
        // Two live claudes share the nickname; only s2 holds a sessions
        // record, so only s2 is on the roster.
        let roster = [Peer { session_id: "s2".into(), root: "/work/Ada".into(), display: "Ada-two".into() }];
        let (one, two) = (seen("Ada", "s1", true, true), seen("Ada", "s2", true, true));
        assert_eq!(who_line(&two, &roster, "", None), "@Ada-two — session s2 · root /work/Ada", "the roster's own line, by id");
        let first = who_line(&one, &roster, "", None);
        assert!(first.starts_with("@Ada — session s1 · root /work/Ada") && first.contains("no sessions record"), "never its twin's line: {first}");
        assert!(!first.contains("quiet"), "a live claude is not called quiet: {first}");
        assert!(who_line(&seen("Ada", "s2", false, true), &roster, "", None).contains("quiet: no heartbeat"), "a quiet one is told from the column");
    }

    /// A quiet claude that left a heartbeat is told how long ago it was,
    /// not that it has none: an invitee whose last beat is days old reads
    /// apart from one that beat a minute ago.
    #[test]
    fn who_is_a_quiet_claude_says_how_old_its_last_heartbeat_is() {
        let quiet = seen("Bo", "s2", false, true);
        let day = std::time::Duration::from_secs(3 * 86_400);
        assert!(who_line(&quiet, &[], "", Some(day)).ends_with(" · quiet: last heartbeat 3d ago"), "{}", who_line(&quiet, &[], "", Some(day)));
        assert!(who_line(&quiet, &[], "", None).ends_with(" · quiet: no heartbeat"));
        let live = seen("Bo", "s2", true, true);
        assert!(who_line(&live, &[], "", Some(day)).ends_with(" · live: no sessions record"), "a live one is not told its heartbeat");
    }

    #[test]
    fn an_agent_row_is_found_by_its_id_when_two_share_a_name() {
        let mut c = pane_with(vec![seen("Ada", "s1", false, true), seen("Ada", "s2", false, true)]);
        c.agent_menu("s2".into(), None, None);
        match c.to_open.take() {
            Some(Open::Pick { id, title, .. }) => assert_eq!((id.as_str(), title.as_str()), ("agent:s2", "@Ada")),
            other => panic!("no menu: {other:?}"),
        }
        c.menu_picked("agent:s2", WHO);
        assert!(c.status.contains("session s2"), "who is the row picked, not the first of its name: {}", c.status);
        c.agent_menu("gone".into(), None, None);
        assert!(c.to_open.is_none() && c.status_is_error, "a row gone since is said, not guessed");
    }
}
