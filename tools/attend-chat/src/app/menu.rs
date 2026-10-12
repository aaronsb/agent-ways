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
//! - merged: Clear view.
//! - `#open`: Clear history, Clear view.
//! - a named channel: Add agent ▸, Invite agent ▸, Remove agent ▸,
//!   Describe…, Clear history, Leave (Join for a channel the human is
//!   not in), Delete channel.
//! - the `+` slot and Ctrl+N: no menu, straight to the name of a new
//!   channel, typed in the compose box, which says `new channel name >`
//!   while it is lent. Showing a tab, opening a channel's menu, an `≡` item
//!   other than Quit, or the column taking the keyboard gives the box back,
//!   with the draft it held. Quit leaves it lent, so the exit guard asks
//!   over a name typed there.
//! - an agent's row in the left column: Address @Name (puts `@Name ` in
//!   the compose box), Who is this (what `/whois` says of the session
//!   the row is, or what the chat knows of one the roster lacks), and
//!   Remove from #channel when the shown channel lists the agent, live or
//!   quiet. The operator's own row offers Leave #channel there instead:
//!   Remove never takes out the human at the keyboard, here or in the
//!   Remove agent picker.
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
//! leads to (Theme ▸, Invite agent ▸) docks beside that item's row. A
//! picker marks the current value. Invite agent asks a live peer to join
//! (it joins itself, and its join keeps the room's recent history to read).
//!
//! Two items wait on attend work and are drawn set back with what they wait
//! on, passed over by the highlight and deaf to a click: Add agent,
//! enrolling a member with no answer from it (ADR-404 D1), and Seal
//! (ADR-404 D4). Neither is ever the first item.

use agent_theme::ColorDepth;
use agent_tui::{Anchor, Item, Open};

use crate::chip::Seen;
use crate::peers::Peer;

use super::keys::{self, EnterAction};
use super::ChatPane;
use crate::groups::BASE_CHANNEL_NAME;
use crate::slash::SlashOutcome;
use crate::tabs::Tab;

pub(super) const CLEAR_VIEW: &str = "Clear view";
pub(super) const CLEAR_HISTORY: &str = "Clear history";
pub(super) const ADD_AGENT: &str = "Add agent";
pub(super) const SEAL: &str = "Seal";
pub(super) const INVITE_AGENT: &str = "Invite agent ▸";
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

/// The items of an agent row's menu: address it, who it is, then for the
/// shown channel that lists it (`listed`), Remove, or Leave on the
/// operator's own row.
pub(super) fn agent_items(name: &str, listed: Option<&str>, operator: bool) -> Vec<String> {
    let mut out = vec![format!("{ADDRESS} @{name}"), WHO.to_string()];
    if let Some(c) = listed {
        out.push(if operator { format!("{LEAVE} #{c}") } else { format!("{REMOVE_FROM} #{c}") });
    }
    out
}

/// What the compose box is lent to, and the draft it held before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Prompt {
    NewChannel,
    Describe(String),
}

