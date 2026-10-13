//! An agent's DM view (ADR-405, amendment of 2026-10-13): the direct
//! messages between the operator and one agent, both ways, as a view of
//! the messages the chat already holds ([`crate::tabs::shows`]). A click on
//! a claude's row opens it: the view is listed on a row under the agent's
//! and shown, with its tab on the bar's left. The compose box there sends a
//! direct message to the agent. Hide DMs, on the view's menu or the
//! agent's, takes the row off the column and deletes nothing; a click on
//! the agent's row lists it again with every message it held.

use agent_tui::ratatui::layout::Rect;
use agent_tui::{Anchor, Item, Open};

use super::sidebar::SideHit;
use super::ChatPane;
use crate::tabs::Tab;

pub(super) const SHOW_DMS: &str = "Show DMs";
pub(super) const HIDE_DMS: &str = "Hide DMs";

impl ChatPane {
    /// Whether `member` has a DM view: a claude the column lists. A human
    /// has none, since `@name` routes to a session's tray.
    pub(super) fn has_dms(&self, member: &str) -> bool {
        self.world.as_ref().is_some_and(|w| w.seen.iter().any(|s| s.member == member && s.identity.is_claude))
    }

    /// List `member`'s DM view under its row, if it is not listed, and
    /// show it.
    pub(super) fn open_dm(&mut self, member: String) {
        if !self.dms.contains(&member) {
            self.dms.push(member.clone());
            self.relay();
        }
        self.set_tab(Tab::Dm(member));
    }

    /// Take `member`'s DM view off the column. Shown, the operator view
    /// takes its place, where the same DMs show among everything else.
    pub(super) fn hide_dm(&mut self, member: &str) {
        self.dms.retain(|m| m != member);
        if self.foreground == Tab::Dm(member.to_string()) {
            self.set_tab(Tab::Merged);
        }
        self.relay();
    }

    /// Lay the column's rows out again for the DM views listed, keeping
    /// the focus on its row.
    fn relay(&mut self) {
        if let Some(w) = &mut self.world {
            w.laid.rows = super::sidebar::side_items_of(&w.seen, &w.rows, &self.dms);
        }
        self.invalidate();
        if self.world.is_some() {
            self.refocus();
        }
    }

    /// The DM view's menu, docked under `at` when given, else under its row
    /// in the column, else its tab.
    pub(super) fn dm_menu(&mut self, member: String, at: Option<Rect>) {
        self.dirty = true;
        let name = self.world().seen.iter().find(|s| s.member == member).map_or(member.clone(), |s| s.identity.nickname.clone());
        let anchor = match at.or_else(|| self.side_row(&SideHit::Dm(member.clone()))) {
            Some(r) => Anchor::Row(r),
            None => self.slot_of(&Tab::Dm(member.clone())).map_or(Anchor::Center, Anchor::Tab),
        };
        self.to_open = Some(Open::Pick { id: format!("dm:{member}"), title: format!("@{name} DMs"), items: vec![Item::new(HIDE_DMS)], multi: false, anchor });
    }

    /// An item of `member`'s DM view menu.
    pub(super) fn dm_item(&mut self, member: &str, item: &str) {
        if item == HIDE_DMS {
            self.hide_dm(member);
        }
    }
}
