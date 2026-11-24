// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! Where the tick loop puts what the governor lets through: a [`Sink`].
//! The sensor host's writes system signals (`host.rs`).

/// One sensor's disclosed lines.
pub(super) struct Disclosure {
    pub(super) sensor: String,
    /// The magnitude the sensor accumulated.
    pub(super) magnitude: f64,
    pub(super) message_lane: bool,
    /// Each line, with how long it stays deliverable when its sensor said.
    pub(super) lines: Vec<(String, Option<sensor_trait::Lapse>)>,
}

/// What became of one line a sink was handed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Delivery {
    /// It went out.
    Written,
    /// The event lane's cut held it: it was logged and counts as handled.
    Cut,
    /// It could not go out, and its trigger may fire again.
    Failed,
}

/// Where the loop puts what the governor lets through.
pub(super) trait Sink {
    /// Deliver `batch`. Returns what became of each line of each
    /// disclosure. The governor counts a batch with a line written as a
    /// disclosure. A written or cut line spends a session trigger; a failed
    /// one releases it to fire again.
    fn deliver(&mut self, batch: &[Disclosure]) -> Vec<Vec<Delivery>>;
}
