//! The tab menu: what a tab's menu offers and what each item does, the
//! question a destructive item waits on, and the compose box lent to a
//! name or a description. Every item runs the slash command that
//! does the same thing ([`keys::run_slash`]); the commands stay.
//!
//! - `≡`, the common menu: Theme ▸ (this session's look), Keybinding set ▸
//!   (presets over `attend.chat.tabs.*`), the mouse at start, Hide column
//!   or Show column (the left column, as `attend.chat.sidebar_key` toggles
//!   it), Settings…, Quit (as Ctrl-C, asking first over a draft; Esc never
//!   quits, ADR-409).
//! - the operator view (`⋯all`, every channel together): Clear view. It is
//!   not a channel, so it offers no channel's items (ADR-405).
//! - `#open`: Clear view, Clear history.
//! - a named channel: Add agent ▸ (invites a live peer, which joins
//!   itself, as the agent menu's Add agent to chat does), Remove agent ▸,
//!   Describe…, Clear history, Leave (Join for a channel the human is
//!   not in), Seal, Delete channel.
//! - the `+` slot and Ctrl+N: no menu, straight to the name of a new
//!   channel, typed in an entry dialog docked under `+`. Describe… asks its
//!   line in the same dialog, docked beside its item. The compose box is
//!   for chatting and is never borrowed (ADR-409): its draft stays as it
//!   is. A name the dialog refuses is said inside it, the text kept.
//! - an agent's row in the left column: for a claude first Show DMs, or
//!   Hide DMs while its DM view is listed ([`super::dm`]), then Address
//!   @Name (puts `@Name ` in the compose box), Who is this (what `/whois`
//!   says of the session the row is, or what the chat knows of one the
//!   roster lacks), Add agent to chat ▸ (a picker, `add to channel`, of
//!   the channels the agent is not in; the pick invites it, as Add agent ▸
//!   does), and
//!   Remove from #channel when the shown channel lists the agent, live or
//!   quiet. The operator's own row offers Leave #channel there instead:
//!   Remove never takes out the human at the keyboard, here or in the
//!   Remove agent picker.
//! - a DM view, by its row or its tab: Hide DMs.
//! - a message in the feed, by Enter on it: Reply (its channel shown and
//!   the keyboard in the compose box; a direct message is answered by
//!   addressing its sender, and one the chat sent by showing its DM view),
//!   Quote (its lines put in the box as `> ` lines), Address @Name (as
//!   the agent menu's), Copy (the message to the terminal's clipboard by
//!   OSC 52).
//!
//! Every item that drops something asks first: Clear view, Clear history,
//! Leave, Delete channel and Remove. The question is the shell's
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
//! picker marks the current value. Add agent asks a live peer to join
//! (it joins itself, and its join keeps the room's recent history to read).
//!
//! One item waits on attend work and is drawn set back with what it waits
//! on, passed over by the highlight and deaf to a click: Seal (ADR-404 D4),
//! never the first item. Adding an agent is an invitation it answers by
//! joining until attend can enrol a member without asking (ADR-404 D1).

use agent_theme::ColorDepth;
use agent_tui::ratatui::layout::Rect;
use agent_tui::{Anchor, Item, Open};

use crate::chip::Seen;
use crate::peers::Peer;

use super::dm::{HIDE_DMS, SHOW_DMS};
use super::keys::{self, EnterAction};
use super::ChatPane;
use crate::groups::BASE_CHANNEL_NAME;
use crate::slash::SlashOutcome;
use crate::tabs::{self, Slot, Tab};

pub(super) const CLEAR_VIEW: &str = "Clear view";
pub(super) const CLEAR_HISTORY: &str = "Clear history";
pub(super) const ADD_AGENT: &str = "Add agent ▸";
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
pub(super) const SETTINGS: &str = "Settings…";
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

/// The id the chat's questions go to the shell under ([`agent_tui::Open::Ask`]).
const ASK: &str = "ask";

/// A step that asks first, waiting for its answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Step {
    /// Empty the view (`/clear`); the bus keeps every message.
    ClearView,
    /// Leave the channel (`/leave`).
    Leave(String),
    /// Delete the channel's history (`/purge`).
    ClearHistory(String),
    /// Delete the channel (`/dissolve`).
    Delete(String),
    /// Take a member out of the channel (`/kick`).
    Remove(String, Member),
}

