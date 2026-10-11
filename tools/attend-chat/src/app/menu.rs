//! The tab menu: what a tab's menu offers and what each item does, the
//! inline confirm a destructive item waits on, and the compose box lent
//! to a name or a description. Every item runs the slash command that
//! does the same thing ([`keys::run_slash`]); the commands stay.
//!
//! - `≡`, the common menu: Theme ▸ (this session's look), Keybinding set ▸
//!   (presets over `attend.chat.tabs.*`), the mouse at start, Hide column
//!   or Show column (the left column, as `attend.chat.sidebar_key` toggles
//!   it), Settings… .
//! - merged: Clear view.
//! - `#open`: Clear history, Clear view.
//! - a named channel: Add agent ▸, Invite agent ▸, Remove agent ▸,
//!   Describe…, Clear history, Leave (Join for a channel the human is
//!   not in), Delete channel.
//! - the `+` slot and Ctrl+N: no menu, straight to the name of a new
//!   channel, typed in the compose box.
//! - an agent's row in the left column: Address @Name (puts `@Name ` in
//!   the compose box), Who is this (what `/whois` says, or what the chat
//!   knows of one that is not live), and Remove from #channel when the
//!   shown channel lists the agent, live or quiet.
//!
//! Every item that drops something asks first: Clear view, Clear history,
//! Leave and Delete channel. `y` goes ahead, any other key keeps things as
//! they are. A click on a tab opens its menu with the first item under the
//! cursor, so an Enter just after the click must not drop anything. Invite agent asks a live peer to join (it joins
//! itself, and its join keeps the room's recent history to read). Add
//! agent, enrolling a member with no answer from it, is ADR-404 D1 and is
//! not built yet: the item says so.

use agent_theme::ColorDepth;
use agent_tui::Open;

use crate::chip::Seen;
use crate::peers::Peer;

use super::keys::{self, EnterAction};
use super::ChatPane;
use crate::groups::BASE_CHANNEL_NAME;
use crate::slash::SlashOutcome;
use crate::tabs::Tab;

pub(super) const CLEAR_VIEW: &str = "Clear view";
pub(super) const CLEAR_HISTORY: &str = "Clear history";
pub(super) const ADD_AGENT: &str = "Add agent ▸";
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

/// The keybinding presets: a label, then the `attend.chat.tabs.*` values.
pub(super) const PRESETS: &[(&str, &str, &str)] = &[
    ("ctrl: Ctrl+1-9, F2, Ctrl+T", "ctrl", "both"),
    ("alt: Alt+1-9, F2, Ctrl+T", "alt", "both"),
    ("fallback only: F2, Ctrl+T, Tab", "none", "both"),
    ("auto: Ctrl+1-9 where it arrives, else Alt+1-9", "auto", "both"),
];

/// A step waiting for `y`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Pending {
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
    /// Quit: Esc where the terminal sends Ctrl+3 as Esc.
    Quit,
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

/// What the compose box is lent to, and the draft it held before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Prompt {
    NewChannel,
    Describe(String),
}

/// The items of a tab's menu, by the channel it shows (`None`: merged):
/// a named channel offers Leave to a member and Join to anyone else.
pub(super) fn items(channel: Option<&str>, joined: bool) -> Vec<String> {
    let membership = if joined { LEAVE } else { JOIN };
    let items: &[&str] = match channel {
        None => &[CLEAR_VIEW],
        Some(BASE_CHANNEL_NAME) => &[CLEAR_VIEW, CLEAR_HISTORY],
        Some(_) => &[ADD_AGENT, INVITE_AGENT, REMOVE_AGENT, DESCRIBE, CLEAR_HISTORY, membership, DELETE],
    };
    items.iter().map(|s| s.to_string()).collect()
}

impl ChatPane {
    /// The bottom bar's line while a step waits or the compose box is lent.
    pub(super) fn menu_status(&self) -> Option<String> {
        if let Some(p) = &self.pending {
            return Some(match p {
                Pending::ClearView => "clear the view? the bus keeps every message · y clears · any other key keeps it".into(),
                Pending::Leave(c) => format!("leave #{c}? y leaves · any other key stays"),
                Pending::ClearHistory(c) => {
                    format!("clear #{c} history? deletes what every live agent has read, older than 90 s · y clears · any other key keeps it")
                }
                Pending::Delete(c) => format!("delete #{c} and its history? y deletes · any other key keeps it"),
                Pending::Remove(c, m) => format!("remove @{} from #{c}? y removes · any other key keeps them", m.label),
                Pending::Quit => "quit? y quits · any other key stays · Ctrl-C quits at once".into(),
            });
        }
        self.prompt.as_ref().map(|(p, _)| match p {
            Prompt::NewChannel => "new channel: type its name · Enter creates it · Esc cancels".into(),
            Prompt::Describe(c) => format!("describe #{c}: type a line · Enter sets it (empty clears) · Esc cancels"),
        })
    }

