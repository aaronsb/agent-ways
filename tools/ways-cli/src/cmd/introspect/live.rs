//! Which sessions are being written to, read from `stat` alone (#780).
//!
//! A session is live while Claude Code writes its transcript. The sessions
//! screen learns that from each transcript's size and mtime, never from its
//! text: one stat pass when the list is built, then each session re-stated
//! on its own interval. The interval is short right after a write and
//! doubles with each quiet sample up to a ceiling; a write resets it. A
//! transcript last written longer ago than the cutoff is not re-stated at
//! all while the screen is open, so a month of old sessions costs one stat
//! each. A replay that follows a live session samples the event log and
//! the session's transcript on the same backoff, so an idle follow costs no
//! more than the list. A replay of a quiet session within the cutoff
//! watches its transcript the same way, and follows once it is written.
//! A replay's stream is one file or two, so it is never cut off.
//!
//! The clock and the stat are passed in, so a test counts the stats a
//! schedule makes without waiting on a real one.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A transcript written within this long is live: the list marks it and a
/// replay of it follows. Claude Code writes a turn's messages and tool
/// results as they come, so a session at work writes far more often.
pub(crate) const LIVE_WINDOW: Duration = Duration::from_secs(120);

/// The re-stat interval right after a write.
pub(crate) const RESTAT_MIN: Duration = Duration::from_secs(2);

/// The ceiling the interval doubles to while a file stays quiet.
pub(crate) const RESTAT_MAX: Duration = Duration::from_secs(60);

/// A file last written longer ago than this is not re-stated while the
/// screen is open: its first stat is enough to show its age.
pub(crate) const RESTAT_CUTOFF: Duration = Duration::from_secs(24 * 3600);

/// How often the screen asks for the stats that are due. Finer than
/// [`RESTAT_MIN`], so a due stat waits at most this long; a tick with
/// nothing due stats nothing.
pub(crate) const SAMPLE_TICK: Duration = Duration::from_secs(1);

fn ms(d: Duration) -> u64 {
    d.as_millis() as u64
}

/// What a stat tells: the file's length and when it was last written, in
/// milliseconds since the Unix epoch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Probe {
    pub(crate) len: u64,
    pub(crate) mtime_ms: u64,
}

/// Now, in milliseconds since the Unix epoch.
pub(crate) type Clock = Rc<dyn Fn() -> u64>;

/// A stat of one file.
pub(crate) type Stat = Rc<dyn Fn(&Path) -> Option<Probe>>;

pub(crate) fn system_clock() -> Clock {
    Rc::new(|| SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64))
}

/// The file's length and mtime, or `None` when it cannot be stated.
pub(crate) fn stat_file(p: &Path) -> Option<Probe> {
    let meta = std::fs::metadata(p).ok()?;
    let mtime_ms = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_millis() as u64;
    Some(Probe { len: meta.len(), mtime_ms })
}

pub(crate) fn system_stat() -> Stat {
    Rc::new(stat_file)
}

/// Whether a file last written at `mtime_ms` is live at `now_ms`.
pub(crate) fn is_live(mtime_ms: u64, now_ms: u64) -> bool {
    now_ms.saturating_sub(mtime_ms) <= ms(LIVE_WINDOW)
}

/// One file's re-stat schedule: what the last stat saw, the interval, and
/// when the next stat is due; `None` when it is never due again.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Backoff {
    last: Option<Probe>,
    interval_ms: u64,
    next_ms: Option<u64>,
    /// A replay's schedule: never cut off, and a file missing at one stat
    /// is a quiet stat, not the end of the schedule.
    endless: bool,
}

impl Backoff {
    /// The schedule after the first stat, `probe`, at `now`. The first
    /// interval is the time since the last write, between the floor and
    /// the ceiling: a file quiet for an hour starts at the ceiling.
    pub(crate) fn start(probe: Option<Probe>, now: u64) -> Backoff {
        let age = probe.map_or(0, |p| now.saturating_sub(p.mtime_ms));
        let mut b = Backoff { last: probe, interval_ms: age.clamp(ms(RESTAT_MIN), ms(RESTAT_MAX)), next_ms: None, endless: false };
        b.schedule(now);
        b
    }

    fn schedule(&mut self, now: u64) {
        self.next_ms = match self.last {
            _ if self.endless => Some(now + self.interval_ms),
            Some(p) if now.saturating_sub(p.mtime_ms) <= ms(RESTAT_CUTOFF) => Some(now + self.interval_ms),
            _ => None,
        };
    }

