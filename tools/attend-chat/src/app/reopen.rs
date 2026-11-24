// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! Opening on the tab the last chat showed (#1349). The interactive chat
//! names attend-chat's state directory ([`super::Chat::reopen_last_tab`]);
//! the chat then reopens the tab recorded there, records each change of
//! the shown tab and of the operator's membership of it, and keeps the
//! DM watermarks of the operator view and the DM views across restarts. What the files hold and
//! how a record is chosen is [`crate::last_tab`]'s; here is when the chat
//! reads and writes them.

use std::path::PathBuf;

use agent_theme::ColorDepth;

use super::ChatPane;
use crate::hidden::{Hidden, Order};
use crate::last_tab::{self, Last};
use crate::signal::human_member_id;
use crate::tabs::Tab;

impl ChatPane {
    /// Open on the tab recorded in `dir`, and record there from now on. A
    /// channel gone since, or left, opens `#open`; with no record, or one
    /// that does not parse, the chat opens where it opens without one.
    ///
    /// A chat that opens on another tab than the operator view has not
    /// shown that view, which shows every DM. The DM marks the last chat
    /// kept stand in for it, so the DMs that came while no chat ran mark
    /// their agents' rows, and those read in a DM view do not. With no
    /// operator view mark kept, the DMs from before this start count as
    /// shown, and the start is kept as the mark for the next chat.
    pub(super) fn reopen_last_tab(&mut self, dir: PathBuf) {
        let last = Last::load(&last_tab::path(&dir));
        let known = crate::groups::channels(ColorDepth::NoColor);
        if let Some(tab) = last_tab::reopen(last.as_ref(), &known, &human_member_id()) {
            if tab != Tab::Merged {
                let path = last_tab::shown_path(&dir);
                let mut kept = Hidden::load(&path);
                if !kept.has(&Tab::Merged) {
                    let start = (now(), 0, 0);
                    match Hidden::shown_in(&path, &Tab::Merged, start) {
                        Ok(written) => kept = written,
                        Err(_) => kept.hide(&Tab::Merged, start),
                    }
                }
                self.shown = kept;
            }
            self.foreground = tab;
            self.invalidate();
        }
        self.recorded = last;
        self.state_dir = Some(dir);
    }

    /// Record the tab shown, when the chat keeps a record and the record
    /// changes: another tab, or the operator now a member of the channel
    /// shown ([`Last::showing`]). Membership is read from the channels the
    /// world holds, or scanned when none is built. A record that cannot be
    /// written is tried again at the next change.
    pub(super) fn record_tab(&mut self) {
        let Some(dir) = &self.state_dir else { return };
        let person = human_member_id();
        let next = match &self.world {
            Some(w) => Last::showing(&self.foreground, self.recorded.as_ref(), &w.groups, &person),
            None => Last::showing(&self.foreground, self.recorded.as_ref(), &crate::groups::channels(ColorDepth::NoColor), &person),
        };
        if let Some(next) = next.filter(|n| self.recorded.as_ref() != Some(n)) {
            if next.save(&last_tab::path(dir)).is_ok() {
                self.recorded = Some(next);
            }
        }
    }

    /// Keep the mark of the DM view shown, an agent's DM view or its own,
    /// while it is still shown: a chat that quits or is killed there never
    /// leaves it, so no change of tab would keep it. Called on each read of
    /// the world (every Enter, every refresh, a new sender), it writes only
    /// when the view holds a DM past the mark in memory.
    pub(super) fn keep_dm_view_shown(&mut self) {
        if self.state_dir.is_none() || !matches!(self.foreground, Tab::Dm(_) | Tab::Review(_)) {
            return;
        }
        let Some((mark, Some(newest))) = self.shows_up_to(&self.foreground) else { return };
        if !self.shown.covers(&mark, newest) {
            self.shown.hide(&mark, newest);
            self.keep_shown(&mark, Some(newest));
        }
    }

    /// Keep the DM mark `mark` as its view is left ([`Hidden::shown_in`]):
    /// the newest DM the view held. The operator view keeps this moment
    /// when it held none; a DM view that held none keeps nothing.
    pub(super) fn keep_shown(&self, mark: &Tab, newest: Option<Order>) {
        let Some(dir) = &self.state_dir else { return };
        let upto = match (mark, newest) {
            (_, Some(n)) => n,
            (Tab::Merged, None) => (now(), 0, 0),
            _ => return,
        };
        let _ = Hidden::shown_in(&last_tab::shown_path(dir), mark, upto);
    }
}

/// The clock's seconds, as a message's send order starts.
fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}
