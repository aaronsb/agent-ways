//! An agent's two DM views (ADR-405, amendment of 2026-10-13), each a view
//! of the messages the chat already holds ([`crate::tabs::shows`]), listed
//! on a row under the agent's and shown with its tab on the bar's left.
//!
//! - The operator's DMs with the agent, both ways ([`Tab::Dm`]). A click on
//!   a claude's row opens it. The compose box there sends a direct message
//!   to the agent.
//! - The agent's own DMs, every one it sent or received, with anyone
//!   ([`Tab::Review`]). Review DMs, on the agent's menu, opens it. It is
//!   drawn in a colour of its own and offers nothing to send: attend-chat
//!   has no send into another agent's conversations.
//!
//! Hiding a view, on its menu or the agent's, takes its row off the column
//! and deletes nothing; opening it again lists every message it held.

use agent_tui::ratatui::layout::Rect;
use agent_tui::{Anchor, Item, Open};

use super::cleanup::{DELETE_READ, HIDE_HERE, SHOW_HIDDEN, TOMBSTONE, UNDO_DELETE};
use super::sidebar::SideHit;
use super::ChatPane;
use crate::attend_cli::Scope;
use crate::tabs::Tab;

pub(super) const SHOW_DMS: &str = "Show DMs";
pub(super) const HIDE_DMS: &str = "Hide DMs";
pub(super) const REVIEW_DMS: &str = "Review DMs";
pub(super) const HIDE_REVIEW: &str = "Hide review";

/// Which of an agent's two DM views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    /// The operator's DMs with the agent.
    Dm,
    /// The agent's own DMs, with anyone.
    Review,
}

impl Kind {
    /// The view of this kind for `member`.
    pub(super) fn tab(self, member: &str) -> Tab {
        match self {
            Kind::Dm => Tab::Dm(member.to_string()),
            Kind::Review => Tab::Review(member.to_string()),
        }
    }

    /// The column row of this kind for `member`.
    fn hit(self, member: &str) -> SideHit {
        match self {
            Kind::Dm => SideHit::Dm(member.to_string()),
            Kind::Review => SideHit::Review(member.to_string()),
        }
    }

    /// The prefix of the view's menu id, before the agent's id.
    fn menu_id(self) -> &'static str {
        match self {
            Kind::Dm => "dm:",
            Kind::Review => "review:",
        }
    }

    /// The item that hides the view.
    fn hide_item(self) -> &'static str {
        match self {
            Kind::Dm => HIDE_DMS,
            Kind::Review => HIDE_REVIEW,
        }
    }
}

impl ChatPane {
    /// Whether `member` has DM views: a claude the column lists. A human
    /// has none, since `@name` routes to a session's tray.
    pub(super) fn has_dms(&self, member: &str) -> bool {
        self.world.as_ref().is_some_and(|w| w.seen.iter().any(|s| s.member == member && s.identity.is_claude))
    }

    /// The agents whose views of `kind` the column lists.
    fn listed(&mut self, kind: Kind) -> &mut Vec<String> {
        match kind {
            Kind::Dm => &mut self.views.dms,
            Kind::Review => &mut self.views.reviews,
        }
    }

    /// Whether the column lists `member`'s view of `kind`.
    pub(super) fn lists(&self, kind: Kind, member: &str) -> bool {
        match kind {
            Kind::Dm => self.views.dms.iter().any(|m| m == member),
            Kind::Review => self.views.reviews.iter().any(|m| m == member),
        }
    }

    /// List `member`'s view of `kind` under its row, if it is not listed,
    /// and show it.
    pub(super) fn open_view(&mut self, kind: Kind, member: String) {
        if !self.lists(kind, &member) {
            self.listed(kind).push(member.clone());
            self.remember_dm_names();
            self.relay();
        }
        self.set_tab(kind.tab(&member));
    }

    /// Take `member`'s view of `kind` off the column. Shown, the operator
    /// view takes its place.
    pub(super) fn hide_view(&mut self, kind: Kind, member: &str) {
        self.listed(kind).retain(|m| m != member);
        if self.foreground == kind.tab(member) {
            self.set_tab(Tab::Merged);
        }
        self.relay();
    }