    /// The same schedule, never cut off: a replay's.
    fn endless(mut self, now: u64) -> Backoff {
        self.endless = true;
        self.schedule(now);
        self
    }

    /// Whether a stat is due at `now`.
    pub(crate) fn due(&self, now: u64) -> bool {
        self.next_ms.is_some_and(|n| now >= n)
    }

    /// Whether the file is re-stated at all.
    pub(crate) fn watching(&self) -> bool {
        self.next_ms.is_some()
    }

    /// Take a stat made at `now`. True when the file was written since the
    /// last one: the interval goes back to the floor. A quiet stat doubles
    /// it to the ceiling. A file gone, or quiet past the cutoff, is not
    /// stated again.
    pub(crate) fn observe(&mut self, probe: Option<Probe>, now: u64) -> bool {
        let wrote = probe.is_some() && probe != self.last;
        self.interval_ms = if wrote { ms(RESTAT_MIN) } else { (self.interval_ms * 2).min(ms(RESTAT_MAX)) };
        if probe.is_some() || !self.endless {
            self.last = probe;
        }
        self.schedule(now);
        wrote
    }

    /// When the file was last written, as the last stat saw it.
    pub(crate) fn last_write(&self) -> Option<u64> {
        self.last.map(|p| p.mtime_ms)
    }

    #[cfg(test)]
    pub(crate) fn interval(&self) -> Duration {
        Duration::from_millis(self.interval_ms)
    }
}

/// The transcripts of the listed sessions under watch: a schedule each,
/// the stat and clock they run on, and how many stats were made.
pub(crate) struct Sampler {
    paths: Vec<Option<PathBuf>>,
    watch: Vec<Backoff>,
    stat: Stat,
    clock: Clock,
    stats: usize,
}

impl Sampler {
    /// State each path once, at list build. A session with no transcript
    /// has no path, and nothing to watch.
    pub(crate) fn new(paths: Vec<Option<PathBuf>>, stat: Stat, clock: Clock) -> Sampler {
        let now = clock();
        let mut stats = 0;
        let watch = paths
            .iter()
            .map(|p| {
                let probe = p.as_deref().and_then(|p| {
                    stats += 1;
                    stat(p)
                });
                Backoff::start(probe, now)
            })
            .collect();
        Sampler { paths, watch, stat, clock, stats }
    }

    /// A sampler that watches nothing, for a list built by hand.
    pub(crate) fn idle(n: usize) -> Sampler {
        Sampler::new(vec![None; n], Rc::new(|_| None), Rc::new(|| 0))
    }

    /// Re-state each file whose stat is due. True when one was written.
    pub(crate) fn sample(&mut self) -> bool {
        let now = (self.clock)();
        let mut wrote = false;
        for (p, b) in self.paths.iter().zip(&mut self.watch) {
            let Some(p) = p else { continue };
            if b.due(now) {
                self.stats += 1;
                wrote |= b.observe((self.stat)(p), now);
            }
        }
        wrote
    }

    /// Whether any file is still re-stated: the screen ticks only then.
    pub(crate) fn watching(&self) -> bool {
        self.watch.iter().any(Backoff::watching)
    }

    pub(crate) fn now(&self) -> u64 {
        (self.clock)()
    }

    /// When session `i`'s transcript was last written, in ms.
    pub(crate) fn last_write(&self, i: usize) -> Option<u64> {
        self.watch.get(i).and_then(Backoff::last_write)
    }

    pub(crate) fn live(&self, i: usize) -> bool {
        self.last_write(i).is_some_and(|t| is_live(t, self.now()))
    }

    pub(crate) fn live_count(&self) -> usize {
        (0..self.watch.len()).filter(|i| self.live(*i)).count()
    }

    /// How many stats were made, the first pass included.
    #[cfg(test)]
    pub(crate) fn stats(&self) -> usize {
        self.stats
    }
}

/// A replay's sources under stat on the backoff, and the probe and clock it
/// runs on: a live session's event log and transcript, which it follows,
/// or a quiet session's transcript, which it watches for a write.
pub(crate) struct Follow {
    backoff: Backoff,
    probe: Rc<dyn Fn() -> Option<Probe>>,
    clock: Clock,
    stats: usize,
}

impl Follow {
    /// Following starts at the floor, as after a write: it was opened on a
    /// session being written to, however long the source sat quiet.
    pub(crate) fn new(probe: Rc<dyn Fn() -> Option<Probe>>, clock: Clock) -> Follow {
        let now = clock();
        let mut backoff = Backoff::start(probe(), now);
        backoff.interval_ms = ms(RESTAT_MIN);
        Follow { backoff: backoff.endless(now), probe, clock, stats: 1 }
    }

