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

use crate::app::term::{restore, Signals, TermGuard};
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

/// Run `s` on the terminal until a key ends it or a signal comes. Returns
/// the signal, if one ended it; the terminal is restored by then, and the
/// caller exits with 128 plus it.
pub fn run(s: &mut dyn Screen) -> io::Result<Option<i32>> {
    let signals = Signals::install()?;
    let hook = std::panic::take_hook();
    let mut guard = TermGuard::new();
    // ratatui::init's own hook restores less; this one replaces it.
    let _ratatui_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        hook(info);
    }));
    let term = &mut guard.term;
    let mut last_tick = Instant::now();
    loop {
        if let Some(sig) = signals.caught() {
            signals.take_up();
            return Ok(Some(sig));
        }
        let wait = match s.tick_every() {
            Some(every) => every.saturating_sub(last_tick.elapsed()).min(POLL),
            None => POLL,
        };
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
        if let Some(every) = s.tick_every() {
            if last_tick.elapsed() >= every {
                s.tick();
                last_tick = Instant::now();
            }
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

    #[test]
    fn render_draws_the_screen_headless() {
        let mut c = Count(0);
        assert!(c.key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)));
        let buf = render(&mut c, 6, 1);
        assert_eq!(testkit::text(&buf), "n=1   ");
        assert!(!c.key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)));
    }
}
