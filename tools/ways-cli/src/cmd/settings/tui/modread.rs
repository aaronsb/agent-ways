//! The mod tab's state, read off the screen's thread (ADR-512 item 6). The
//! read calls Claude Code for every enabled target, so it runs on a worker
//! and the screens show the last result until the next lands.
//!
//! A read starts only when one is owed: at the start, and after a queued
//! command or a file change asks for one. A request that comes while a read
//! runs, or within `fresh` of the last one starting, is kept and starts once
//! both have passed. A read landing owes nothing, so a slow read cannot
//! start the next. A read that panics is reported, not waited on.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use crate::cmd::mods::Overview;

use super::mods::Read;

/// What reads the state: [`crate::cmd::mods::overview`], or a test's.
pub type Reader = fn() -> Vec<Overview>;

pub struct ModRead {
    read: Reader,
    fresh: Duration,
    /// The clock the window is measured on: the real one, or a test's.
    now: Box<dyn Fn() -> Instant>,
    value: Option<Vec<Overview>>,
    failed: Option<String>,
    rx: Option<Receiver<Result<Vec<Overview>, String>>>,
    started: Option<Instant>,
    /// Reads that finished, landed or failed: part of the screens' stamp.
    finished: u64,
    owed: bool,
    waited: bool,
}

impl ModRead {
    /// A read owed from the start, none running.
    pub fn new(read: Reader, fresh: Duration) -> ModRead {
        ModRead::with_clock(read, fresh, Box::new(Instant::now))
    }

    /// [`ModRead::new`] measuring the window on `now`.
    pub fn with_clock(read: Reader, fresh: Duration, now: Box<dyn Fn() -> Instant>) -> ModRead {
        ModRead { read, fresh, now, value: None, failed: None, rx: None, started: None, finished: 0, owed: true, waited: false }
    }

    /// Ask for a read; it starts when no read runs and `fresh` has passed
    /// since the last one started.
    pub fn owe(&mut self) {
        self.owed = true;
        self.pump();
    }

