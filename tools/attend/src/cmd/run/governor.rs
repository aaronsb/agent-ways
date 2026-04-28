//! Disclosure governor + sensor scheduling primitives.
//!
//! Both types are private to the run loop — nothing else touches them.
//! Split out of `mod.rs` so the per-tick logic and the rate-limit
//! state machine live in independently reviewable files.

use std::time::{Duration, Instant};

pub(super) struct DisclosureGovernor {
    base_cooldown: Duration,
    last_disclosure: Option<Instant>,
    pub(super) max_disclosures_per_window: u32,
    pub(super) window_disclosures: u32,
    window_start: Instant,
    rate_window: Duration,
    total_events: u32,
    total_events_start: Instant,
}

impl DisclosureGovernor {
    pub(super) fn new(
        base_cooldown: Duration,
        max_per_window: u32,
        rate_window: Duration,
    ) -> Self {
        let now = Instant::now();
        Self {
            base_cooldown,
            last_disclosure: None,
            max_disclosures_per_window: max_per_window,
            window_disclosures: 0,
            window_start: now,
            rate_window,
            total_events: 0,
            total_events_start: now,
        }
    }

    pub(super) fn record_event(&mut self) {
        self.total_events += 1;
    }

    fn aggregate_rate(&self) -> f64 {
        let elapsed = self.total_events_start.elapsed().as_secs_f64();
        if elapsed < 0.001 {
            return 0.0;
        }
        self.total_events as f64 / elapsed
    }

    pub(super) fn cooldown(&self) -> Duration {
        let rate = self.aggregate_rate();
        let multiplier = 1.0 + rate.sqrt() * 3.0;
        self.base_cooldown.mul_f64(multiplier)
    }

    pub(super) fn can_disclose(&mut self) -> bool {
        if self.window_start.elapsed() >= self.rate_window {
            self.window_disclosures = 0;
            self.window_start = Instant::now();
        }

        if self.window_disclosures >= self.max_disclosures_per_window {
            return false;
        }

        if let Some(last) = self.last_disclosure {
            if last.elapsed() < self.cooldown() {
                return false;
            }
        }

        true
    }

    pub(super) fn record_disclosure(&mut self) {
        self.last_disclosure = Some(Instant::now());
        self.window_disclosures += 1;
    }
}

/// Priority-queue entry for the next-due-sensor heap. Ordered by
/// `fire_at` ascending (so `BinaryHeap::peek` returns the soonest).
pub(super) struct ScheduledSensor {
    pub(super) fire_at: Instant,
    pub(super) index: usize,
}

impl Eq for ScheduledSensor {}
impl PartialEq for ScheduledSensor {
    fn eq(&self, other: &Self) -> bool {
        self.fire_at == other.fire_at
    }
}
impl Ord for ScheduledSensor {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.fire_at.cmp(&self.fire_at)
    }
}
impl PartialOrd for ScheduledSensor {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn governor_initial_can_disclose() {
        let mut g =
            DisclosureGovernor::new(Duration::from_secs(1), 5, Duration::from_secs(60));
        assert!(g.can_disclose(), "fresh governor should allow first disclose");
    }

    #[test]
    fn governor_blocks_after_window_max() {
        let mut g =
            DisclosureGovernor::new(Duration::from_millis(1), 2, Duration::from_secs(60));
        // Burn through the window quota — base_cooldown is 1ms so the
        // per-disclosure cooldown does not block the second hit.
        assert!(g.can_disclose());
        g.record_disclosure();
        std::thread::sleep(Duration::from_millis(5));
        assert!(g.can_disclose());
        g.record_disclosure();
        // Third should be blocked by window cap regardless of cooldown.
        assert!(
            !g.can_disclose(),
            "governor should block once window quota is reached"
        );
    }
}