impl Prompt {
    /// What the box says it is lent to, in place of `>`.
    pub fn label(&self) -> String {
        match self {
            Prompt::NewChannel => "new channel name".into(),
            Prompt::Describe(c) => format!("describe #{c}"),
        }
    }
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
            INVITE_AGENT.into(),
            Item::disabled(ADD_AGENT, "needs ADR-404 D1"),
            REMOVE_AGENT.into(),
            DESCRIBE.into(),
            CLEAR_HISTORY.into(),
            membership.into(),
            Item::disabled(SEAL, "needs ADR-404 D4"),
            DELETE.into(),
        ],
    }
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
    /// The bottom bar's line while the compose box is lent.
    pub(super) fn menu_status(&self) -> Option<String> {
        self.prompt.as_ref().map(|(p, _)| match p {
            Prompt::NewChannel => "new channel: type its name · Enter creates it · Esc cancels".into(),
            Prompt::Describe(c) => format!("describe #{c}: type a line · Enter sets it (empty clears) · Esc cancels"),
        })
    }

    /// Open the menu of the bar's tab `i`: 0 is the `≡` slot, 1 merged,
    /// then the channels in strip order, then the `+` slot. A channel's
    /// menu gives a lent compose box back first; the `+` slot lends it. The
    /// `≡` menu opens over the lent box, and its item decides
    /// ([`ChatPane::common_item`]).
    pub(super) fn open_tab_menu(&mut self, i: usize) {
        self.dirty = true;
        let names = self.strip_names();
        if i == names.len() + 2 {
            return self.new_channel();
        }
        if i == 0 {
            return self.common_menu();
        }
        self.prompt_cancel();
        let channel = match i {
            1 => None,
            i => match names.get(i - 2) {
                Some(n) => Some(n.clone()),
                None => return,
            },
        };
        self.channel_menu(channel);
    }

    /// The menu of `channel`, or merged's for `None`: from its tab, or its
    /// row in the left column, which lists channels that have no tab.
    pub(super) fn channel_menu(&mut self, channel: Option<String>) {
        self.dirty = true;
        let title = channel.as_ref().map_or("merged".to_string(), |c| format!("#{c}"));
        let id = format!("menu:{}", channel.as_deref().unwrap_or(""));
        let me = crate::signal::human_member_id();
        let joined = channel.as_deref().is_some_and(|c| self.world().rows.iter().any(|r| r.known.group.name == c && r.known.membership.members.contains(&me)));
        // Under the channel's row in the column, else under its tab.
        let row = channel.clone().and_then(|c| self.side_row(&super::sidebar::SideHit::Channel(c)));
        let anchor = match (row, &channel) {
            (Some(r), _) => Anchor::Row(r),
            (None, None) => Anchor::Tab(1),
            (None, Some(c)) => self.strip_names().iter().position(|n| n == c).map_or(Anchor::Center, |i| Anchor::Tab(i + 2)),
        };
        self.open = Some(Open::Pick { id, title, items: items(channel.as_deref(), joined), multi: false, anchor });
    }

    fn common_menu(&mut self) {
        let mouse = if self.config.mouse { MOUSE_OFF } else { MOUSE_ON };
        let column = if self.sidebar { HIDE_COLUMN } else { SHOW_COLUMN };
        let items = [THEME, KEYS, mouse, column, SETTINGS, QUIT].into_iter().map(Item::from).collect();
        self.open = Some(Open::Pick { id: "common".into(), title: "attend-chat".into(), items, multi: false, anchor: Anchor::Tab(0) });
    }

    /// The keybinding preset the chat's settings match, if any.
    fn preset(&self) -> Option<&'static str> {
        PRESETS.iter().find(|(_, jump, focus)| self.config.jump == *jump && self.config.focus_key == *focus).map(|(l, _, _)| *l)
    }

    /// An item of the `≡` menu. Quit leaves a lent compose box as it is, so
    /// the shell's quit counts a name typed there as unsaved and asks, as
    /// Ctrl-C does; every other item gives the box back first.
    fn common_item(&mut self, item: &str) {
        if item != QUIT {
            self.prompt_cancel();
        }
        match item {
            THEME => {
                let set = agent_theme::ThemeSet::load(agent_theme::user_dir().as_deref());
                let mut names = vec!["terminal".to_string()];
                names.extend(set.list().map(|(t, _)| t.name.clone()));
                let items = names.into_iter().map(|n| Item::new(n.clone()).current(n == self.theme)).collect();
                self.open = Some(Open::Pick { id: "theme".into(), title: "theme for this session".into(), items, multi: false, anchor: Anchor::Parent });
            }
            KEYS => {
                let current = self.preset();
                let items = PRESETS.iter().map(|(l, _, _)| Item::new(*l).current(current == Some(*l))).collect();
                self.open = Some(Open::Pick { id: "keys".into(), title: "keybinding set".into(), items, multi: false, anchor: Anchor::Parent });
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
                self.open = Some(Open::Report { label: "settings".into(), command: "/config".into(), text });
            }
            // The shell's quit, as Ctrl-C: at once, or the exit guard over
            // a draft.
            QUIT => self.open = Some(Open::Quit),
            _ => {}
        }
    }

    /// The session's look: `terminal` is the terminal's own palette. Kept
    /// for this session; the stored choice is ways' `theme.active`, which
    /// the chat reads and does not write (ADR-504).
    fn theme_picked(&mut self, name: &str) {
        let depth = self.palette.depth();
        let active = (name != "terminal").then_some(name);
        let (palette, warning) = crate::theme::palette(active, depth);
        self.palette = palette;
        self.theme = name.to_string();
        self.invalidate();
        match warning {
            Some(w) => self.say(format!("theme: {w}"), true),
            None => self.say(format!("theme {name} for this session; `ways settings set theme.active {name}` keeps it"), false),
        }
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
        // A peer from Invite agent or Remove agent: the label's first word is
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
    /// its membership id: two agents can share a name, never an id.
    pub(super) fn agent_menu(&mut self, member: String) {
        self.dirty = true;
        let Some(s) = self.seen_member(&member) else {
            return self.say("that agent is not in the column any more", true);
        };
        let name = s.identity.nickname;
        let me = crate::signal::human_member_id();
        let listed = match self.normal_tab() {
            Tab::Channel(c) => self.channel_members(&c).into_iter().find(|m| m.id == member).map(|m| (c, m)),
            Tab::Merged => None,
        };
        let options = agent_items(&name, listed.as_ref().map(|(c, _)| c.as_str()), member == me);
        self.removable = removable(listed.map(|(_, m)| m).into_iter().collect(), &me);
        let anchor = self.side_row(&super::sidebar::SideHit::Agent(member.clone())).map_or(Anchor::Center, Anchor::Row);
        let items = options.into_iter().map(Item::from).collect();
        self.open = Some(Open::Pick { id: format!("agent:{member}"), title: format!("@{name}"), items, multi: false, anchor });
    }

    fn agent_item(&mut self, member: &str, item: &str) {
        if item.starts_with(ADDRESS) {
            match self.seen_member(member) {
                Some(s) => self.complete(super::Chip::Agent(s.identity.nickname)),
                None => self.say("that agent is not in the column any more", true),
            }
        } else if item == WHO {
            self.whois(member);
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
            DESCRIBE => self.start_prompt(Prompt::Describe(channel.into())),
            INVITE_AGENT => self.peer_menu(channel, true),
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
        let (id, title) = if add { (format!("invite:{channel}"), format!("invite to #{channel}")) } else { (format!("remove:{channel}"), format!("remove from #{channel}")) };
        let items = options.into_iter().map(Item::from).collect();
        self.open = Some(Open::Pick { id, title, items, multi: false, anchor: Anchor::Parent });
    }

    /// Ask for a new channel's name in the compose box: from the `+` slot
    /// or Ctrl+N. The name already being typed is left alone, so a repeat
    /// cannot take the name for the draft.
    /// A description being typed is never thrown away: the request waits
    /// until it is sent or cancelled.
    pub(super) fn new_channel(&mut self) {
        self.dirty = true;
        match self.prompt {
            Some((Prompt::NewChannel, _)) => {}
            Some(_) => self.say("finish the description, or Esc, first", false),
            None => self.start_prompt(Prompt::NewChannel),
        }
    }

    /// Lend the compose box to `p`, keeping the draft for after.
    fn start_prompt(&mut self, p: Prompt) {
        let draft = self.input.text().to_string();
        self.input.clear();
        self.prompt = Some((p, draft));
    }

    /// Give the compose box back, with the draft it held.
    fn end_prompt(&mut self) {
        if let Some((_, draft)) = self.prompt.take() {
            let n = draft.chars().count();
            self.input.set(draft, n);
        }
    }

    /// Enter while the compose box is lent: run what it was lent for.
    pub(super) fn prompt_enter(&mut self) {
        let Some((p, _)) = self.prompt.clone() else { return };
        let text = self.input.text().trim().to_string();
        match p {
            Prompt::NewChannel => {
                let name = text.trim_start_matches('#').to_string();
                if let Err(e) = attend_groups::validate_group_name(&name) {
                    return self.say(format!("new channel: {e}"), true);
                }
                self.end_prompt();
                if self.run("channels create", SlashOutcome::CreateChannel { name: name.clone(), description: None }) && !self.dry_run {
                    self.stale();
                    self.set_tab(Tab::Channel(name));
                }
            }
            Prompt::Describe(c) => {
                self.end_prompt();
                self.run("channels describe", SlashOutcome::DescribeChannel { name: c, description: text });
            }
        }
    }

    /// Esc, or a click outside the compose box, while it is lent: give it
    /// back with its draft, saying nothing, as every overlay closes.
    pub(super) fn prompt_cancel(&mut self) {
        if self.prompt.is_some() {
            self.end_prompt();
            self.status.clear();
            self.dirty = true;
        }
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
        self.open = Some(Open::Ask { id: ASK.into(), question, yes: yes.into(), no: no.into() });
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

    #[test]
    fn each_tab_offers_what_its_channel_allows() {
        assert_eq!(labels(&items(None, false)), [CLEAR_VIEW]);
        assert_eq!(labels(&items(Some("open"), true)), [CLEAR_VIEW, CLEAR_HISTORY], "the harmless item first");
        assert_eq!(labels(&items(Some("deploy"), true)), [INVITE_AGENT, ADD_AGENT, REMOVE_AGENT, DESCRIBE, CLEAR_HISTORY, LEAVE, SEAL, DELETE]);
        assert_eq!(labels(&items(Some("deploy"), false)), [INVITE_AGENT, ADD_AGENT, REMOVE_AGENT, DESCRIBE, CLEAR_HISTORY, JOIN, SEAL, DELETE], "Join to a non-member");
        let deploy = items(Some("deploy"), true);
        let waiting: Vec<(&str, Option<&str>)> = deploy.iter().filter(|i| !i.enabled).map(|i| (i.label.as_str(), i.reason.as_deref())).collect();
        assert_eq!(waiting, [(ADD_AGENT, Some("needs ADR-404 D1")), (SEAL, Some("needs ADR-404 D4"))], "what waits on attend says so");
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
            assert!(matches!(c.open.take(), Some(Open::Ask { question, .. }) if question.contains(ask)), "{item} asks the shell");
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
        assert_eq!(agent_items("Wells", Some("deploy"), false), ["Address @Wells", WHO, "Remove from #deploy"]);
        assert_eq!(agent_items("aaron", Some("deploy"), true), ["Address @aaron", WHO, "Leave #deploy"], "the operator leaves, never removes themself");
        assert_eq!(agent_items("Lyra", None, false), ["Address @Lyra", WHO], "not listed: nothing to remove");
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
        c.world = Some(super::super::World { groups: Vec::new(), seen, rows: Vec::new(), live_members: Vec::new(), instances: attend_instances::SnapshotCache::new() });
        c
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
        c.agent_menu("s2".into());
        match c.open.take() {
            Some(Open::Pick { id, title, .. }) => assert_eq!((id.as_str(), title.as_str()), ("agent:s2", "@Ada")),
            other => panic!("no menu: {other:?}"),
        }
        c.menu_picked("agent:s2", WHO);
        assert!(c.status.contains("session s2"), "who is the row picked, not the first of its name: {}", c.status);
        c.agent_menu("gone".into());
        assert!(c.open.is_none() && c.status_is_error, "a row gone since is said, not guessed");
    }
}