    /// Watching a quiet source: its first interval is its age between the
    /// floor and the ceiling, so an idle watch costs a stat a minute at
    /// most, as its row on the list does.
    pub(crate) fn watch(probe: Rc<dyn Fn() -> Option<Probe>>, clock: Clock) -> Follow {
        let now = clock();
        let backoff = Backoff::start(probe(), now).endless(now);
        Follow { backoff, probe, clock, stats: 1 }
    }

    /// A watch on a quiet session's transcript, which a write wakes; none
    /// when there is no transcript or it was last written before the cutoff,
    /// as the list does not re-state it either.
    pub(crate) fn waking(transcript: Option<PathBuf>, stat: Stat, clock: Clock) -> Option<Follow> {
        let path = transcript?;
        let first = stat(&path)?;
        if clock().saturating_sub(first.mtime_ms) > ms(RESTAT_CUTOFF) {
            return None;
        }
        Some(Follow::watch(Rc::new(move || stat(&path)), clock))
    }

    /// A session's sources as one probe: the event log's, which hold its
    /// frames, and its transcript, which holds the token positions and is
    /// written on every turn, so a session at work keeps the interval at
    /// the floor until its hooks log. Their summed length and newest mtime.
    pub(crate) fn session(transcript: Option<PathBuf>) -> Follow {
        Follow::new(Rc::new(move || sources_probe(transcript.as_deref())), system_clock())
    }

    /// Re-state the source when due. True when it was written: read it
    /// again.
    pub(crate) fn poll(&mut self) -> bool {
        let now = (self.clock)();
        if !self.backoff.due(now) {
            return false;
        }
        self.stats += 1;
        self.backoff.observe((self.probe)(), now)
    }

    pub(crate) fn watching(&self) -> bool {
        self.backoff.watching()
    }

    #[cfg(test)]
    pub(crate) fn stats(&self) -> usize {
        self.stats
    }

    #[cfg(test)]
    pub(crate) fn backoff(&self) -> &Backoff {
        &self.backoff
    }
}

