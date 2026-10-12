//! One focus owner (ADR-405, #931 U5): the keyboard is on one element at a
//! time, the tab bar's tabs, the column's rows, the feed or the compose box,
//! and that element alone is drawn focused.
//!
//! The ring is the one ordered list the key handler walks and the renderer
//! marks ([`ChatPane::ring`]): the tab bar in the order drawn (`≡`, the
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

use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use agent_tui::ratatui::layout::Rect;
use agent_tui::{Anchor, Binding, Open};

use super::activate::{How, Target};
use super::sidebar::SideHit;
use super::ChatPane;
use crate::tabs::{self, Slot, Tab};

/// Where the keyboard is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) enum Focus {
    /// The tab bar's slot `i`, in [`tabs::bar`]'s order.
    Tab(usize),
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
    /// The ring of focusable elements, in order: the tab bar's slots, the
    /// column's rows while it is drawn, the feed, the compose box.
    pub(super) fn ring(&mut self) -> Vec<Focus> {
        let slots = tabs::bar(&self.strip_names()).len();
        let mut out: Vec<Focus> = (0..slots).map(Focus::Tab).collect();
        if self.side_drawn() {
            out.extend((0..self.side_items().len()).map(Focus::Row));
        }
        out.extend([Focus::Feed, Focus::Compose]);
        out
    }

    /// The column's row under the focus, if it is there.
    pub(super) fn side_focus(&self) -> Option<usize> {
        match self.focus {
            Focus::Row(i) => Some(i),
            _ => None,
        }
    }

    /// What the focused element is, from the world as last read.
    pub(super) fn focus_kind(&self) -> Kind {
        let world = self.world.as_ref();
        match self.focus.clone() {
            Focus::Tab(i) => match tabs::bar(&world.map(|w| tabs::strip_names(&w.groups)).unwrap_or_default()).get(i) {
                Some(Slot::Common) => Kind::Common,
                Some(Slot::New) => Kind::New,
                Some(Slot::Tab(Tab::Merged)) => Kind::View,
                _ => Kind::ChannelTab,
            },
            Focus::Row(i) => match world {
                Some(w) if i < w.seen.len() => Kind::AgentRow,
                _ => Kind::ChannelRow,
            },
            Focus::Feed => Kind::Feed,
            Focus::Compose => Kind::Compose,
        }
    }

    /// Put the focus on `f`. Landing on a tab shows it, as moving along the
    /// tab bar always has; a slot (`≡`, `+`) is only pointed at.
    pub(super) fn focus_on(&mut self, f: Focus) {
        match &f {
            Focus::Tab(i) => {
                if let Some(Slot::Tab(t)) = tabs::bar(&self.strip_names()).get(*i).cloned() {
                    self.set_tab(t);
                }
            }
            Focus::Row(i) => {
                let items = self.side_items();
                self.side_focus_hit = items.get(*i).cloned();
                self.side_follow = true;
            }
            Focus::Feed | Focus::Compose => {}
        }
        self.focus = f;
        self.dirty = true;
    }

    /// One step along the ring, forward or back, wrapping.
    pub(super) fn step(&mut self, forward: bool) {
        let ring = self.ring();
        let n = ring.len();
        let at = ring.iter().position(|f| *f == self.focus).unwrap_or(n - 1);
        let to = if forward { (at + 1) % n } else { (at + n - 1) % n };
        self.focus_on(ring[to].clone());
    }

    /// The tab bar's slot showing `tab`.
    pub(super) fn slot_of(&mut self, tab: &Tab) -> Option<usize> {
        tabs::bar(&self.strip_names()).iter().position(|s| *s == Slot::Tab(tab.clone()))
    }

    /// F2 or Ctrl+T: the focus to the tab shown, or from the tab bar back to
    /// the compose box.
    pub(super) fn toggle_tabs(&mut self) {
        if matches!(self.focus, Focus::Tab(_)) {
            return self.focus_on(Focus::Compose);
        }
        let shown = self.normal_tab();
        let at = self.slot_of(&shown).unwrap_or(0);
        self.focus_on(Focus::Tab(at));
    }

    /// A key while the compose box has the focus: every key edits, but for
    /// the ways out (Shift+Tab, ↑ at the very top, ↓ at the very end).
    pub(super) fn compose_key(&mut self, k: KeyEvent) {
        let m = k.modifiers;
        let width = self.hits.text_width;
        match k.code {
            KeyCode::Esc => self.clear(),
            KeyCode::Enter if !m.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) => self.enter(),
            KeyCode::Tab => self.tab_key(),
            KeyCode::BackTab => self.step(false),
            KeyCode::Up => {
                if !self.input.up(width) {
                    self.step(false);
                }
            }
            KeyCode::Down => {
                if !self.input.down(width) {
                    self.step(true);
                }
            }
            KeyCode::PageUp => self.page(true),
            KeyCode::PageDown => self.page(false),
            _ => {
                self.input.key(k);
            }
        }
    }

    /// A key while the focus is outside the compose box.
    pub(super) fn nav_key(&mut self, k: KeyEvent) {
        let plain = !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        match k.code {
            KeyCode::Tab | KeyCode::Right => self.step(true),
            KeyCode::BackTab | KeyCode::Left => self.step(false),
            KeyCode::Up => self.nav_vertical(false),
            KeyCode::Down => self.nav_vertical(true),
            KeyCode::Home | KeyCode::End => self.nav_end(k.code == KeyCode::End),
            KeyCode::Enter => self.activate_focus(),
            KeyCode::Esc => self.focus_on(super::focus::Focus::Compose),
            KeyCode::PageUp => self.page(true),
            KeyCode::PageDown => self.page(false),
            // Anything else is typing: it goes to the compose box.
            KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Delete if plain => {
                self.focus_on(Focus::Compose);
                self.compose_key(k);
            }
            _ => {}
        }
    }

    /// ↑ or ↓ outside the compose box: along the column, through the feed's
    /// messages, from the tab bar down into the column.
    fn nav_vertical(&mut self, down: bool) {
        match self.focus.clone() {
            Focus::Tab(i) if down => {
                // Into the column: the row of the tab's channel, else the
                // first row; past a hidden column, the feed.
                let tab = match tabs::bar(&self.strip_names()).get(i) {
                    Some(Slot::Tab(Tab::Channel(c))) => Some(c.clone()),
                    _ => None,
                };
                if !self.side_drawn() || self.side_items().is_empty() {
                    return self.focus_on(Focus::Feed);
                }
                let items = self.side_items();
                let row = tab.and_then(|c| items.iter().position(|h| *h == SideHit::Channel(c.clone()))).unwrap_or(0);
                self.focus_on(Focus::Row(row));
            }
            Focus::Tab(_) => {}
            Focus::Row(i) => {
                let last = self.side_items().len().saturating_sub(1);
                match (down, i) {
                    (false, 0) => {
                        let shown = self.normal_tab();
                        let at = self.slot_of(&shown).unwrap_or(0);
                        self.focus_on(Focus::Tab(at));
                    }
                    (false, i) => self.focus_on(Focus::Row(i - 1)),
                    (true, i) if i >= last => self.focus_on(Focus::Feed),
                    (true, i) => self.focus_on(Focus::Row(i + 1)),
                }
            }
            Focus::Feed => self.feed_move(down),
            Focus::Compose => {}
        }
    }

    /// Home or End outside the compose box: the column's first or last row,
    /// the feed's oldest or newest message.
    fn nav_end(&mut self, end: bool) {
        match self.focus {
            Focus::Row(_) => {
                let last = self.side_items().len().saturating_sub(1);
                self.focus_on(Focus::Row(if end { last } else { 0 }));
            }
            Focus::Feed => {
                let n = self.feed.len();
                if n > 0 {
                    self.feed.reveal(if end { n - 1 } else { 0 });
                    self.dirty = true;
                }
            }
            _ => {}
        }
    }

    /// ↑ or ↓ in the feed: the selection moves to the older or newer
    /// message, starting from the newest. Past the newest, the compose box.
    fn feed_move(&mut self, down: bool) {
        let n = self.feed.len();
        if n == 0 {
            if down {
                self.focus_on(Focus::Compose);
            }
            return;
        }
        let to = match (self.feed.selected, down) {
            (None, false) => Some(n - 1),
            (None, true) => None,
            (Some(i), false) => Some(i.saturating_sub(1)),
            (Some(i), true) if i + 1 < n => Some(i + 1),
            (Some(_), true) => None,
        };
        match to {
            Some(i) => self.feed.reveal(i),
            None => {
                self.feed.selected = None;
                self.focus_on(Focus::Compose);
            }
        }
        self.dirty = true;
    }

    /// Enter outside the compose box: the focused element's menu, the one a
    /// click (or a right click) on it opens, through the one activation
    /// path. A menu from the tab bar docks under its tab.
    pub(super) fn activate_focus(&mut self) {
        match self.focus.clone() {
            Focus::Tab(i) => {
                let Some(target) = self.tab_target(i) else { return };
                self.activate(target, How::Menu, None);
                if let Some(Open::Pick { anchor, .. } | Open::Entry { anchor, .. }) = &mut self.open {
                    *anchor = Anchor::Tab(i);
                }
            }
            Focus::Row(i) => {
                if let Some(hit) = self.side_items().get(i).cloned() {
                    self.activate(Target::of(hit), How::Menu, None);
                }
            }
            Focus::Feed => self.feed_menu(),
            Focus::Compose => {}
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
                Binding::new("Shift+Tab", "or ↑ at the top leaves the box"),
                Binding::new("Enter", "send"),
                Binding::new("Tab", "complete"),
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
                Binding::new("Shift+Tab", "back"),
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
