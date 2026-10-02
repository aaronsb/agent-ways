//! A screen that is not the settings shell: an application draws it and
//! handles its keys, and this runs it on the terminal (ADR-504 §3). The
//! test kit renders it headless and drives its keys
//! ([`crate::testkit::render_screen`], [`crate::testkit::drive`]).
//!
//! [`run_screen`] takes the terminal through [`crate::with_terminal`], the
//! one owner of the guard, the signals and the panic hook, as
//! [`crate::run`] does. The signal handlers stay installed for the life of
//! the process, so a process runs one session: put every screen it shows
//! behind one [`Screen`], and end the process when `run_screen` returns,
//! with 128 plus the signal when one ended it.

use std::io;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use ratatui::{DefaultTerminal, Frame};

use crate::app::term::{kill_job_group, restore, Signals, HANGUP, POLL};
use crate::theme::{self, Palette};

/// What an application supplies for one screen session.
pub trait Screen {
    /// The palette the frame draws in. It is set before [`Screen::draw`]
    /// and every cell left on the terminal's ground gets the theme's after.
    fn palette(&self) -> Palette;

    fn draw(&mut self, f: &mut Frame);

    /// Handle a key, pressed or repeated. False ends the session.
    fn key(&mut self, k: KeyEvent) -> bool;

    /// How often [`Screen::tick`] runs; `None` never.
    fn tick_every(&self) -> Option<Duration> {
        None
    }

    /// Time has passed: play a frame on, read a source again.
    fn tick(&mut self) {}
}

/// One frame of `s`: its palette set, the screen drawn, the theme's ground
/// filled in. The terminal loop and the test kit both draw through here.
pub(crate) fn frame<S: Screen + ?Sized>(s: &mut S, f: &mut Frame) {
    theme::set(s.palette());
    s.draw(f);
    theme::fill(f.buffer_mut());
}

/// A panic hook, as `std::panic::set_hook` takes one.
#[doc(hidden)]
pub type Hook = Box<dyn Fn(&std::panic::PanicHookInfo) + Send + Sync>;

/// The panic hook every session runs under ([`crate::run`] and
/// [`run_screen`] alike): end the command in flight and restore the
/// terminal, then run `prev`, the hook that was in place.
#[doc(hidden)]
pub fn panic_hook(prev: Hook) -> Hook {
    Box::new(move |info| {
        // A panic that aborts runs no drop: end the command in flight here.
        kill_job_group();
        restore();
        prev(info);
    })
}

/// When a screen's tick is due.
struct Ticker {
    last: Instant,
}

impl Ticker {
    fn new(now: Instant) -> Ticker {
        Ticker { last: now }
    }

    /// How long to wait for input before the tick is due, at most `POLL`.
    fn wait(&self, every: Option<Duration>, now: Instant) -> Duration {
        match every {
            Some(every) => every.saturating_sub(now.duration_since(self.last)).min(POLL),
            None => POLL,
        }
    }

    /// Whether a tick is due at `now`, taking it when it is. While no tick
    /// is asked for, the clock follows `now`, so the first tick after
    /// ticking starts comes a whole period later.
    fn due(&mut self, every: Option<Duration>, now: Instant) -> bool {
        match every {
            Some(every) if now.duration_since(self.last) >= every => {
                self.last = now;
                true
            }
            Some(_) => false,
            None => {
                self.last = now;
                false
            }
        }
    }
}

/// Run `s` on the terminal until a key ends it or a signal comes. Returns
/// the signal, if one ended it; the terminal is restored by then, and the
/// caller exits with 128 plus it.
pub fn run_screen(s: &mut dyn Screen) -> io::Result<Option<i32>> {
    crate::with_terminal(|term, signals| run_on(s, term, signals))
}

/// The loop, on the terminal [`run_screen`] set up. Crate-private, so a
/// screen always gets the guard, the signals and the panic hook.
pub(crate) fn run_on(s: &mut dyn Screen, term: &mut DefaultTerminal, signals: &Signals) -> io::Result<Option<i32>> {
    let mut ticker = Ticker::new(Instant::now());
    loop {
        if let Some(sig) = signals.caught() {
            signals.take_up();
            return Ok(Some(sig));
        }
        let wait = ticker.wait(s.tick_every(), Instant::now());
        // A terminal that can no longer be drawn on or read from has hung
        // up: end as on SIGHUP.
        let event = match term.draw(|f| frame(s, f)).and_then(|_| event::poll(wait)) {
            Ok(true) => event::read().map(Some),
            Ok(false) => Ok(None),
            Err(e) => Err(e),
        };
        match event {
            Err(_) => return Ok(Some(HANGUP)),
            // A repeat is a key too: holding Backspace in a text entry.
            Ok(Some(Event::Key(k))) if k.kind != KeyEventKind::Release && !s.key(k) => return Ok(None),
            _ => {}
        }
        if ticker.due(s.tick_every(), Instant::now()) {
            s.tick();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit;
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};
    use ratatui::widgets::Paragraph;

    struct Count(u32);

    impl Screen for Count {
        fn palette(&self) -> Palette {
            Palette::default()
        }
        fn draw(&mut self, f: &mut Frame) {
            f.render_widget(Paragraph::new(format!("n={}", self.0)), f.area());
        }
        fn key(&mut self, k: KeyEvent) -> bool {
            self.0 += 1;
            k.code != KeyCode::Char('q')
        }
    }

    /// A screen that starts ticking, as a replay does when play starts,
    /// waits a whole period for its first tick, however long it sat idle.
    #[test]
    fn the_first_tick_waits_a_whole_period_after_ticking_starts() {
        let t0 = Instant::now();
        let mut t = Ticker::new(t0);
        let every = Duration::from_millis(1000);
        for s in 1..=5 {
            assert!(!t.due(None, t0 + Duration::from_secs(s)), "idle: no tick asked for");
        }
        let start = t0 + Duration::from_secs(5);
        assert!(!t.due(Some(every), start), "the first frame advanced as play started");
        assert_eq!(t.wait(Some(every), start), POLL);
        assert!(!t.due(Some(every), start + Duration::from_millis(999)));
        assert!(t.due(Some(every), start + every));
        assert!(!t.due(Some(every), start + every + Duration::from_millis(1)), "taken once");
    }

    #[test]
    fn the_test_kit_draws_and_drives_the_screen_headless() {
        let mut c = Count(0);
        assert!(testkit::drive(&mut c, &[KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)]));
        let buf = testkit::render_screen(&mut c, 6, 1);
        assert_eq!(testkit::text(&buf), "n=1   ");
        assert!(!testkit::drive(&mut c, &[KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)]));
    }
}