    /// The operator's DMs with `member`, listed and shown.
    pub(super) fn open_dm(&mut self, member: String) {
        self.open_view(Kind::Dm, member);
    }

    /// Keep the name the column shows for each agent whose DM views are
    /// listed, for when the agent leaves the column.
    pub(super) fn remember_dm_names(&mut self) {
        let Some(w) = &self.world else { return };
        for m in self.views.dms.iter().chain(&self.views.reviews) {
            if let Some(s) = w.seen.iter().find(|s| s.member == *m) {
                self.dm_names.insert(m.clone(), s.identity.nickname.clone());
            }
        }
    }

    /// The name of the agent `member`: as the column shows it, else as it
    /// showed it last, else the first part of its id.
    pub(super) fn dm_name(&self, member: &str) -> String {
        let seen = self.world.as_ref().and_then(|w| w.seen.iter().find(|s| s.member == member)).map(|s| s.identity.nickname.clone());
        seen.or_else(|| self.dm_names.get(member).cloned()).unwrap_or_else(|| member.split('-').next().unwrap_or(member).to_string())
    }

    /// Lay the column's rows out again for the DM views listed, keeping
    /// the focus on its row.
    fn relay(&mut self) {
        if let Some(w) = &mut self.world {
            w.laid.rows = super::sidebar::side_items_of(&w.seen, &w.rows, &self.views);
        }
        self.invalidate();
        if self.world.is_some() {
            self.refocus();
        }
    }

    /// The menu of `member`'s view of `kind`, docked under `at` when given,
    /// else under its row in the column, else its tab.
    pub(super) fn view_menu(&mut self, kind: Kind, member: String, at: Option<Rect>) {
        self.dirty = true;
        self.world();
        let name = self.dm_name(&member);
        let anchor = match at.or_else(|| self.side_row(&kind.hit(&member))) {
            Some(r) => Anchor::Row(r),
            None => self.slot_of(&kind.tab(&member)).map_or(Anchor::Center, Anchor::Tab),
        };
        let title = match kind {
            Kind::Dm => format!("@{name} DMs"),
            Kind::Review => format!("{name} DMs"),
        };
        let items = self.view_items(kind, &member);
        self.to_open = Some(Open::Pick { id: format!("{}{member}", kind.menu_id()), title, items, multi: false, anchor });
    }

    /// The items of `member`'s view of `kind`: Hide DMs, then Hide messages
    /// here and the delete items on the operator's DMs with it; Hide review,
    /// then Tombstone… for an agent that is not live, on its own.
    fn view_items(&self, kind: Kind, member: &str) -> Vec<Item> {
        let mut out = vec![Item::new(kind.hide_item())];
        match kind {
            Kind::Dm => {
                out.extend(self.hide_items(&kind.tab(member), true));
                out.extend(self.delete_items(&Scope::Dm(member.to_string())));
            }
            Kind::Review if self.is_dead_claude(member) => out.push(Item::new(TOMBSTONE)),
            Kind::Review => {}
        }
        out
    }

    /// An item of a DM view's menu, the menu id `id` naming the view; false
    /// when `id` names none.
    pub(super) fn view_item(&mut self, id: &str, item: &str) -> bool {
        for kind in [Kind::Dm, Kind::Review] {
            if let Some(member) = id.strip_prefix(kind.menu_id()) {
                match item {
                    _ if item == kind.hide_item() => self.hide_view(kind, member),
                    HIDE_HERE => self.ask_hide(kind.tab(member)),
                    SHOW_HIDDEN => self.show_hidden(&kind.tab(member)),
                    DELETE_READ => self.delete_menu(&Scope::Dm(member.to_string())),
                    UNDO_DELETE => self.undo_delete(),
                    TOMBSTONE => self.ask_tombstone(member),
                    _ => {}
                }
                return true;
            }
        }
        false
    }
}
