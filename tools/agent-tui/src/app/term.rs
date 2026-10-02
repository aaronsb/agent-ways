//! The run loop on a real terminal: drawing, input, the tick of a running
//! apply and the file watch, and the guard that restores the terminal on
//! every way out: a quit, an error, a panic, or a signal (SIGTERM, SIGHUP,
//! SIGINT), which the loop sees between polls.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use ratatui::crossterm::cursor::Show;

use super::*;

/// How long the loop waits for input before it looks at the tick, the watch
/// and the signals again.
const POLL: Duration = Duration::from_millis(100);

/// Put the terminal back as the shell found it: mouse reporting off, raw
/// mode off, the main screen back, the cursor shown. Safe to call twice.
pub fn restore() {
    let _ = execute!(io::stdout(), DisableMouseCapture);
    ratatui::restore();
    let _ = execute!(io::stdout(), Show);
}

/// The terminal in raw mode on the alternate screen, restored when dropped.
pub struct TermGuard {
    pub term: DefaultTerminal,
}

impl TermGuard {
    /// Take the terminal. A panic restores it too: `ratatui::init`'s hook
    /// and the one [`crate::run`] adds, which matter where a panic aborts
    /// without unwinding.
    pub fn new() -> TermGuard {
        TermGuard { term: ratatui::init() }
    }
}

impl Default for TermGuard {
    fn default() -> TermGuard {
        TermGuard::new()
    }
}

impl Drop for TermGuard {
    fn drop(&mut self) {
        restore();
    }
}

/// The termination signals, caught into a flag the loop reads, so it can
/// stop a running command and restore the terminal before the process ends.
#[derive(Clone, Default)]
pub struct Signals(Arc<AtomicUsize>);

impl Signals {
    /// Catch SIGTERM, SIGINT and (on Unix) SIGHUP.
    pub fn install() -> io::Result<Signals> {
        let s = Signals::default();
        #[cfg(unix)]
        let caught = [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT, signal_hook::consts::SIGHUP];
        #[cfg(not(unix))]
        let caught = [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT];
        for sig in caught {
            signal_hook::flag::register_usize(sig, s.0.clone(), sig as usize)?;
        }
        Ok(s)
    }

    /// The signal caught, if one was.
    pub fn caught(&self) -> Option<i32> {
        match self.0.load(Ordering::SeqCst) {
            0 => None,
            n => Some(n as i32),
        }
    }
}

impl App {
    /// Run until quit or a signal. Mouse capture follows `self.mouse`; the
    /// guard the caller holds turns it off with the rest.
    pub fn run(mut self, term: &mut DefaultTerminal, signals: &Signals) -> io::Result<Session> {
        let mut captured = false;
        let (mut last_tick, mut last_watch) = (Instant::now(), Instant::now());
        loop {
            if let Some(sig) = signals.caught() {
                self.stop_run("stopped by a signal");
                return Ok(self.session(Some(sig)));
            }
            if self.mouse != captured {
                if self.mouse {
                    execute!(io::stdout(), EnableMouseCapture)?;
                } else {
                    execute!(io::stdout(), DisableMouseCapture)?;
                }
                captured = self.mouse;
            }
            term.draw(|f| self.draw(f))?;
            if event::poll(POLL)? {
                match event::read()? {
                    Event::Key(k) if k.kind == KeyEventKind::Press && !self.key(k) => return Ok(self.session(None)),
                    Event::Mouse(m) => self.mouse(m),
                    _ => {}
                }
            }
            // Timed, not on an idle poll: a moving mouse sends events all the
            // time and would otherwise starve both.
            if self.applying() {
                if last_tick.elapsed() >= TICK {
                    self.tick();
                    last_tick = Instant::now();
                }
            } else if last_watch.elapsed() >= WATCH {
                self.watch();
                last_watch = Instant::now();
            }
        }
    }

    fn session(self, signal: Option<i32>) -> Session {
        Session { roots: self.roots, queue: self.queue, signal }
    }
}
