// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The calling hook's deadline (#1287).
//!
//! Claude Code kills a hook at its `timeout` and drops its output. A hook
//! killed after it recorded a fire leaves the fire with no text shown, so the
//! way waits out its refire window unseen. A hook therefore takes one
//! deadline when it starts, a little inside its bound, and every slow step
//! (postchecks, the judge, each macro) takes only what is left before it.
//! Outside a hook there is no deadline and each step keeps its own limit.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

static DEADLINE: OnceLock<Instant> = OnceLock::new();

/// Starts the deadline `budget` from now. The first call wins.
pub(crate) fn start(budget: Duration) {
    let _ = DEADLINE.set(Instant::now() + budget);
}

/// What is left before the deadline; `None` outside a hook.
pub(crate) fn remaining() -> Option<Duration> {
    DEADLINE.get().map(|d| d.saturating_duration_since(Instant::now()))
}

/// `want`, cut to what is left before the deadline once `reserve` is kept
/// back for the steps after this one.
pub(crate) fn within(want: Duration, reserve: Duration) -> Duration {
    match remaining() {
        Some(left) => want.min(left.saturating_sub(reserve)),
        None => want,
    }
}

/// Whether less than `margin` is left: too little to show another way.
pub(crate) fn nearly_passed(margin: Duration) -> bool {
    remaining().is_some_and(|left| left < margin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outside_a_hook_each_step_keeps_its_own_limit() {
        // Test binaries never start a deadline.
        assert_eq!(remaining(), None);
        assert_eq!(within(Duration::from_secs(3), Duration::from_secs(1)), Duration::from_secs(3));
        assert!(!nearly_passed(Duration::from_secs(1)));
    }
}
