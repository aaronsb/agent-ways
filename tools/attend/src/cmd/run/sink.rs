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

/// Where the loop puts what the governor lets through.
pub(super) trait Sink {
    /// Deliver `batch`. Returns, for each disclosure, whether each of its
    /// lines went out. The governor counts a batch with any line out as a
    /// disclosure, and a session trigger is spent only by a line that went
    /// out.
    fn deliver(&mut self, batch: &[Disclosure]) -> Vec<Vec<bool>>;
}

/// `attend run`'s sink: Monitor lines on stdout. A low-priority batch is
/// logged, not printed, so its lines do not go out.
pub(super) struct Monitor;

impl Sink for Monitor {
    fn deliver(&mut self, batch: &[Disclosure]) -> Vec<Vec<bool>> {
        let lines: Vec<(String, String, Vec<String>)> =
            batch.iter().map(|d| (d.sensor.clone(), d.priority.to_string(), d.lines.iter().map(|(l, _)| l.clone()).collect())).collect();
        emit::emit_batch(&lines);
        batch.iter().map(|d| vec![d.priority != "low"; d.lines.len()]).collect()
    }
}