    /// Open the menu of the bar's tab `i`: 0 is the `≡` slot, 1 merged,
    /// then the channels in strip order, then the `+` slot.
    pub(super) fn open_tab_menu(&mut self, i: usize) {
        self.dirty = true;
        let names = self.strip_names();
        if i == 0 {
            return self.common_menu();
        }
        if i == names.len() + 2 {
            return self.new_channel();
        }
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
        self.open = Some(Open::Pick { id, title, options: items(channel.as_deref(), joined), multi: false, chosen: Vec::new() });
    }

    fn common_menu(&mut self) {
        let mouse = if self.config.mouse { MOUSE_OFF } else { MOUSE_ON };
        let column = if self.sidebar { HIDE_COLUMN } else { SHOW_COLUMN };
        let options = [THEME, KEYS, mouse, column, SETTINGS].iter().map(|s| s.to_string()).collect();
        self.open = Some(Open::Pick { id: "common".into(), title: "attend-chat".into(), options, multi: false, chosen: Vec::new() });
    }

    fn common_item(&mut self, item: &str) {
        match item {
            THEME => {
                let set = agent_theme::ThemeSet::load(agent_theme::user_dir().as_deref());
                let mut options = vec!["terminal".to_string()];
                options.extend(set.list().map(|(t, _)| t.name.clone()));
                self.open = Some(Open::Pick { id: "theme".into(), title: "theme for this session".into(), options, multi: false, chosen: Vec::new() });
            }
            KEYS => {
                let options = PRESETS.iter().map(|(l, _, _)| l.to_string()).collect();
                self.open = Some(Open::Pick { id: "keys".into(), title: "keybinding set".into(), options, multi: false, chosen: Vec::new() });
            }
            MOUSE_ON | MOUSE_OFF => {
                let on = item == MOUSE_ON;
                self.run("config", SlashOutcome::Config { key: Some("mouse".into()), value: Some(on.to_string()) });
            }
            HIDE_COLUMN | SHOW_COLUMN => self.toggle_sidebar(),
            SETTINGS => {
                self.run("config", SlashOutcome::Config { key: None, value: None });
            }
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
        self.invalidate();
        match warning {
            Some(w) => self.say(format!("theme: {w}"), true),
            None => self.say(format!("theme {name} for this session; `ways settings set theme.active {name}` keeps it"), false),
        }
    }

    /// A menu item, or a submenu's peer, was chosen.
    pub(super) fn menu_picked(&mut self, id: &str, value: &str) {
        self.dirty = true;
        match id {
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
                self.pending = Some(Pending::Remove(c.into(), m));
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
        let mut options = vec![format!("{ADDRESS} @{name}"), WHO.to_string()];
        self.removable.clear();
        if let Tab::Channel(c) = self.normal_tab() {
            if let Some(m) = self.channel_members(&c).into_iter().find(|m| m.id == member) {
                options.push(format!("{REMOVE_FROM} #{c}"));
                self.removable = vec![m];
            }
        }
        self.open = Some(Open::Pick { id: format!("agent:{member}"), title: format!("@{name}"), options, multi: false, chosen: Vec::new() });
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
                self.pending = Some(Pending::Remove(c, m));
            }
        }
    }

    /// Who an agent of the column is: `/whois` for a live claude, and what
    /// the chat knows of the rest, which `/whois` cannot resolve.
    fn whois(&mut self, member: &str) {
        let Some(s) = self.seen_member(member) else {
            return self.say("that agent is not in the column any more", true);
        };
        let name = &s.identity.nickname;
        if s.identity.is_claude && s.live {
            self.run("whois", SlashOutcome::Whois(name.clone()));
            return;
        }
        let said = if s.identity.is_claude {
            format!("@{name} — session {} · root {} · quiet: no heartbeat", s.member, attend_presence::session::normalize_origin(&s.identity.cwd))
        } else {
            format!("@{name} — a human · from {} · {}", s.identity.cwd, if s.live { "here" } else { "away" })
        };
        self.say(said, false);
    }

    /// The members of `channel`, from its membership: live or not.
    fn channel_members(&mut self, channel: &str) -> Vec<Member> {
        let ids = attend_groups::load_groups(&crate::signal::signals_base()).get(channel).map(|e| e.members.clone()).unwrap_or_default();
        let roster = crate::peers::roster(ColorDepth::detect(), &attend_instances::SnapshotCache::new());
        members(&ids, &roster, &self.world().seen)
    }

    fn item(&mut self, channel: &str, item: &str) {
        let chan = || (!channel.is_empty()).then(|| channel.to_string());
        match item {
            CLEAR_VIEW => self.pending = Some(Pending::ClearView),
            CLEAR_HISTORY => self.pending = Some(Pending::ClearHistory(chan().unwrap_or(BASE_CHANNEL_NAME.into()))),
            DELETE => self.pending = chan().map(Pending::Delete),
            LEAVE => self.pending = chan().map(Pending::Leave),
            JOIN => {
                if let Some(c) = chan() {
                    self.run("join", SlashOutcome::Join(c));
                }
            }
            DESCRIBE => self.start_prompt(Prompt::Describe(channel.into())),
            ADD_AGENT => self.say("Add agent enrolls without asking: ADR-404 D1, not built yet; Invite agent ▸ asks them", true),
            INVITE_AGENT => self.peer_menu(channel, true),
            REMOVE_AGENT => self.peer_menu(channel, false),
            _ => {}
        }
    }

