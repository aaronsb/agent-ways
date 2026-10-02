//! A screen that is not the settings shell: an application draws it and
//! handles its keys, and this runs it on the terminal, or renders it
//! headless for the test kit (ADR-504 §3, §12).
//!
//! [`run`] restores the terminal on every way out, as [`crate::run`] does,
//! and installs the same [`Signals`]. The signal handlers stay installed
//! for the life of the process, so a process runs one session: put every
//! screen it shows behind one [`Screen`], and end the process when `run`
//! returns.

use std::io;
use std::time::{Duration, Instant};

use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use ratatui::{Frame, Terminal};

use crate::app::term::{kill_job_group, restore, Signals, TermGuard};
use crate::theme::{self, Palette};

/// What an application supplies for one screen session.
pub trait Screen {
    /// The palette the frame draws in. It is set before [`Screen::draw`]
    /// and every cell left on the terminal's ground gets the theme's after.
    fn palette(&self) -> Palette;

    fn draw(&mut self, f: &mut Frame);

    /// Handle a key. False ends the session.
    fn key(&mut self, k: KeyEvent) -> bool;

    /// How often [`Screen::tick`] runs; `None` never.
    fn tick_every(&self) -> Option<Duration> {
        None
    }

    /// Time has passed: play a frame on, or read a source again.
    fn tick(&mut self) {}
}

fn draw(s: &mut dyn Screen, f: &mut Frame) {
    theme::set(s.palette());
    s.draw(f);
    theme::fill(f.buffer_mut());
}

/// One frame of `s` at `w` by `h`, drawn as [`run`] draws it.
pub fn render(s: &mut dyn Screen, w: u16, h: u16) -> Buffer {
    let mut term = Terminal::new(TestBackend::new(w, h)).expect("a test backend");
    term.draw(|f| draw(s, f)).expect("a frame");
    term.backend().buffer().clone()
}

/// How long the loop waits for input at most before it looks at the
/// signals and the tick again.
const POLL: Duration = Duration::from_millis(100);

/// The signal a hung-up terminal stands for.
#[cfg(unix)]
const HANGUP: i32 = libc::SIGHUP;
#[cfg(not(unix))]
const HANGUP: i32 = 1;

/// A panic hook, as `std::panic::set_hook` takes one.
#[doc(hidden)]
pub type Hook = Box<dyn Fn(&std::panic::PanicHookInfo) + Send + Sync>;

/// The panic hook a screen session runs under: end the command in flight
/// and restore the terminal, then run `prev`, the hook that was in place.
#[doc(hidden)]
pub fn panic_hook(prev: Hook) -> Hook {
    Box::new(move |info| {
        // A panic that aborts runs no drop: end the command in flight here,
        // as `crate::run`'s hook does.
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
pub fn run(s: &mut dyn Screen) -> io::Result<Option<i32>> {
    let signals = Signals::install()?;
    let hook = std::panic::take_hook();
    let mut guard = TermGuard::new();
    // ratatui::init's own hook restores less; this one replaces it.
    let _ratatui_hook = std::panic::take_hook();
    std::panic::set_hook(panic_hook(hook));
    let term = &mut guard.term;
    let mut ticker = Ticker::new(Instant::now());
    loop {
        if let Some(sig) = signals.caught() {
            signals.take_up();
            return Ok(Some(sig));
        }
        let wait = ticker.wait(s.tick_every(), Instant::now());
        // A terminal that can no longer be drawn on or read from has hung
        // up: end as on SIGHUP.
        let event = match term.draw(|f| draw(s, f)).and_then(|_| event::poll(wait)) {
            Ok(true) => event::read().map(Some),
            Ok(false) => Ok(None),
            Err(e) => Err(e),
        };
        match event {
            Err(_) => return Ok(Some(HANGUP)),
            Ok(Some(Event::Key(k))) if k.kind == KeyEventKind::Press && !s.key(k) => return Ok(None),
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
    fn render_draws_the_screen_headless() {
        let mut c = Count(0);
        assert!(c.key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)));
        let buf = render(&mut c, 6, 1);
        assert_eq!(testkit::text(&buf), "n=1   ");
        assert!(!c.key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)));
    }
}
