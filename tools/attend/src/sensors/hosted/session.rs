// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The memory of a file sensor's triggers kept for the session, and how
//! the loop settles the lines they offer.
//!
//! A session trigger that fires offers its lines and is outstanding until
//! they settle: it does not fire again while the loop holds them, so a line
//! the governor holds is not offered twice. A line that was written, or that
//! the event lane's cut handled, settles; once every line the trigger
//! offered has settled, the trigger is spent and fires no more in the
//! session. A line the loop dropped releases the trigger, which fires again
//! on the next poll that meets it. A re-executed host starts with nothing
//! outstanding, so a line held at the exec is offered again.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
#[cfg(test)]
use sensor_spec::Trigger;

use super::{FileSensor, Memo};

/// The memory of a sensor's triggers with `memory: session`, as the
/// session's checkpoint carries it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub(super) struct SessionMemory {
    /// What each trigger last saw, by trigger name.
    #[serde(default)]
    pub(super) held: BTreeMap<String, Memo>,
    /// The fingerprints of the trigger definitions whose lines were all
    /// delivered. Each is delivered at most once in the session, under
    /// whatever name it carries.
    #[serde(default)]
    pub(super) spent: BTreeSet<String>,
}

/// The session triggers whose lines are out: by fingerprint, the lines not
/// settled yet. It lives in the process only.
pub(super) type Outstanding = BTreeMap<String, BTreeSet<String>>;

impl FileSensor {
    /// Mark the session trigger `trigger` spent, as the delivery of its
    /// lines does: it fires no more in this session.
    #[cfg(test)]
    pub(crate) fn spend(&mut self, trigger: &Trigger) {
        self.spend_test(trigger.fingerprint());
    }

    fn spend_test(&mut self, test: String) {
        if self.session.spent.insert(test) {
            self.newly_spent = true;
        }
    }

    /// The trigger fingerprinted `test` offered `lines`.
    pub(super) fn offer(&mut self, test: String, lines: BTreeSet<String>) {
        if !lines.is_empty() {
            self.outstanding.insert(test, lines);
        }
    }

    /// The line `description` was written, or handled by the cut. A trigger
    /// all of whose lines have settled is spent.
    pub(super) fn settle(&mut self, description: &str) {
        let mut done = Vec::new();
        for (test, lines) in self.outstanding.iter_mut() {
            if lines.remove(description) && lines.is_empty() {
                done.push(test.clone());
            }
        }
        for test in done {
            self.outstanding.remove(&test);
            self.spend_test(test);
        }
    }

    /// The line `description` was dropped: each trigger that offered it is
    /// no longer outstanding, and fires again on the next poll that meets
    /// it.
    pub(super) fn release(&mut self, description: &str) {
        self.outstanding.retain(|_, lines| !lines.contains(description));
    }

    /// Whether an outstanding trigger offered the line `description`, so
    /// that dropping it would release the trigger.
    pub(super) fn awaits(&self, description: &str) -> bool {
        self.outstanding.values().any(|lines| lines.contains(description))
    }
}
