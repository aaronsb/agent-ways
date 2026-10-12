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
//!   flag) opens the menu at once and shows nothing.
//! - An agent has no shown state: either opens its menu (ADR-405).
//! - `≡` opens the common menu over a lent compose box; `+` lends the box.
//!   Every other activation gives a lent box back first (ADR-409 (d)).

use agent_tui::ratatui::layout::{Position, Rect};

use super::sidebar::SideHit;
use super::ChatPane;
use crate::chip::CHIP_WIDTH;
use crate::tabs::Tab;

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
            SideHit::Channel(name) => Target::Channel(name),
        }
    }
}

impl ChatPane {
    /// The tab bar's slot `i`: 0 is `≡`, 1 merged, then the channels in
    /// strip order, then `+`.
    pub(super) fn tab_target(&mut self, i: usize) -> Option<Target> {
        let names = self.strip_names();
        match i {
            0 => Some(Target::Common),
            1 => Some(Target::Merged),
            i if i == names.len() + 2 => Some(Target::NewChannel),
            i => names.get(i - 2).map(|n| Target::Channel(n.clone())),
        }
    }

    /// The channel shown, as a target.
    pub(super) fn shown_target(&mut self) -> Target {
        match self.normal_tab() {
            Tab::Merged => Target::Merged,
            Tab::Channel(c) => Target::Channel(c),
        }
    }

    /// Act on `target` as `how` says. A menu docks under `at` when given,
    /// else under the target's column row, else its tab.
    pub(super) fn activate(&mut self, target: Target, how: How, at: Option<Rect>) {
        self.dirty = true;
        match target {
            Target::Common => self.common_menu(),
            Target::NewChannel => self.new_channel(),
            Target::Agent(member) => {
                self.prompt_cancel();
                self.agent_menu(member, at);
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
                self.prompt_cancel();
                if menu {
                    self.channel_menu(channel, at);
                } else {
                    self.set_tab(tab);
                }
            }
        }
    }

    /// The destination flag clicked: the shown channel's menu, docked at
    /// the flag.
    pub(super) fn flag_menu(&mut self) {
        let target = self.shown_target();
        let flag = self.hits.flag;
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
            let row = Rect { x: self.hits.feed.x, y: at.y, width: CHIP_WIDTH, height: 1 };
            self.activate(Target::Agent(member), How::Menu, Some(row));
        }
    }

    /// The membership id of the sender whose name box is at `at` in the
    /// feed: the label left of a message.
    fn sender_at(&self, at: Position) -> Option<String> {
        let feed = self.hits.feed;
        if !feed.contains(at) || at.x >= feed.x + CHIP_WIDTH {
            return None;
        }
        let i = self.feed.entry_at(at.y - feed.y)?;
        self.entry_members.get(i).cloned().flatten()
    }
}