impl Step {
    /// The question, then what yes and no do, as the answers name them.
    fn question(&self) -> (String, &'static str, &'static str) {
        match self {
            Step::ClearView => ("clear the view? the bus keeps every message".into(), "clear", "keep"),
            Step::Leave(c) => (format!("leave #{c}?"), "leave", "stay"),
            Step::ClearHistory(c) => (format!("clear #{c} history? deletes what every live agent has read, older than 90 s"), "clear", "keep"),
            Step::Delete(c) => (format!("delete #{c} and its history?"), "delete", "keep"),
            Step::Remove(c, m) => (format!("remove @{} from #{c}?", m.label), "remove", "keep"),
        }
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

/// The items of an agent row's menu: its DM view shown or hidden, for an
/// agent that has one (`dms`: whether the column lists it), address it, who
/// it is, add it to a channel, then for the shown channel that lists it
/// (`listed`), Remove, or Leave on the operator's own row. `add` is why the
/// agent cannot be added to a channel, when it cannot: Add agent to chat is
/// then set back with the reason.
pub(super) fn agent_items(name: &str, dms: Option<bool>, listed: Option<&str>, operator: bool, add: Result<(), String>) -> Vec<Item> {
    let mut out: Vec<Item> = dms.map(|on| Item::new(if on { HIDE_DMS } else { SHOW_DMS })).into_iter().collect();
    out.extend([Item::new(format!("{ADDRESS} @{name}")), Item::new(WHO)]);
    out.push(match add {
        Ok(()) => Item::new(ADD_TO_CHAT),
        Err(why) => Item::disabled(ADD_TO_CHAT, why),
    });
    if let Some(c) = listed {
        out.push(Item::new(if operator { format!("{LEAVE} #{c}") } else { format!("{REMOVE_FROM} #{c}") }));
    }
    out
}

/// The channels an agent can be added to: those on disk it is not in,
/// `#open` left out (every agent is already there).
pub(super) fn addable(rows: &[crate::groups::ChannelRow], member: &str) -> Vec<String> {
    rows.iter()
        .filter(|r| r.on_disk && !r.known.is_base && !r.known.membership.members.iter().any(|m| m == member))
        .map(|r| r.known.group.name.clone())
        .collect()
}

/// The items of a tab's menu, by the channel it shows (`None`: merged):
/// a named channel offers Leave to a member and Join to anyone else. Add
/// agent and Seal wait on attend (ADR-404 D1, D4) and say so; neither is
/// first, so the highlight never starts on one.
pub(super) fn items(channel: Option<&str>, joined: bool) -> Vec<Item> {
    let membership = if joined { LEAVE } else { JOIN };
    match channel {
        None => vec![CLEAR_VIEW.into()],
        Some(BASE_CHANNEL_NAME) => vec![CLEAR_VIEW.into(), CLEAR_HISTORY.into()],
        Some(_) => vec![
            ADD_AGENT.into(),
            REMOVE_AGENT.into(),
            DESCRIBE.into(),
            CLEAR_HISTORY.into(),
            membership.into(),
            Item::disabled(SEAL, "needs ADR-404 D4"),
            DELETE.into(),
        ],
    }
}

/// The items of a message's menu: reply where it was said, quote it,
/// address its sender `name`, copy it. A local notice has no channel to
/// reply in, and a message with no sender the column knows has no one to
/// address.
pub(super) fn message_items(name: Option<&str>, notice: bool) -> Vec<Item> {
    vec![
        if notice { Item::disabled(REPLY, "a notice of the chat's") } else { Item::new(REPLY) },
        Item::new(QUOTE),
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

/// What Who is this says of `s`. A live claude the roster holds under its
/// session id is told as `/whois` tells it; the id, not the name, finds it,
/// since twins share a name and `/whois @name` would pick one of them. A
/// live claude with no sessions record, a quiet one and a human are told
/// from what the chat itself has seen.
pub(super) fn who_line(s: &Seen, roster: &[Peer]) -> String {
    let name = &s.identity.nickname;
    if !s.identity.is_claude {
        return format!("@{name} — a human · from {} · {}", s.identity.cwd, if s.live { "here" } else { "away" });
    }
    if let Some(p) = roster.iter().find(|p| s.live && p.session_id == s.member) {
        return format!("@{} — session {} · root {}", p.display, p.session_id, p.root);
    }
    let how = if s.live { "live: no sessions record" } else { "quiet: no heartbeat" };
    format!("@{name} — session {} · root {} · {how}", s.member, attend_presence::session::normalize_origin(&s.identity.cwd))
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
        self.to_open = Some(Open::Pick { id, title, items: items(channel.as_deref(), joined), multi: false, anchor });
    }

    pub(super) fn common_menu(&mut self) {
        let mouse = if self.config.mouse { MOUSE_OFF } else { MOUSE_ON };
        let column = if self.sidebar { HIDE_COLUMN } else { SHOW_COLUMN };
        let items = [THEME, KEYS, mouse, column, SETTINGS, QUIT].into_iter().map(Item::from).collect();
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
            // Every chat setting, a line each, in the response modal: what
            // `/config` lists. Setting one is `/config <key> <value>` until
            // the settings can be edited here.
            SETTINGS => {
                let text = crate::settings::listing_lines(&std::env::current_dir().unwrap_or_default()).join("\n");
                let text = format!("{text}\n\n/config <key> <value> sets one");
                self.to_open = Some(Open::Report { label: "settings".into(), command: "/config".into(), text });
            }
            // The shell's quit, as Ctrl-C: at once, or the exit guard over
            // a draft.
            QUIT => self.to_open = Some(Open::Quit),
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
            ASK => return self.answered(value == "y"),
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
        if let Some(c) = id.strip_prefix("menu:") {
            return self.item(c, value);
        }
        if let Some(member) = id.strip_prefix("agent:") {
            return self.agent_item(member, value);
        }
        if let Some(member) = id.strip_prefix("dm:") {
            return self.dm_item(member, value);
        }
        if let Some(m) = id.strip_prefix("message:") {
            return self.message_item(m, value);
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
        // A peer from Add agent or Remove agent: the label's first word is
        // the display name the commands resolve.
        let peer = value.split_whitespace().next().unwrap_or(value).to_string();
        if let Some(c) = id.strip_prefix("invite:") {
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
    pub(super) fn agent_menu(&mut self, member: String, at: Option<Rect>) {
        self.dirty = true;
        let Some(s) = self.seen_member(&member) else {
            return self.say("that agent is not in the column any more", true);
        };
        let name = s.identity.nickname.clone();
        let me = crate::signal::human_member_id();
        let listed = match self.normal_tab() {
            Tab::Channel(c) => self.channel_members(&c).into_iter().find(|m| m.id == member).map(|m| (c, m)),
            Tab::Merged | Tab::Dm(_) => None,
        };
        let add = self.can_add(&s);
        let dms = self.has_dms(&member).then(|| self.dms.contains(&member));
        let items = agent_items(&name, dms, listed.as_ref().map(|(c, _)| c.as_str()), member == me, add);
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
        let (i, sig) = self.signals.iter().filter(|s| tabs::shows(s, &shown)).enumerate().find(|(_, s)| s.id == id)?;
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
        let Some(id) = self.signals.iter().filter(|s| tabs::shows(s, &shown)).nth(i).map(|s| s.id.clone()) else { return };
        let Some((sig, name)) = self.feed_message(&id) else { return };
        let feed = self.hits.feed;
        let anchor = self.feed.header_row(i).map_or(Anchor::Center, |r| Anchor::Row(Rect { x: feed.x, y: feed.y + r, width: feed.width, height: 1 }));
        let title = name.as_ref().map_or("message".to_string(), |n| format!("message from @{n}"));
        let items = message_items(name.as_deref(), sig.channel == crate::signal::Channel::Info);
        self.to_open = Some(Open::Pick { id: format!("message:{}", sig.id), title, items, multi: false, anchor });
    }

    /// An item of the menu of the message with the id `id`. A message the
    /// feed has dropped since the menu opened is acted on no more.
    fn message_item(&mut self, id: &str, item: &str) {
        let Some((sig, name)) = self.feed_message(id) else {
            return self.say("that message is not in the feed any more", true);
        };
        let address = |c: &mut ChatPane, n: &str| c.complete(super::Chip::Agent(n.to_string()));
        match item {
            REPLY => match (&sig.channel, name.as_deref()) {
                (crate::signal::Channel::Group(g), _) => self.set_tab(Tab::Channel(g.clone())),
                (crate::signal::Channel::Open, _) => self.set_tab(Tab::Channel(BASE_CHANNEL_NAME.into())),
                (crate::signal::Channel::Direct | crate::signal::Channel::Person, Some(n)) => address(self, n),
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

    fn agent_item(&mut self, member: &str, item: &str) {
        if item == SHOW_DMS {
            self.open_dm(member.to_string());
        } else if item == HIDE_DMS {
            self.hide_dm(member);
        } else if item.starts_with(ADDRESS) {
            match self.seen_member(member) {
                Some(s) => self.complete(super::Chip::Agent(s.identity.nickname)),
                None => self.say("that agent is not in the column any more", true),
            }
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
        self.say(who_line(&s, &roster), false);
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
            CLEAR_VIEW => self.ask(Step::ClearView),
            CLEAR_HISTORY => self.ask(Step::ClearHistory(chan().unwrap_or(BASE_CHANNEL_NAME.into()))),
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
            ADD_AGENT => self.peer_menu(channel, true),
            REMOVE_AGENT => self.peer_menu(channel, false),
            _ => {}
        }
    }

    /// The live peers not in `channel` (add), or every member of it but
    /// the operator, live or quiet (remove): a quiet member is removed by
    /// its membership id; the operator leaves with Leave.
    fn peer_menu(&mut self, channel: &str, add: bool) {
        let options: Vec<String> = if add {
            let members = attend_groups::load_groups(&crate::signal::signals_base()).get(channel).map(|e| e.members.clone()).unwrap_or_default();
            let roster = crate::peers::roster(ColorDepth::detect(), &attend_instances::SnapshotCache::new());
            roster.iter().filter(|p| !members.contains(&p.session_id)).map(|p| p.display.clone()).collect()
        } else {
            let members = self.channel_members(channel);
            self.removable = removable(members, &crate::signal::human_member_id());
            self.removable.iter().map(Member::option).collect()
        };
        if options.is_empty() {
            let what = if add { "no live agent outside" } else { "no member in" };
            return self.say(format!("{what} #{channel}"), true);
        }
        let (id, title) = if add { (format!("invite:{channel}"), format!("add to #{channel}")) } else { (format!("remove:{channel}"), format!("remove from #{channel}")) };
        let items = options.into_iter().map(Item::from).collect();
        self.to_open = Some(Open::Pick { id, title, items, multi: false, anchor: Anchor::Parent });
    }

    /// Whether `s` can be added to a channel from its menu, and if not why.
    /// Adding is an invitation the agent answers by joining (ADR-404: attend
    /// has no enrolment without asking yet, D1), so it needs a live claude
    /// with an inbox, and a channel it is not in.
    fn can_add(&mut self, s: &Seen) -> Result<(), String> {
        if !s.identity.is_claude {
            return Err("a human joins with /join".into());
        }
        if !s.live {
            return Err("quiet: no live session to invite".into());
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
        // Under the agent's row, where its menu was; beside the closed menu
        // only when the row is not drawn.
        let anchor = self.side_row(&super::sidebar::SideHit::Agent(member.to_string())).map_or(Anchor::Parent, Anchor::Row);
        self.to_open = Some(Open::Pick { id: format!("addto:{member}"), title: "add to channel".into(), items, multi: false, anchor });
    }

    /// The channel picked for an agent: it is invited there, by its session,
    /// as Add agent ▸ invites, and joins itself (ADR-404).
    fn add_to(&mut self, member: &str, channel: &str) {
        if self.dry_run {
            return self.say("dry run: /invite not run", false);
        }
        let roster = crate::peers::roster(ColorDepth::detect(), &attend_instances::SnapshotCache::new());
        let Some(peer) = roster.into_iter().find(|p| p.session_id == member) else {
            return self.say("that agent has no live session to invite", true);
        };
        let action = keys::invite_peer(channel, &peer);
        if self.apply(action) {
            let said = format!("{} — they join themselves", self.status);
            self.say(said, false);
        }
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
    /// ([`agent_tui::Open::Ask`]), and the answer comes back through
    /// [`ChatPane::answered`].
    pub(super) fn ask(&mut self, step: Step) {
        let (question, yes, no) = step.question();
        self.asked = Some(step);
        self.to_open = Some(Open::Ask { id: ASK.into(), question, yes: yes.into(), no: no.into() });
    }

    /// The answer to the question asked: yes runs the step, no keeps things
    /// as they are and says so.
    pub(super) fn answered(&mut self, yes: bool) {
        let Some(step) = self.asked.take() else { return };
        self.dirty = true;
        if !yes {
            return self.say("kept", false);
        }
        match step {
            Step::ClearView => {
                self.run("clear", SlashOutcome::ClearTranscript);
            }
            Step::Leave(c) => {
                self.run("leave", SlashOutcome::Leave(Some(c)));
            }
            Step::ClearHistory(c) => {
                self.run("purge", SlashOutcome::Purge(Some(c)));
            }
            Step::Delete(c) => {
                self.run("dissolve", SlashOutcome::Dissolve(Some(c)));
            }
            Step::Remove(c, m) => {
                if self.dry_run {
                    self.say("dry run: /kick not run", false);
                } else {
                    self.apply(keys::kick_member(&c, &m.id, &m.label, m.root.as_deref()));
                }
            }
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
        let action = keys::run_slash(outcome, &fg);
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
                self.push(echo);
                true
            }
            EnterAction::StatusOnly(s) => {
                self.say(s, true);
                false
            }
            EnterAction::ClearWithStatusAndFocus { status, focus } => {
                self.say(status, false);
                self.set_tab(focus);
                true
            }
            EnterAction::ClearTranscript => {
                self.signals.clear();
                self.say("transcript cleared", false);
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
        assert_eq!(labels(&items(None, false)), [CLEAR_VIEW]);
        assert_eq!(labels(&items(Some("open"), true)), [CLEAR_VIEW, CLEAR_HISTORY], "the harmless item first");
        assert_eq!(labels(&items(Some("deploy"), true)), [ADD_AGENT, REMOVE_AGENT, DESCRIBE, CLEAR_HISTORY, LEAVE, SEAL, DELETE]);
        assert_eq!(labels(&items(Some("deploy"), false)), [ADD_AGENT, REMOVE_AGENT, DESCRIBE, CLEAR_HISTORY, JOIN, SEAL, DELETE], "Join to a non-member");
        let deploy = items(Some("deploy"), true);
        let waiting: Vec<(&str, Option<&str>)> = deploy.iter().filter(|i| !i.enabled).map(|i| (i.label.as_str(), i.reason.as_deref())).collect();
        assert_eq!(waiting, [(SEAL, Some("needs ADR-404 D4"))], "what waits on attend says so; Add agent invites, as the agent menu's Add agent to chat does");
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
        for (item, ask) in [(CLEAR_VIEW, "clear the view?"), (CLEAR_HISTORY, "clear #deploy history?"), (LEAVE, "leave #deploy?"), (DELETE, "delete #deploy")] {
            c.item("deploy", item);
            assert!(matches!(c.to_open.take(), Some(Open::Ask { question, .. }) if question.contains(ask)), "{item} asks the shell");
            assert_eq!(c.status, "", "{item} ran nothing yet");
            c.menu_picked(ASK, "n");
            assert_eq!(c.status, "kept");
            c.status.clear();
        }
        c.item("deploy", LEAVE);
        c.menu_picked(ASK, "y");
        assert_eq!(c.status, "dry run: /leave not run", "y goes ahead");
    }

    #[test]
    fn remove_never_offers_the_operator_and_their_own_row_offers_leave() {
        let l = |items: Vec<Item>| items.into_iter().map(|i| i.label).collect::<Vec<_>>();
        assert_eq!(l(agent_items("Wells", Some(false), Some("deploy"), false, Ok(()))), [SHOW_DMS, "Address @Wells", WHO, ADD_TO_CHAT, "Remove from #deploy"]);
        assert_eq!(l(agent_items("aaron", None, Some("deploy"), true, Ok(()))), ["Address @aaron", WHO, ADD_TO_CHAT, "Leave #deploy"], "the operator leaves, never removes themself; a human has no DM view");
        assert_eq!(l(agent_items("Lyra", Some(true), None, false, Ok(()))), [HIDE_DMS, "Address @Lyra", WHO, ADD_TO_CHAT], "not listed: nothing to remove; its DMs listed: hide them");
        let m = |id: &str| Member { id: id.into(), label: id.into(), live: true, root: None };
        let left: Vec<String> = removable(vec![m("s1"), m("aaron"), m("s2")], "aaron").into_iter().map(|m| m.id).collect();
        assert_eq!(left, ["s1", "s2"], "the Remove agent picker leaves the operator out");
        let mut c = ChatPane::new(None, Default::default());
        c.dry_run = true;
        c.menu_picked("agent:aaron", "Leave #deploy");
        assert_eq!(c.asked, Some(Step::Leave("deploy".into())), "Leave asks first, as the channel menu's does");
    }

    /// A pane whose world is `seen`, shown on merged.
    fn pane_with(seen: Vec<Seen>) -> ChatPane {
        let mut c = ChatPane::new(None, Default::default());
        c.dry_run = true;
        c.world = Some(super::super::World::new(Vec::new(), seen, Vec::new(), Vec::new(), attend_instances::SnapshotCache::new(), &c.foreground, &c.dms));
        c
    }

    #[test]
    fn add_agent_to_chat_is_set_back_with_why_when_nothing_can_be_added() {
        let human = seen("aaron", "aaron", true, false);
        let quiet = seen("Bo", "s2", false, true);
        let everywhere = seen("Ada", "s1", true, true);
        let mut c = pane_with(vec![human.clone(), quiet.clone(), everywhere.clone()]);
        assert_eq!(c.can_add(&human), Err("a human joins with /join".into()));
        assert_eq!(c.can_add(&quiet), Err("quiet: no live session to invite".into()));
        assert_eq!(c.can_add(&everywhere), Err("already in every channel".into()), "no channel it is not in");
        let items = agent_items("Ada", None, None, false, c.can_add(&everywhere));
        let add = items.iter().find(|i| i.label == ADD_TO_CHAT).expect("the item is listed");
        assert!(!add.enabled && add.reason.as_deref() == Some("already in every channel"), "{add:?}");
    }

    #[test]
    fn who_is_a_live_twin_is_found_by_its_session_id_not_its_name() {
        // Two live claudes share the nickname; only s2 holds a sessions
        // record, so only s2 is on the roster.
        let roster = [Peer { session_id: "s2".into(), root: "/work/Ada".into(), display: "Ada-two".into() }];
        let (one, two) = (seen("Ada", "s1", true, true), seen("Ada", "s2", true, true));
        assert_eq!(who_line(&two, &roster), "@Ada-two — session s2 · root /work/Ada", "the roster's own line, by id");
        let first = who_line(&one, &roster);
        assert!(first.starts_with("@Ada — session s1 · root /work/Ada") && first.contains("no sessions record"), "never its twin's line: {first}");
        assert!(!first.contains("quiet"), "a live claude is not called quiet: {first}");
        assert!(who_line(&seen("Ada", "s2", false, true), &roster).contains("quiet: no heartbeat"), "a quiet one is told from the column");
    }

    #[test]
    fn an_agent_row_is_found_by_its_id_when_two_share_a_name() {
        let mut c = pane_with(vec![seen("Ada", "s1", false, true), seen("Ada", "s2", false, true)]);
        c.agent_menu("s2".into(), None);
        match c.to_open.take() {
            Some(Open::Pick { id, title, .. }) => assert_eq!((id.as_str(), title.as_str()), ("agent:s2", "@Ada")),
            other => panic!("no menu: {other:?}"),
        }
        c.menu_picked("agent:s2", WHO);
        assert!(c.status.contains("session s2"), "who is the row picked, not the first of its name: {}", c.status);
        c.agent_menu("gone".into(), None);
        assert!(c.to_open.is_none() && c.status_is_error, "a row gone since is said, not guessed");
    }
}
