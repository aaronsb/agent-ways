// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! Opening on the tab the last chat showed (#1349). The interactive chat
//! names attend-chat's state directory ([`super::Chat::reopen_last_tab`]);
//! the chat then reopens the tab recorded there, records each change of
//! the shown tab and of the operator's membership of it, and keeps the
//! operator view's DM watermark across restarts. What the files hold and
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
    /// shown that view, which shows every DM. The view's watermark from
    /// the last chat stands in for it, so the DMs that came while no chat
    /// ran mark their agents' rows; with none kept, the DMs from before
    /// this start count as shown.
    pub(super) fn reopen_last_tab(&mut self, dir: PathBuf) {
        let last = Last::load(&last_tab::path(&dir));
        let known = crate::groups::channels(ColorDepth::NoColor);
        if let Some(tab) = last_tab::reopen(last.as_ref(), &known, &human_member_id()) {
            if tab != Tab::Merged {
                let mut kept = Hidden::load(&last_tab::shown_path(&dir));
                if !kept.has(&Tab::Merged) {
                    kept.hide(&Tab::Merged, (now(), 0, 0));
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

    /// Keep the operator view's DM watermark as it is left: the newest DM
    /// it held, or this moment when it held none.
    pub(super) fn keep_operator_view_mark(&self, newest: Option<Order>) {
        let Some(dir) = &self.state_dir else { return };
        let _ = Hidden::hide_in(&last_tab::shown_path(dir), &Tab::Merged, newest.unwrap_or((now(), 0, 0)));
    }
}

/// The clock's seconds, as a message's send order starts.
fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}
