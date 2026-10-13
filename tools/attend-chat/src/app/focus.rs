//! One focus owner (ADR-405, #931 U5): the keyboard is on one element at a
//! time, the tab bar's tabs, the column's rows, the feed or the compose box,
//! and that element alone is drawn focused.
//!
//! The ring is the one ordered list the key handler walks and the renderer
//! marks: the tab bar in the order drawn (`≡`, the created channel or DM
//! view shown, `+`, then the reserved `#open` and the operator view), the
//! column's rows (every agent, each followed by its DM view's row while
//! listed, then every channel), the feed, then the compose box, and round
//! again. Tab walks it forward and Shift+Tab back, exact mirrors.
//!
//! - In the compose box every key edits: Enter sends, the arrows move the
//!   cursor, and Tab completes a word that can be completed. Tab with no
//!   such word leaves forward and Shift+Tab backward; ↑ at the start of the
//!   first line and ↓ at the end of the last leave a box that holds text.
//! - The feed is the chat too: a character typed there goes to the compose
//!   box, and `q`, `m`, `?` and the digits are typed, never the shell's.
//! - Outside the chat, on the tab bar and the column, Tab or → moves forward
//!   and Shift+Tab or ← back; ↑ and ↓ move within the column and the feed;
//!   Enter opens the focused element's menu, the one a right click opens
//!   ([`ChatPane::activate`]), and on the feed the selected message's menu.
//!   The shell's single keys work there as the help names them (`?`, `q`,
//!   `m`), and on a channel's row the digits pick a channel by its number;
//!   any other character, a digit on an agent's row among them, goes to the
//!   compose box and is typed there.
//! - Esc moves along one cycle: the box, the feed, the `≡` menu, the tab or
//!   column row the focus was last on (the feed when there is none), the
//!   `≡` menu again. Tab out of the box onto `≡` is not remembered, so the
//!   cycle comes back to the tab or row focused before. An overlay that is
//!   open takes the first Esc.
//!
//! These rules are [`ix::step`], a pure function over the one interaction
//! state ([`ix::Ix`]); [`ChatPane::dispatch`] is where every input that can
//! move the focus goes, and carries out what the step asks.

use agent_tui::{Anchor, Binding, Open};

use super::ix::{self, Effect, Ev, View};
use super::sidebar::SideHit;
use super::ChatPane;
use crate::tabs::{Slot, Tab};

/// Where the keyboard is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) enum Focus {
    /// A slot of the tab bar ([`tabs::bar`]), kept by what it is, so a
    /// channel created or dissolved under the focus leaves it on the same
    /// slot ([`ChatPane::refocus`]).
    Tab(Slot),
    /// The column's row `i`, agents then channels ([`ChatPane::side_items`]).
    Row(usize),
    Feed,
    #[default]
    Compose,
}

/// What the focused element is, for the footer and the mode lozenge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Common,
    New,
    ChannelTab,
    View,
    AgentRow,
    DmRow,
    ChannelRow,
    Feed,
    Compose,
}

impl ChatPane {
    /// The column's row under the focus, if it is there.
    pub(super) fn side_focus(&self) -> Option<usize> {
        self.ix.side_focus()
    }

    /// What the focused element is, from the world as last read.
    pub(super) fn focus_kind(&self) -> Kind {
        let world = self.world.as_ref();
        match self.ix.focus().clone() {
            Focus::Tab(slot) => match slot {
                Slot::Common => Kind::Common,
                Slot::New => Kind::New,
                Slot::Tab(Tab::Merged | Tab::Dm(_)) => Kind::View,
                Slot::Tab(_) => Kind::ChannelTab,
            },
            Focus::Row(i) => match world.and_then(|w| w.laid.rows.get(i)) {
                Some(SideHit::Agent(_)) => Kind::AgentRow,
                Some(SideHit::Dm(_)) => Kind::DmRow,
                _ => Kind::ChannelRow,
            },
            Focus::Feed => Kind::Feed,
            Focus::Compose => Kind::Compose,
        }
    }

    /// The tab bar as the world lays it out, for the tab shown.
    pub(super) fn bar(&mut self) -> Vec<Slot> {
        self.world().laid.bar.clone()
    }

    /// The tab bar's index of the slot showing `tab`.
    pub(super) fn slot_of(&mut self, tab: &Tab) -> Option<usize> {
        let slot = Slot::Tab(tab.clone());
        self.bar().iter().position(|s| *s == slot)
    }

    /// Whether Tab would change the compose box ([`super::keys::completable`]),
    /// over the names the column lists; with the world unread, none.
    fn completable(&self) -> bool {
        let (agents, channels) = self.world.as_ref().map(|w| super::sidebar::completion_pool(&w.seen, &w.rows)).unwrap_or_default();
        super::keys::completable(self.input.text(), self.input.cursor(), self.tab_cycle.as_ref(), &agents, &channels)
    }