    /// The live peers not in `channel` (add), or every member of it, live
    /// or quiet (remove): a quiet member is removed by its membership id.
    fn peer_menu(&mut self, channel: &str, add: bool) {
        let options: Vec<String> = if add {
            let members = attend_groups::load_groups(&crate::signal::signals_base()).get(channel).map(|e| e.members.clone()).unwrap_or_default();
            let roster = crate::peers::roster(ColorDepth::detect(), &attend_instances::SnapshotCache::new());
            roster.iter().filter(|p| !members.contains(&p.session_id)).map(|p| p.display.clone()).collect()
        } else {
            self.removable = self.channel_members(channel);
            self.removable.iter().map(Member::option).collect()
        };
        if options.is_empty() {
            let what = if add { "no live agent outside" } else { "no member in" };
            return self.say(format!("{what} #{channel}"), true);
        }
        let (id, title) = if add { (format!("invite:{channel}"), format!("invite to #{channel}")) } else { (format!("remove:{channel}"), format!("remove from #{channel}")) };
        self.open = Some(Open::Pick { id, title, options, multi: false, chosen: Vec::new() });
    }

    /// Ask for a new channel's name in the compose box: from the `+` slot
    /// or Ctrl+N. A waiting question is dropped; the name already being
    /// typed is left alone, so a repeat cannot take the name for the draft.
    /// A description being typed is never thrown away: the request waits
    /// until it is sent or cancelled.
    pub(super) fn new_channel(&mut self) {
        self.dirty = true;
        self.pending = None;
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

    /// Esc while the compose box is lent: give it back unchanged.
    pub(super) fn prompt_cancel(&mut self) {
        self.end_prompt();
        self.say("cancelled", false);
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

    /// The shell took a key or a click: a waiting question is dropped.
    pub(super) fn drop_question(&mut self) {
        if let Some(p) = self.pending.take() {
            self.say(if p == Pending::Quit { "stayed" } else { "kept" }, false);
        }
    }

    /// A key while a step waits: `y` goes ahead, any other key keeps
    /// things as they are. True when the screen should quit.
    pub(super) fn answer(&mut self, yes: bool) -> bool {
        let Some(p) = self.pending.take() else { return false };
        self.dirty = true;
        if !yes {
            self.say(if p == Pending::Quit { "stayed" } else { "kept" }, false);
            return false;
        }
        match p {
            Pending::Quit => return true,
            Pending::ClearView => {
                self.run("clear", SlashOutcome::ClearTranscript);
            }
            Pending::Leave(c) => {
                self.run("leave", SlashOutcome::Leave(Some(c)));
            }
            Pending::ClearHistory(c) => {
                self.run("purge", SlashOutcome::Purge(Some(c)));
            }
            Pending::Delete(c) => {
                self.run("dissolve", SlashOutcome::Dissolve(Some(c)));
            }
            Pending::Remove(c, m) => {
                if self.dry_run {
                    self.say("dry run: /kick not run", false);
                } else {
                    self.apply(keys::kick_member(&c, &m.id, &m.label, m.root.as_deref()));
                }
            }
        }
        false
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
        assert_eq!(items(None, false), [CLEAR_VIEW]);
        assert_eq!(items(Some("open"), true), [CLEAR_VIEW, CLEAR_HISTORY], "the harmless item first");
        assert_eq!(items(Some("deploy"), true), [ADD_AGENT, INVITE_AGENT, REMOVE_AGENT, DESCRIBE, CLEAR_HISTORY, LEAVE, DELETE]);
        assert_eq!(items(Some("deploy"), false), [ADD_AGENT, INVITE_AGENT, REMOVE_AGENT, DESCRIBE, CLEAR_HISTORY, JOIN, DELETE], "Join to a non-member");
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
            assert!(c.menu_status().is_some_and(|s| s.contains(ask)), "{item}: {:?}", c.menu_status());
            assert_eq!(c.status, "", "{item} ran nothing yet");
            c.answer(false);
            assert_eq!(c.status, "kept");
            c.status.clear();
        }
        c.item("deploy", LEAVE);
        c.answer(true);
        assert_eq!(c.status, "dry run: /leave not run", "y goes ahead");
    }

    /// A pane whose world is `seen`, shown on merged.
    fn pane_with(seen: Vec<Seen>) -> ChatPane {
        let mut c = ChatPane::new(None, Default::default());
        c.dry_run = true;
        let known = crate::chip::known_of(&seen);
        c.world = Some(super::super::World { groups: Vec::new(), known, seen, rows: Vec::new(), live_members: Vec::new(), instances: attend_instances::SnapshotCache::new() });
        c
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
