// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The time source a waiter reads and sleeps on. Production runs on
//! [`SystemClock`]; a test passes a clock it advances itself, so a wait that
//! spans a claim window or a deadline needs no real time to pass and no
//! tolerance for a slow runner.

use std::time::{Duration, Instant};

pub trait Clock {
    fn now(&self) -> Instant;
    fn sleep(&self, d: Duration);
}

/// The real clock.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }
}

#[cfg(test)]
pub(crate) use manual::ManualClock;

#[cfg(test)]
mod manual {
    use super::Clock;
    use std::cell::{Cell, RefCell};
    use std::time::{Duration, Instant};

    /// A clock a test advances: `sleep` moves it forward by what was asked
    /// and returns at once. The closure given to `at_first_sleep` runs at
    /// the first sleep, so a test can act as the host between two polls.
    /// With `really_sleeping_after_the_first`, every later sleep also waits
    /// in real time, for a test whose retry budget has to span the time a
    /// real process needs.
    pub(crate) struct ManualClock {
        base: Instant,
        offset: Cell<Duration>,
        on_sleep: RefCell<Option<Box<dyn FnOnce()>>>,
        real_after_first: bool,
    }

    impl ManualClock {
        pub(crate) fn new() -> ManualClock {
            ManualClock { base: Instant::now(), offset: Default::default(), on_sleep: Default::default(), real_after_first: false }
        }
        pub(crate) fn at_first_sleep(self, f: impl FnOnce() + 'static) -> ManualClock {
            *self.on_sleep.borrow_mut() = Some(Box::new(f));
            self
        }
        pub(crate) fn really_sleeping_after_the_first(mut self) -> ManualClock {
            self.real_after_first = true;
            self
        }
        /// Time passed on this clock.
        pub(crate) fn elapsed(&self) -> Duration {
            self.offset.get()
        }
    }

    impl Clock for ManualClock {
        fn now(&self) -> Instant {
            self.base + self.offset.get()
        }
        fn sleep(&self, d: Duration) {
            let first = self.on_sleep.borrow_mut().take();
            match first {
                Some(f) => f(),
                None if self.real_after_first => std::thread::sleep(d),
                None => {}
            }
            self.offset.set(self.offset.get() + d);
        }
    }
}
