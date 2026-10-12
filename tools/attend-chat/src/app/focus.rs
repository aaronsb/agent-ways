//! One focus owner (ADR-405, #931 U5): the keyboard is on one element at a
//! time, the tab bar's tabs, the column's rows, the feed or the compose box,
//! and that element alone is drawn focused.
//!
//! The ring is the one ordered list the key handler walks and the renderer
//! marks: the tab bar in the order drawn (`≡`, the
//! created channels, `+`, then the reserved `#open` and the operator view),
//! the column's rows (every agent, then every channel), the feed, then the
//! compose box, and round again.
//!
//! - In the compose box every key edits: Tab completes, Enter sends, the
//!   arrows move the cursor. Shift+Tab leaves backward, as does ↑ at the
//!   start of the first line; ↓ at the end of the last line leaves forward.
//! - Outside it, Tab or → moves forward and Shift+Tab or ← back; ↑ and ↓
//!   move within the column and the feed; Enter opens the focused element's
//!   menu, the one a click opens ([`ChatPane::activate`]); Esc goes back to
//!   the compose box. The shell's single keys work as the help names them
//!   (`?`, `q`, `m`, the digits), since nothing is being typed; any other
//!   character goes to the compose box and is typed there.
//!
//! These rules are [`ix::step`], a pure function over the one interaction
//! state ([`ix::Ix`]); [`ChatPane::dispatch`] is where every input that can
//! move the focus goes, and carries out what the step asks.

use agent_tui::ratatui::layout::Rect;
use agent_tui::{Anchor, Binding, Open};

use super::activate::{How, Target};
use super::ix::{self, Effect, Ev, Laid, View};
use super::ChatPane;
use crate::tabs::{self, Slot, Tab};

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
        match self.ix.focus.clone() {
            Focus::Tab(slot) => match slot {
                Slot::Common => Kind::Common,
                Slot::New => Kind::New,
                Slot::Tab(Tab::Merged) => Kind::View,
                Slot::Tab(_) => Kind::ChannelTab,
            },
            Focus::Row(i) => match world {
                Some(w) if i < w.seen.len() => Kind::AgentRow,
                _ => Kind::ChannelRow,
            },
            Focus::Feed => Kind::Feed,
            Focus::Compose => Kind::Compose,
        }
    }

    /// The tab bar's index of the slot showing `tab`.
    pub(super) fn slot_of(&mut self, tab: &Tab) -> Option<usize> {
        let slot = Slot::Tab(tab.clone());
        tabs::bar(&self.strip_names()).iter().position(|s| *s == slot)
    }

    /// What the step reads of the screen: the tab bar and the column when
    /// the world is read, never reading it ([`ix::NeedWorld`]).
    fn ix_view(&self) -> View {
        let world = self.world.as_ref().map(|w| {
            let names = tabs::strip_names(&w.groups);
            Laid { bar: tabs::bar(&names), rows: super::sidebar::side_items_of(w), shown: tabs::normalize(self.foreground.clone(), &names) }
        });
        let cursor = self.input.cursor();
        View {
            world,
            side_drawn: self.side_drawn(),
            draft_empty: self.input.is_empty(),
            caret_start: cursor == 0,
            caret_end: cursor == self.input.text().chars().count(),
            feed_len: self.feed.len(),
            feed_selected: self.feed.selected,
            menu_on_repeat: self.config.menu_on_repeat,
        }
    }

    /// Move the interaction state by `ev` ([`ix::step`]) and carry out what
    /// the step asks, in order. A step that needs the world reads it first,
    /// which refocuses, then steps again.
    pub(super) fn dispatch(&mut self, ev: Ev) {
        let (ix, effects) = loop {
            match ix::step(&self.ix, ev.clone(), &self.ix_view()) {
                Ok(done) => break done,
                Err(ix::NeedWorld) => {
                    self.world();
                }
            }
        };
        self.ix = ix;
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
            Effect::Complete => self.tab_key(),
            Effect::Clear => self.clear(),
            Effect::Page { up } => self.page(up),
            Effect::ShowTab(t) | Effect::RestoreTab(t) => self.set_tab(t),
            Effect::Follow => self.side_follow = true,
            Effect::Activate { target, how, under_tab } => {
                self.activate(target, how, None);
                if let (Some(i), Some(Open::Pick { anchor, .. } | Open::Entry { anchor, .. })) = (under_tab, &mut self.open) {
                    *anchor = Anchor::Tab(i);
                }
            }
            Effect::FeedMenu => self.feed_menu(),
            Effect::Reveal(i) => self.feed.reveal(i),
            Effect::Deselect => self.feed.selected = None,
            Effect::Redraw => self.dirty = true,
        }
    }

    /// Enter on the feed: the menu of the selected message's sender, docked
    /// under its header, as a right click on the sender's name opens it.
    /// With no message selected, the newest is selected first.
    fn feed_menu(&mut self) {
        let n = self.feed.len();
        let Some(i) = self.feed.selected.or(n.checked_sub(1)) else {
            return self.say("no message to act on", false);
        };
        self.feed.reveal(i);
        let Some(member) = self.entry_members.get(i).cloned().flatten() else {
            return self.say("that message has no sender to act on", false);
        };
        let feed = self.hits.feed;
        let at = self.feed.header_row(i).map(|r| Rect { x: feed.x, y: feed.y + r, width: feed.width, height: 1 });
        self.activate(Target::Agent(member), How::Menu, at);
    }

    /// The chat's keys, the same keys wherever the focus is: those the
    /// focused element takes are on the footer, first and in its words; the
    /// rest are for the key help. Each key is bound once.
    pub(super) fn focus_bindings(&self) -> Vec<Binding> {
        let kind = self.focus_kind();
        let on = |b: Binding, footer: bool| Binding { footer, ..b };
        let list = matches!(kind, Kind::AgentRow | Kind::ChannelRow | Kind::Feed);
        let mut out = Vec::new();
        if kind == Kind::Compose {
            out.extend([
                Binding::new("Enter", "send"),
                Binding::new("Tab", "complete"),
                Binding::new("S-Tab", "or ↑ at the top leaves the box").short("or ↑ leaves"),
                Binding::help("← →", "move the cursor; outside the box, the element before or next"),
                Binding::help("↑ ↓", "between the box's lines; outside it, a column row or a feed message"),
                Binding::help("Esc", "clears the selection and the bar; outside the box, back to it"),
            ]);
        } else {
            let enter = match kind {
                Kind::Common => "the menu",
                Kind::New => "new channel",
                Kind::Feed => "sender's menu",
                _ => "its menu",
            };
            out.extend([
                Binding::new("Enter", enter),
                on(Binding::new("↑ ↓", if kind == Kind::Feed { "select" } else { "move" }), list),
                Binding::new("Tab", "next"),
                Binding::new("S-Tab", "back"),
                Binding::new("Esc", "the box"),
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
            Kind::AgentRow | Kind::ChannelRow => "column",
            Kind::Feed => "feed",
        }
        .into()
    }
}
