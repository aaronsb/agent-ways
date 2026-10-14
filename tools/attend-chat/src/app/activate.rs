//! One activation path. Every input on something the chat can act on
//! reaches [`ChatPane::activate`]: a click on a tab or its digit, a click
//! on a column row or Enter on it, a right click on any of those, on a
//! sender's name in the feed or on the compose box's destination flag.
//!
//! - Select (a left click, a digit, Enter in the column) shows a channel;
//!   on the channel shown, it opens the menu. The tab bar's inputs open it
//!   on a repeat only when `attend.chat.tabs.menu_on_repeat` says so; the
//!   column's always do (ADR-407). A channel with no tab cannot be shown, so
//!   its menu opens at once.
//! - Menu (a right click, Enter on the focused tab bar, the destination
//!   flag) opens the menu at once and shows nothing. The flag acts on the
//!   channel it names: `#open` on merged.
//! - A claude's row selects its DM view (ADR-405, amendment of
//!   2026-10-13): the view is listed under the row and shown; on either of
//!   its DM views shown, the agent's menu opens. Each view's own row and tab
//!   select the same way, and their menu is the view's. A human has no DM
//!   view: either input opens its menu.
//! - `≡` opens the common menu; `+` asks a new channel's name in an entry
//!   dialog docked under it. The compose box is never borrowed (ADR-409).

use agent_tui::ratatui::layout::{Position, Rect};

use super::dm::Kind;
use super::sidebar::SideHit;
use super::ChatPane;
use crate::tabs::{Slot, Tab};

/// What an input acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Target {
    /// The `≡` slot.
    Common,
    /// The `+` slot.
    NewChannel,
    Merged,
    Channel(String),
    /// An agent, by its membership id.
    Agent(String),
    /// One of an agent's DM views, by the agent's membership id.
    View(super::dm::Kind, String),
}

/// How it acts: select, which on the target shown opens its menu when
/// `repeat` allows; or the menu at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum How {
    Select { repeat: bool },
    Menu,
}

impl Target {
    /// The target a column row stands for.
    pub(super) fn of(hit: SideHit) -> Target {
        match hit {
            SideHit::Agent(member) => Target::Agent(member),
            SideHit::Dm(member) => Target::View(Kind::Dm, member),
            SideHit::Review(member) => Target::View(Kind::Review, member),
            SideHit::Channel(name) => Target::Channel(name),
        }
    }

    /// The target a slot of the tab bar stands for.
    pub(super) fn of_slot(slot: &Slot) -> Target {
        match slot {
            Slot::Common => Target::Common,
            Slot::New => Target::NewChannel,
            Slot::Tab(Tab::Merged) => Target::Merged,
            Slot::Tab(Tab::Dm(member)) => Target::View(Kind::Dm, member.clone()),
            Slot::Tab(Tab::Review(member)) => Target::View(Kind::Review, member.clone()),
            Slot::Tab(Tab::Channel(c)) => Target::Channel(c.clone()),
        }
    }
}

impl ChatPane {
    /// Act on `target` as `how` says. A menu docks under `at` when given,
    /// else under the target's column row, else its tab.
    pub(super) fn activate(&mut self, target: Target, how: How, at: Option<Rect>) {
        self.dirty = true;
        match target {
            Target::Common => self.common_menu(),
            Target::NewChannel => self.new_channel(),
            Target::Agent(member) => {
                let tab = self.normal_tab();
                let shown = [Kind::Dm, Kind::Review].iter().any(|k| tab == k.tab(&member));
                match how {
                    How::Select { .. } if !shown && self.has_dms(&member) => self.open_dm(member),
                    _ => self.agent_menu(member, at),
                }
            }
            Target::View(kind, member) => {
                let shown = self.normal_tab() == kind.tab(&member);
                match how {
                    How::Select { repeat } if !(shown && repeat) => self.open_view(kind, member),
                    _ => self.view_menu(kind, member, at),
                }
            }
            Target::Merged | Target::Channel(_) => {
                let channel = match target {
                    Target::Channel(c) => Some(c),
                    _ => None,
                };
                let tab = channel.clone().map_or(Tab::Merged, Tab::Channel);
                let has_tab = channel.as_ref().is_none_or(|c| self.strip_names().contains(c));
                let shown = self.normal_tab() == tab;
                let menu = match how {
                    How::Menu => true,
                    How::Select { repeat } => !has_tab || (shown && repeat),
                };
                if menu {
                    self.channel_menu(channel, at);
                } else {
                    self.set_tab(tab);
                }
            }
        }
    }

    /// The destination flag clicked: the menu of what it names, the shown
    /// tab's send scope (`#open` on merged), or in a DM view its agent,
    /// docked at the flag. A clicked element acts on what it shows.
    pub(super) fn flag_menu(&mut self) {
        let flag = self.hits.flag;
        let target = match self.normal_tab() {
            Tab::Dm(member) | Tab::Review(member) => Target::Agent(member),
            tab => Target::Channel(crate::tabs::send_scope(&tab)),
        };
        self.activate(target, How::Menu, Some(flag));
    }

    /// A right click at `at`: the menu of what is under it, at once. A
    /// column row, the destination flag, or a sender's name in the feed.
    pub(super) fn context_at(&mut self, at: Position) {
        if let Some((_, hit)) = self.hits.side.iter().find(|(r, _)| r.contains(at)).cloned() {
            return self.activate(Target::of(hit), How::Menu, None);
        }
        if self.hits.flag.contains(at) {
            return self.flag_menu();
        }
        if let Some(member) = self.sender_at(at) {
            let row = Rect { x: self.hits.feed.x, y: at.y, width: self.hits.feed.width, height: 1 };
            self.activate(Target::Agent(member), How::Menu, Some(row));
        }
    }

    /// The membership id of the sender whose header band is at `at` in the
    /// feed: the line over a message.
    fn sender_at(&self, at: Position) -> Option<String> {
        let feed = self.hits.feed;
        if !feed.contains(at) {
            return None;
        }
        let (i, row) = self.feed.entry_row(at.y - feed.y)?;
        if row != 0 {
            return None;
        }
        self.entry_members.get(i).cloned().flatten()
    }
}
