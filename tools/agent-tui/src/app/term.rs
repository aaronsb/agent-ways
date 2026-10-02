//! The run loop on a real terminal: drawing, input, the tick of a running
//! apply and the file watch, and the guard that restores the terminal on
//! every way out: a quit, an error, a panic, or a signal (SIGTERM, SIGHUP,
//! SIGINT), which the loop sees between polls.

use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use ratatui::crossterm::cursor::Show;

use super::*;

/// How long the loop waits for input before it looks at the tick, the watch
/// and the signals again.
pub(crate) const POLL: Duration = Duration::from_millis(100);

/// Put the terminal back as the shell found it: mouse reporting off, raw
/// mode off, the main screen back, the cursor shown. Safe to call twice.
pub fn restore() {
    let _ = execute!(io::stdout(), DisableMouseCapture);
    // try_restore: restore() prints its error, and a print to a terminal
    // that hung up panics, which in a drop aborts.
    let _ = ratatui::try_restore();
    let _ = execute!(io::stdout(), Show);
}

/// The terminal in raw mode on the alternate screen, restored when dropped.
///
/// The terminal itself is never dropped: ratatui's drop shows the cursor
/// and prints when that fails, and a print to a terminal that hung up
/// panics, which with `panic = "abort"` dumps core. [`restore`] has shown
/// the cursor already.
pub struct TermGuard {
    pub term: ManuallyDrop<DefaultTerminal>,
}

