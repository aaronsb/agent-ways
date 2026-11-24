// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! `attend sensor host`: one session's sensor loop.
//!
//! - `host` — the command: setup, the outer loop, the re-exec on a changed
//!   binary or sensor file, and the sink that writes system signals.
//! - `governor` — `DisclosureGovernor` rate-limit state machine plus
//!   the priority-queue entry type. Pure CPU; no IO.
//! - `tick` — per-tick body, engagement-curve builder, and the
//!   `TickState` bundle that ties them to a single iteration. Reachable
//!   from tests without standing up the host.
//! - `sink` — the [`sink::Sink`] the tick hands what the governor lets
//!   through.
//! - `stamps` — the sweep of per-session stamp files.

mod governor;
pub(crate) mod host;
mod sink;
mod stamps;
mod tick;
