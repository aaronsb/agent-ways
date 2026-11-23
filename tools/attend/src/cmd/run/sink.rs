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
    pub(super) message_lane: bool,
    /// Each line, with how long it stays deliverable when its sensor said.
    pub(super) lines: Vec<(String, Option<sensor_trait::Lapse>)>,
}

/// Where the loop puts what the governor lets through.
pub(super) trait Sink {
    /// Deliver `batch`. Returns whether anything went out, which the
    /// governor counts as a disclosure.
    fn deliver(&mut self, batch: &[Disclosure]) -> bool;
}

/// `attend run`'s sink: Monitor lines on stdout.
pub(super) struct Monitor;

impl Sink for Monitor {
    fn deliver(&mut self, batch: &[Disclosure]) -> bool {
        let lines: Vec<(String, String, Vec<String>)> =
            batch.iter().map(|d| (d.sensor.clone(), d.priority.to_string(), d.lines.iter().map(|(l, _)| l.clone()).collect())).collect();
        emit::emit_batch(&lines)
    }
}