impl TermGuard {
    /// Take the terminal. A panic restores it too: `ratatui::init`'s hook
    /// and the one [`crate::run`] adds, which matter where a panic aborts
    /// without unwinding. The line settings from before are kept, for the
    /// restore a second signal makes from its handler.
    pub fn new() -> TermGuard {
        save_termios();
        TermGuard { term: ManuallyDrop::new(ratatui::init()) }
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

/// The terminal's line settings before raw mode, for the signal handler.
#[cfg(unix)]
static SAVED_TERMIOS: OnceLock<libc::termios> = OnceLock::new();

/// The escapes that turn mouse reporting off, leave the alternate screen
/// and show the cursor, built once, for the signal handler to write.
static RESET: OnceLock<Vec<u8>> = OnceLock::new();

fn save_termios() {
    #[cfg(unix)]
    {
        // SAFETY: termios is plain data the call fills.
        let mut t: libc::termios = unsafe { std::mem::zeroed() };
        // SAFETY: fd 0 is ours to query.
        if unsafe { libc::tcgetattr(0, &mut t) } == 0 {
            let _ = SAVED_TERMIOS.set(t);
        }
    }
    let _ = RESET.get_or_init(|| {
        ["?1000l", "?1002l", "?1003l", "?1006l", "?1015l", "?1049l", "?25h"]
            .iter()
            .flat_map(|s| [&[0x1b, b'['][..], s.as_bytes()].concat())
            .collect()
    });
}

/// A second signal while the first is not yet acted on: end the command's
/// process group, put the terminal back and exit, from the handler. Only
/// async-signal-safe calls: an atomic load, killpg, write, tcsetattr and
/// _exit.
#[cfg(unix)]
fn emergency_exit(sig: i32) -> ! {
    let pg = JOB_GROUP.load(Ordering::SeqCst);
    // SAFETY: each call below is async-signal-safe and touches only data
    // set up before the handler was installed.
    unsafe {
        if pg > 0 {
            libc::killpg(pg, libc::SIGKILL);
        }
        if let Some(r) = RESET.get() {
            libc::write(1, r.as_ptr().cast(), r.len());
        }
        if let Some(t) = SAVED_TERMIOS.get() {
            libc::tcsetattr(0, libc::TCSANOW, t);
        }
        libc::_exit(128 + sig)
    }
}

/// The process group of the command in flight, which a forced exit and the
/// panic hook end with it. 0 when there is none.
static JOB_GROUP: AtomicI32 = AtomicI32::new(0);

/// Note the process group of a command now running, so that whatever way
/// the screens end, the command and every process it started end too.
pub fn register_job_group(pgid: u32) {
    JOB_GROUP.store(pgid as i32, Ordering::SeqCst);
}

/// The command in group `pgid` has ended.
pub fn clear_job_group(pgid: u32) {
    let _ = JOB_GROUP.compare_exchange(pgid as i32, 0, Ordering::SeqCst, Ordering::SeqCst);
}

/// End the command in flight and every process in its group.
pub fn kill_job_group() {
    let pg = JOB_GROUP.swap(0, Ordering::SeqCst);
    if pg > 0 {
        kill_group(pg as u32);
    }
}

/// Send SIGKILL to every process in group `pgid` (Unix; elsewhere nothing).
pub fn kill_group(pgid: u32) {
    #[cfg(unix)]
    // SAFETY: killpg only sends a signal; a group already gone is ESRCH.
    unsafe {
        libc::killpg(pgid as i32, libc::SIGKILL);
    }
    #[cfg(not(unix))]
    let _ = pgid;
}

/// The signal a hung-up terminal stands for.
#[cfg(unix)]
pub(crate) const HANGUP: i32 = libc::SIGHUP;
#[cfg(not(unix))]
pub(crate) const HANGUP: i32 = 1;

/// How long the loop has to act on a signal before the watch thread ends
/// the process itself, and how long once the loop has taken it up.
const UNSEEN: Duration = Duration::from_millis(500);
const SEEN: Duration = Duration::from_secs(3);

/// The termination signals, caught into a flag the loop reads, so it can
/// stop a running command and restore the terminal before the process ends.
///
/// The loop can be kept from reading it: on a terminal that hung up,
/// crossterm's read returns nothing over and over inside one poll. So a
/// watch thread gives the loop [`UNSEEN`] to take the signal up, then ends
/// the command's process group, restores what it can and exits with 128
/// plus the signal; a hung-up terminal, seen as `tcgetattr` failing on a
/// stdin that was a terminal, counts as SIGHUP. A second signal exits at
/// once, whatever state the process is in.
#[derive(Clone, Default)]
pub struct Signals {
    caught: Arc<AtomicUsize>,
    /// Set by the loop when it acts on the signal.
    seen: Arc<AtomicBool>,
}

impl Signals {
    /// Catch SIGTERM, SIGINT and (on Unix) SIGHUP, and start the watch.
    pub fn install() -> io::Result<Signals> {
        let s = Signals::default();
        #[cfg(unix)]
        let caught = [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT, signal_hook::consts::SIGHUP];
        #[cfg(not(unix))]
        let caught = [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT];
        save_termios();
        for sig in caught {
            // The first signal of any kind arms it; a second, of any kind,
            // ends the process from the handler, the job and terminal first.
            #[cfg(unix)]
            {
                static ARMED: AtomicBool = AtomicBool::new(false);
                // SAFETY: the handler makes only async-signal-safe calls.
                unsafe {
                    signal_hook::low_level::register(sig, move || {
                        if ARMED.swap(true, Ordering::SeqCst) {
                            emergency_exit(sig);
                        }
                    })?;
                }
            }
            signal_hook::flag::register_usize(sig, s.caught.clone(), sig as usize)?;
        }
        let w = s.clone();
        std::thread::Builder::new().name("agent-tui-signals".into()).spawn(move || w.watch())?;
        Ok(s)
    }

    /// The signal caught, if one was.
    pub fn caught(&self) -> Option<i32> {
        match self.caught.load(Ordering::SeqCst) {
            0 => None,
            n => Some(n as i32),
        }
    }

    /// The loop is acting on the signal.
    pub fn take_up(&self) {
        self.seen.store(true, Ordering::SeqCst);
    }

    fn watch(&self) {
        let tty = stdin_is_tty();
        let mut since: Option<Instant> = None;
        loop {
            std::thread::sleep(Duration::from_millis(50));
            if self.caught().is_none() && tty && hung_up() {
                #[cfg(unix)]
                self.caught.store(libc::SIGHUP as usize, Ordering::SeqCst);
            }
            let Some(sig) = self.caught() else { continue };
            let start = *since.get_or_insert_with(Instant::now);
            let limit = if self.seen.load(Ordering::SeqCst) { SEEN } else { UNSEEN };
            if start.elapsed() >= limit {
                kill_job_group();
                restore();
                std::process::exit(128 + sig);
            }
        }
    }
}

fn stdin_is_tty() -> bool {
    use std::io::IsTerminal;
    io::stdin().is_terminal()
}

/// Whether the terminal on stdin has gone: its attributes can no longer be
/// read, as after a hangup.
fn hung_up() -> bool {
    #[cfg(unix)]
    {
        // SAFETY: termios is plain data the call fills.
        let mut t: libc::termios = unsafe { std::mem::zeroed() };
        // SAFETY: fd 0 is ours to query.
        unsafe { libc::tcgetattr(0, &mut t) != 0 }
    }
    #[cfg(not(unix))]
    false
}

impl App {
    /// Run until quit or a signal. Mouse capture follows `self.mouse`; the
    /// guard the caller holds turns it off with the rest.
    pub fn run(mut self, term: &mut DefaultTerminal, signals: &Signals) -> io::Result<Session> {
        let mut captured = false;
        let (mut last_tick, mut last_watch) = (Instant::now(), Instant::now());
        loop {
            if let Some(sig) = signals.caught() {
                signals.take_up();
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
            // A terminal that can no longer be drawn on or read from has hung
            // up: end as on SIGHUP.
            let event = match term.draw(|f| self.draw(f)).and_then(|_| event::poll(POLL)) {
                Ok(true) => event::read().map(Some),
                Ok(false) => Ok(None),
                Err(e) => Err(e),
            };
            match event {
                Err(_) => {
                    self.stop_run("stopped: the terminal hung up");
                    return Ok(self.session(Some(HANGUP)));
                }
                Ok(Some(Event::Key(k))) if k.kind == KeyEventKind::Press && !self.key(k) => return Ok(self.session(None)),
                Ok(Some(Event::Mouse(m))) => self.mouse(m),
                Ok(_) => {}
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