    /// What the step reads of the screen: the tab bar and the column as the
    /// world laid them out when it was read, never reading it
    /// ([`ix::NeedWorld`]).
    fn ix_view(&self) -> View {
        let cursor = self.input.cursor();
        View {
            world: self.world.as_ref().map(|w| w.laid.clone()),
            side_drawn: self.side_drawn(),
            draft_empty: self.input.is_empty(),
            completable: self.completable(),
            caret_start: cursor == 0,
            caret_end: cursor == self.input.text().chars().count(),
            feed_len: self.feed.len(),
            feed_selected: self.feed.selected,
            menu_on_repeat: self.config.menu_on_repeat,
        }
    }

    /// Move the interaction state by `ev` ([`ix::Machine::step`]) and carry
    /// out what the step asks, in order. A step that needs the world reads
    /// it, which refocuses, then steps once more, with the world in the
    /// view. The only writer of the interaction state.
    pub(super) fn dispatch(&mut self, ev: Ev) {
        let effects = match self.ix.step(ev.clone(), &self.ix_view()) {
            Ok(fx) => fx,
            Err(ix::NeedWorld) => {
                self.world();
                self.ix.step(ev, &self.ix_view()).expect("the world was just read")
            }
        };
        for e in effects {
            self.apply_effect(e);
        }
    }

    /// After the world is read again: the focus follows its row or its tab.
    pub(super) fn refocus(&mut self) {
        self.dispatch(Ev::WorldChanged);
    }

    fn apply_effect(&mut self, e: Effect) {
        match e {
            Effect::Type(k) => {
                self.send_anyway = None;
                self.input.key(k);
            }
            Effect::Caret { up } => {
                let width = self.hits.text_width;
                if up {
                    self.input.up(width);
                } else {
                    self.input.down(width);
                }
            }
            Effect::Send => self.enter(),
            Effect::SendNow => self.send(true),
            Effect::Complete => {
                self.send_anyway = None;
                self.tab_key();
            }
            Effect::Clear => {
                self.send_anyway = None;
                self.clear();
            }
            Effect::Page { up } => self.page(up),
            Effect::ShowTab(t) => self.set_tab(t),
            Effect::Follow => self.side_follow = true,
            Effect::Activate { target, how, under_tab } => {
                self.activate(target, how, None);
                if let (Some(i), Some(Open::Pick { anchor, .. } | Open::Entry { anchor, .. })) = (under_tab, &mut self.to_open) {
                    *anchor = Anchor::Tab(i);
                }
            }
            Effect::FeedMenu => self.message_menu(),
            Effect::Reveal(i) => self.feed.reveal(i),
            Effect::Deselect => self.feed.selected = None,
            Effect::Redraw => self.dirty = true,
        }
    }

    /// The chat's keys, the same keys wherever the focus is: those the
    /// focused element takes are on the footer, first and in its words; the
    /// rest are for the key help. Each key is bound once.
    pub(super) fn focus_bindings(&self) -> Vec<Binding> {
        let kind = self.focus_kind();
        let on = |b: Binding, footer: bool| Binding { footer, ..b };
        let list = matches!(kind, Kind::AgentRow | Kind::DmRow | Kind::ChannelRow | Kind::Feed);
        let mut out = Vec::new();
        if kind == Kind::Compose {
            out.extend([
                Binding::new("Enter", "send"),
                Binding::new("Tab", "complete, else next").short("complete"),
                Binding::new("S-Tab", "or ↑ at the top leaves the box").short("or ↑ leaves"),
                Binding::new("Esc", "the feed"),
                Binding::help("← →", "move the cursor; outside the box, the element before or next"),
                Binding::help("↑ ↓", "between the box's lines; outside it, a column row or a feed message"),
            ]);
            // Claude Code's `chat:sendNow`; Enter again after a quiet
            // refusal does the same on any terminal.
            if self.enhanced {
                out.push(Binding::help("C-Enter", "send now, to a quiet agent too"));
            }
        } else {
            let enter = match kind {
                Kind::Common => "the menu",
                Kind::New => "new channel",
                Kind::Feed => "message menu",
                _ => "its menu",
            };
            out.extend([
                Binding::new("Enter", enter),
                on(Binding::new("↑ ↓", if kind == Kind::Feed { "select" } else { "move" }), list),
                Binding::new("Tab", "next"),
                Binding::new("S-Tab", "back"),
                Binding::new("Esc", "≡ menu"),
                Binding::help("← →", "the element before or next; in the box, the cursor"),
            ]);
        }
        out.extend([
            Binding::help("PgUp PgDn", "scroll the feed"),
            Binding::help("S-Enter M-Enter", "new line"),
            Binding::help("Home End Bksp Del", "edit in the box; Home and End jump in lists"),
            Binding::help("click a message", "selects it; a chip completes; the compose box: the cursor"),
        ]);
        if let Some(key) = self.sidebar_key_label() {
            out.push(Binding::help(key, "show or hide the agents and channels column"));
        }
        out
    }

    /// The bottom bar's lozenge: where the keyboard is.
    pub(super) fn focus_mode(&self) -> String {
        match self.focus_kind() {
            Kind::Compose => "chat",
            Kind::Common | Kind::New | Kind::ChannelTab => "tabs",
            Kind::View => "view",
            Kind::AgentRow | Kind::DmRow | Kind::ChannelRow => "column",
            Kind::Feed => "feed",
        }
        .into()
    }
}