/// The event log's sources and the transcript stated together: a change in
/// any of them is something to read again.
fn sources_probe(transcript: Option<&Path>) -> Option<Probe> {
    let mut out: Option<Probe> = None;
    let logs = ways_core::paths::events_log_sources();
    for p in logs.iter().map(PathBuf::as_path).chain(transcript) {
        if let Some(s) = stat_file(p) {
            let o = out.get_or_insert(Probe { len: 0, mtime_ms: 0 });
            o.len = o.len.saturating_add(s.len);
            o.mtime_ms = o.mtime_ms.max(s.mtime_ms);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;

    const S: u64 = 1000;
    const DAY: u64 = 24 * 3600 * S;

    /// A clock the test moves and a stat over files the test writes, with
    /// the stats it served counted per path.
    struct World {
        now: Rc<Cell<u64>>,
        files: Rc<RefCell<HashMap<PathBuf, Probe>>>,
        served: Rc<RefCell<HashMap<PathBuf, usize>>>,
    }

    impl World {
        fn new(now: u64) -> World {
            World { now: Rc::new(Cell::new(now)), files: Rc::default(), served: Rc::default() }
        }
        fn clock(&self) -> Clock {
            let n = self.now.clone();
            Rc::new(move || n.get())
        }
        fn stat(&self) -> Stat {
            let (files, served) = (self.files.clone(), self.served.clone());
            Rc::new(move |p: &Path| {
                *served.borrow_mut().entry(p.to_path_buf()).or_default() += 1;
                files.borrow().get(p).copied()
            })
        }
        fn write(&self, name: &str, mtime: u64) {
            let mut f = self.files.borrow_mut();
            let len = f.get(Path::new(name)).map_or(0, |p| p.len) + 10;
            f.insert(PathBuf::from(name), Probe { len, mtime_ms: mtime });
        }
        fn served(&self, name: &str) -> usize {
            self.served.borrow().get(Path::new(name)).copied().unwrap_or(0)
        }
        /// Run the screen's tick for `secs` seconds of the clock.
        fn run(&self, s: &mut Sampler, secs: u64) {
            for _ in 0..secs / SAMPLE_TICK.as_secs() {
                self.now.set(self.now.get() + ms(SAMPLE_TICK));
                s.sample();
            }
        }
    }

    /// The interval starts at the time since the last write, doubles while
    /// the file stays quiet, stops at the ceiling, and a write puts it back
    /// at the floor.
    #[test]
    fn the_interval_doubles_while_quiet_and_a_write_resets_it() {
        let t0 = 1_000 * DAY;
        let p = |len, at| Some(Probe { len, mtime_ms: at });
        let mut b = Backoff::start(p(10, t0), t0);
        assert_eq!(b.interval(), RESTAT_MIN, "just written: the floor");
        let mut now = t0;
        let mut seen = Vec::new();
        for _ in 0..7 {
            now += b.interval_ms;
            assert!(b.due(now) && !b.due(now - 1));
            assert!(!b.observe(p(10, t0), now), "quiet");
            seen.push(b.interval().as_secs());
        }
        assert_eq!(seen, [4, 8, 16, 32, 60, 60, 60]);
        now += b.interval_ms;
        assert!(b.observe(p(20, now - 5), now), "a write");
        assert_eq!(b.interval(), RESTAT_MIN);
        // Quiet for an hour before the first stat: it starts at the ceiling.
        assert_eq!(Backoff::start(p(1, t0), t0 + 3600 * S).interval(), RESTAT_MAX);
    }

    /// A transcript last written before the cutoff is stated once, when the
    /// list is built, and never again; one inside it is re-stated, and one
    /// that crosses the cutoff while the list is open stops.
    #[test]
    fn the_backoff_stops_restating_old_sessions() {
        let t0 = 1_000 * DAY;
        let w = World::new(t0);
        w.write("/month", t0 - 30 * DAY);
        w.write("/yesterday", t0 - DAY - 60 * S);
        w.write("/hour", t0 - 3600 * S);
        w.write("/now", t0 - 3 * S);
        let names = ["/month", "/yesterday", "/hour", "/now", "/gone"];
        let paths = names.iter().map(|n| Some(PathBuf::from(n))).chain([None]).collect();
        let mut s = Sampler::new(paths, w.stat(), w.clock());
        assert_eq!(s.stats(), 5, "one stat each at build; a session without a transcript has none");
        assert!(!s.live(2) && s.live(3), "an hour old is not live; three seconds is");
        w.run(&mut s, 600);
        assert_eq!(w.served("/month"), 1, "a month old: never re-stated");
        assert_eq!(w.served("/yesterday"), 1, "past the cutoff: never re-stated");
        assert_eq!(w.served("/gone"), 1, "no file: never re-stated");
        assert!((10..=11).contains(&w.served("/hour")), "an hour quiet: once a minute: {}", w.served("/hour"));
        assert!(w.served("/now") <= 16, "backs off from the floor: {}", w.served("/now"));
        // Ten minutes on, the newest is no longer live, though still watched.
        assert!(!s.live(3) && s.watching());

        // A file that crosses the cutoff while the list is open stops.
        let w = World::new(t0);
        w.write("/edge", t0 - DAY + 30 * S);
        let mut s = Sampler::new(vec![Some(PathBuf::from("/edge"))], w.stat(), w.clock());
        w.run(&mut s, 600);
        assert!(!s.watching());
        assert!(w.served("/edge") <= 2, "{}", w.served("/edge"));
    }

    /// A write is seen at the next due stat, marks the session live, and
    /// puts its interval back at the floor.
    #[test]
    fn a_write_marks_the_session_live_and_resets_its_interval() {
        let t0 = 1_000 * DAY;
        let w = World::new(t0);
        w.write("/a", t0 - 3600 * S);
        let mut s = Sampler::new(vec![Some(PathBuf::from("/a"))], w.stat(), w.clock());
        assert!(!s.live(0) && s.live_count() == 0);
        w.write("/a", t0 + S);
        w.run(&mut s, 60);
        assert!(s.live(0) && s.live_count() == 1, "seen within the ceiling");
        assert_eq!(s.watch[0].interval(), RESTAT_MIN);
    }

    /// A hundred sessions over a month, two of them busy: ten minutes of
    /// the list open costs a bounded number of stats, and the old ones
    /// cost only their first.
    #[test]
    fn sampling_a_hundred_sessions_is_bounded() {
        const BUSY: usize = 2;
        let t0 = 1_000 * DAY;
        let w = World::new(t0);
        let names: Vec<String> = (0..100).map(|i| format!("/s{i}")).collect();
        for (i, n) in names.iter().enumerate() {
            // Spread over thirty days, newest first; the first two busy.
            w.write(n, t0 - (i as u64) * 30 * DAY / 100 - if i < BUSY { 0 } else { 60 * S });
        }
        let mut s = Sampler::new(names.iter().map(|n| Some(PathBuf::from(n))).collect(), w.stat(), w.clock());
        let recent = names.iter().enumerate().filter(|(i, _)| (*i as u64) * 30 * DAY / 100 <= DAY).count();
        // The busy two write every five seconds for the ten minutes.
        for _ in 0..120 {
            for n in names.iter().take(BUSY) {
                w.write(n, w.now.get());
            }
            w.run(&mut s, 5);
        }
        let total = s.stats();
        let old: usize = names.iter().skip(recent).map(|n| w.served(n)).sum();
        assert_eq!(old, 100 - recent, "past the cutoff: one stat each");
        // Busy: one per floor interval at most; quiet within a day: once a
        // minute at most, plus the backoff's first steps.
        let bound = 100 + BUSY * (600 / RESTAT_MIN.as_secs() as usize) + (recent - BUSY) * (600 / RESTAT_MAX.as_secs() as usize + 6);
        assert!(total <= bound, "{total} stats, bound {bound}");
        eprintln!("100 sessions, {recent} within the cutoff, {BUSY} busy: {total} stats in 10 minutes (bound {bound})");
    }

    /// The follow re-states the event log on the same backoff, and reports
    /// a write once.
    /// A follow on sources quiet for two days is not cut off: it states at
    /// the ceiling and sees the session write again. A source missing at
    /// one stat is a quiet stat, and the follow goes on.
    #[test]
    fn a_follow_on_sources_quiet_two_days_still_sees_a_write() {
        let t0 = 1_000 * DAY;
        let now = Rc::new(Cell::new(t0));
        let probe: Rc<Cell<Option<Probe>>> = Rc::new(Cell::new(Some(Probe { len: 1, mtime_ms: t0 - 2 * DAY })));
        let (n, p) = (now.clone(), probe.clone());
        let mut f = Follow::new(Rc::new(move || p.get()), Rc::new(move || n.get()));
        assert!(f.watching());
        let mut seen = Vec::new();
        for sec in 1..=300u64 {
            now.set(t0 + sec * S);
            match sec {
                100 => probe.set(None),
                110 => probe.set(Some(Probe { len: 1, mtime_ms: t0 - 2 * DAY })),
                200 => probe.set(Some(Probe { len: 2, mtime_ms: t0 + sec * S })),
                _ => {}
            }
            if f.poll() {
                seen.push(sec);
            }
            assert!(f.watching(), "cut off at {sec}");
        }
        assert_eq!(seen.len(), 1, "the write once, not the file coming back: {seen:?}");
        assert!((200..=260).contains(&seen[0]), "{seen:?}");

        // A watch starts at its source's age and is not cut off either; a
        // transcript past the cutoff, or none, gets no watch.
        let stat_at = |mtime: u64| -> Stat { Rc::new(move |_| Some(Probe { len: 1, mtime_ms: mtime })) };
        let clock: Clock = Rc::new(move || t0);
        let w = Follow::waking(Some(PathBuf::from("/t")), stat_at(t0 - 3600 * S), clock.clone()).expect("an hour quiet: watched");
        assert_eq!(w.backoff().interval(), RESTAT_MAX);
        assert!(Follow::waking(Some(PathBuf::from("/t")), stat_at(t0 - 2 * DAY), clock.clone()).is_none());
        assert!(Follow::waking(None, stat_at(t0), clock.clone()).is_none());
        assert!(Follow::waking(Some(PathBuf::from("/t")), Rc::new(|_| None), clock).is_none());
    }

    #[test]
    fn the_follow_polls_on_the_backoff() {
        let t0 = 1_000 * DAY;
        let now = Rc::new(Cell::new(t0));
        let probe = Rc::new(Cell::new(Probe { len: 1, mtime_ms: t0 }));
        let p = probe.clone();
        let n = now.clone();
        // The source sat quiet an hour; following starts at the floor anyway.
        probe.set(Probe { len: 1, mtime_ms: t0 - 3600 * S });
        let mut f = Follow::new(Rc::new(move || Some(p.get())), Rc::new(move || n.get()));
        assert_eq!(f.backoff().interval(), RESTAT_MIN);
        let mut changes = Vec::new();
        for sec in 1..=40u64 {
            now.set(t0 + sec * S);
            if sec == 20 {
                probe.set(Probe { len: 2, mtime_ms: t0 + sec * S });
            }
            if f.poll() {
                changes.push(sec);
            }
        }
        // Due at 2, 6, 14, 30 (2+4+8+16) from t0; the write at 20 is seen
        // at 30, and the interval starts again from the floor: 32, 36.
        assert_eq!(changes, [30]);
        assert_eq!(f.stats(), 1 + 6);
        assert_eq!(f.backoff().interval(), Duration::from_secs(8));
    }
}
