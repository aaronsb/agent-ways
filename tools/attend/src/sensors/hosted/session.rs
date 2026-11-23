// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The memory of a file sensor's triggers kept for the session, and how a
//! delivered line spends them.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
#[cfg(test)]
use sensor_spec::Trigger;

use super::{FileSensor, Memo};

/// The memory of a sensor's triggers with `memory: session`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub(super) struct SessionMemory {
    /// What each trigger last saw, by trigger name.
    #[serde(default)]
    pub(super) held: BTreeMap<String, Memo>,
    /// The fingerprints of the trigger definitions that have fired. Each
    /// fires at most once in the session, under whatever name it carries.
    #[serde(default)]
    pub(super) spent: BTreeSet<String>,
}

impl FileSensor {
    /// Mark the session trigger `trigger` spent, as the delivery of its line
    /// does: it fires no more in this session.
    #[cfg(test)]
    pub(crate) fn spend(&mut self, trigger: &Trigger) {
        self.spend_test(trigger.fingerprint());
    }

    pub(super) fn spend_test(&mut self, test: String) {
        if self.session.spent.insert(test) {
            self.newly_spent = true;
        }
    }

    /// The most undelivered session-trigger lines kept; the oldest go first.
    pub(super) const UNDELIVERED_MAX: usize = 64;

    /// Spend each session trigger whose line `description` is.
    pub(super) fn spend_lines_of(&mut self, description: &str) {
        let (hit, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.undelivered).into_iter().partition(|(d, _)| d == description);
        self.undelivered = rest;
        for (_, test) in hit {
            self.spend_test(test);
        }
    }
}