    /// Take a finished read, and start an owed one when it may.
    pub fn pump(&mut self) {
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(r) => self.land(r),
                Err(TryRecvError::Disconnected) => self.land(Err("the read ended without a result".into())),
                Err(TryRecvError::Empty) => {}
            }
        }
        let now = (self.now)();
        let idle = self.rx.is_none() && self.started.is_none_or(|t| now.saturating_duration_since(t) >= self.fresh);
        if self.owed && idle {
            self.owed = false;
            self.start();
        }
    }

    /// Wait at most `limit`, once, for the first read: one that lands
    /// quickly shows in the first build.
    pub fn wait_first(&mut self, limit: Duration) {
        if self.waited || self.value.is_some() {
            return;
        }
        self.waited = true;
        if let Some(rx) = self.rx.take() {
            match rx.recv_timeout(limit) {
                Ok(r) => self.land(r),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => self.rx = Some(rx),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => self.land(Err("the read ended without a result".into())),
            }
        }
    }

    fn start(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel();
        let read = self.read;
        std::thread::spawn(move || {
            // A panic here is reported on the tab; it must not restore the
            // terminal under the running screen or print onto it.
            agent_tui::quiet_panics_here();
            let r = std::panic::catch_unwind(read).map_err(|p| {
                p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "it panicked".into())
            });
            let _ = tx.send(r);
        });
        self.rx = Some(rx);
        self.started = Some((self.now)());
    }

    fn land(&mut self, r: Result<Vec<Overview>, String>) {
        match r {
            Ok(v) => {
                self.value = Some(v);
                self.failed = None;
            }
            Err(e) => self.failed = Some(e),
        }
        self.rx = None;
        self.finished += 1;
    }

    /// Reads that finished so far.
    pub fn finished(&self) -> u64 {
        self.finished
    }

    /// Why the last read failed, when it did.
    pub fn last_failed(&self) -> Option<&str> {
        self.failed.as_deref()
    }

    /// What the tab shows: the last states read, and where the read stands.
    pub fn shown(&self) -> (Vec<Overview>, Read) {
        let read = match (&self.value, &self.failed) {
            (None, Some(e)) => Read::Failed(e.clone()),
            (Some(_), Some(e)) => Read::Stale(e.clone()),
            (Some(_), None) => Read::Done,
            (None, None) => Read::Reading,
        };
        (self.value.clone().unwrap_or_default(), read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    /// A clock the test moves by hand, so the window passes only when the
    /// test says, whatever the runner's speed.
    fn clock() -> (Rc<Cell<Instant>>, Box<dyn Fn() -> Instant>) {
        let t = Rc::new(Cell::new(Instant::now()));
        let read = t.clone();
        (t, Box::new(move || read.get()))
    }

    fn advance(t: &Cell<Instant>, by: Duration) {
        t.set(t.get() + by);
    }

    fn quick() -> Vec<Overview> {
        Vec::new()
    }

    fn slow() -> Vec<Overview> {
        std::thread::sleep(Duration::from_millis(120));
        Vec::new()
    }

    /// A read that outlasts any wait a test makes.
    fn very_slow() -> Vec<Overview> {
        std::thread::sleep(Duration::from_secs(2));
        Vec::new()
    }

    fn broken() -> Vec<Overview> {
        panic!("no plugin list")
    }

    /// Pump until `done` holds, waiting on the read's thread for at most ten
    /// seconds. Only a read landing is waited on; the window is the clock's.
    fn until(r: &mut ModRead, done: impl Fn(&ModRead) -> bool) {
        let start = Instant::now();
        while !done(r) && start.elapsed() < Duration::from_secs(10) {
            r.pump();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(done(r), "the read never landed");
    }

    /// Review finding 1: a read landing starts no other, however long ago
    /// the last one started; only a request does.
    #[test]
    fn a_slow_read_landing_starts_no_other() {
        let (t, now) = clock();
        let mut r = ModRead::with_clock(slow, Duration::from_millis(20), now);
        r.pump();
        until(&mut r, |r| r.finished() == 1);
        advance(&t, Duration::from_secs(60));
        for _ in 0..20 {
            r.pump();
        }
        assert_eq!(r.finished(), 1, "nothing owed, nothing read");
        assert!(r.rx.is_none());
    }

    /// Review finding 3: a request during a read, or inside the window, is
    /// kept and starts once both have passed.
    #[test]
    fn a_request_during_a_read_starts_after_it() {
        let (t, now) = clock();
        let mut r = ModRead::with_clock(slow, Duration::from_millis(300), now);
        r.pump();
        r.owe();
        assert!(r.owed && r.rx.is_some(), "kept while the read runs");
        until(&mut r, |r| r.finished() == 1);
        r.pump();
        assert!(r.owed && r.rx.is_none(), "landed, and the window has not passed");
        advance(&t, Duration::from_millis(300));
        r.pump();
        assert!(!r.owed && r.rx.is_some(), "the kept request started once the window passed");
        until(&mut r, |r| r.finished() == 2);

        let (t, now) = clock();
        let mut q = ModRead::with_clock(quick, Duration::from_millis(50), now);
        q.pump();
        until(&mut q, |q| q.finished() == 1);
        q.owe();
        assert!(q.owed && q.rx.is_none(), "inside the window it waits");
        advance(&t, Duration::from_millis(50));
        q.pump();
        until(&mut q, |q| q.finished() == 2);
    }

    /// Review finding 2: a read that panics is reported and not waited on.
    #[test]
    fn a_read_that_panics_is_reported() {
        let mut r = ModRead::new(broken, Duration::from_millis(10));
        r.pump();
        until(&mut r, |r| r.finished() == 1);
        assert_eq!(r.shown().1, Read::Failed("no plugin list".into()));
        assert!(r.rx.is_none(), "no read left running");
    }

    /// Review finding 9: the first build waits once, not at every rebuild:
    /// the second wait returns with the read still running, where waiting
    /// would have seen it land.
    #[test]
    fn the_first_build_waits_once() {
        let mut r = ModRead::new(very_slow, Duration::from_secs(5));
        r.pump();
        r.wait_first(Duration::from_millis(10));
        assert!(r.rx.is_some() && r.waited);
        r.wait_first(Duration::from_secs(30));
        assert!(r.rx.is_some(), "the second call did not wait for the read");
        assert_eq!(r.shown().1, Read::Reading);
    }

    static FLAKY: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    /// Reads once, then panics.
    fn flaky() -> Vec<Overview> {
        if FLAKY.fetch_add(1, std::sync::atomic::Ordering::SeqCst) > 0 {
            panic!("plugin list unreadable");
        }
        Vec::new()
    }

    /// Re-review finding D: a failed read after a good one is shown beside
    /// the earlier state, not hidden behind it.
    #[test]
    fn a_failed_reread_is_shown_with_the_earlier_state() {
        let (t, now) = clock();
        let mut r = ModRead::with_clock(flaky, Duration::from_millis(1), now);
        r.pump();
        until(&mut r, |r| r.finished() == 1);
        assert_eq!(r.shown().1, Read::Done);
        advance(&t, Duration::from_millis(1));
        r.owe();
        until(&mut r, |r| r.finished() == 2);
        assert_eq!(r.shown().1, Read::Stale("plugin list unreadable".into()));
        assert_eq!(r.last_failed(), Some("plugin list unreadable"));
    }
}
