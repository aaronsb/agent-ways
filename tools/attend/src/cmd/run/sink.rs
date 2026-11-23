// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! Where the tick loop puts what the governor lets through: a [`Sink`].
//! `attend run` uses [`Monitor`], which prints Monitor lines on stdout; the
//! sensor host writes signals (`host.rs`).

use crate::emit;

/// One sensor's disclosed lines.
pub(super) struct Disclosure {
    pub(super) sensor: String,
    pub(super) priority: &'static str,
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

/// `attend run`'s sink: Monitor lines on stdout. A low-priority batch is
/// logged, not printed: the cut.
pub(super) struct Monitor;

impl Sink for Monitor {
    fn deliver(&mut self, batch: &[Disclosure]) -> Vec<Vec<Delivery>> {
        let lines: Vec<(String, String, Vec<String>)> =
            batch.iter().map(|d| (d.sensor.clone(), d.priority.to_string(), d.lines.iter().map(|(l, _)| l.clone()).collect())).collect();
        emit::emit_batch(&lines);
        let verdict = |d: &Disclosure| if d.priority == "low" { Delivery::Cut } else { Delivery::Written };
        batch.iter().map(|d| vec![verdict(d); d.lines.len()]).collect()
    }
}
