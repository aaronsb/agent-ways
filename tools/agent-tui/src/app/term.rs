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

/// Whether the keyboard enhancement is pushed on the terminal's stack, so
/// every way out pops it: a quit, a panic and a signal.
static KEYBOARD_PUSHED: AtomicBool = AtomicBool::new(false);

/// The escape that pops the keyboard enhancement, for the signal handler.
const KEYBOARD_POP: &[u8] = b"\x1b[<u";

/// Ask the terminal to report Ctrl+digits and the other keys that legacy
/// encoding folds into others (Ctrl+3 is Esc, Ctrl+8 is Backspace): the
/// kitty keyboard protocol's first flag, DISAMBIGUATE_ESCAPE_CODES. Only
/// when the terminal answers the protocol's query; returns whether it is
/// on. [`restore`] pops it.
pub(crate) fn enable_keyboard_enhancement() -> bool {
    use ratatui::crossterm::event::{KeyboardEnhancementFlags, PushKeyboardEnhancementFlags};
    if !matches!(ratatui::crossterm::terminal::supports_keyboard_enhancement(), Ok(true)) {
        return false;
    }
    let flags = PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES);
    if execute!(io::stdout(), flags).is_err() {
        return false;
    }
    KEYBOARD_PUSHED.store(true, Ordering::SeqCst);
    true
}

/// Put the terminal back as the shell found it: the keyboard enhancement
/// popped, mouse reporting off, raw mode off, the main screen back, the
/// cursor shown. Safe to call twice.
pub fn restore() {
    if KEYBOARD_PUSHED.swap(false, Ordering::SeqCst) {
        let _ = execute!(io::stdout(), ratatui::crossterm::event::PopKeyboardEnhancementFlags);
    }
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
        if KEYBOARD_PUSHED.load(Ordering::SeqCst) {
            libc::write(1, KEYBOARD_POP.as_ptr().cast(), KEYBOARD_POP.len());
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

/// Mouse reporting for clicks, the wheel and drags, without the any-motion
/// mode crossterm's `EnableMouseCapture` adds: no screen here needs a bare
/// move, and with it every move of the mouse is an event and a frame.
/// `DisableMouseCapture` turns this off with the rest.
#[derive(Debug, Clone, Copy)]
pub struct EnableButtonMouse;

impl ratatui::crossterm::Command for EnableButtonMouse {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        // Press and release, then drags, in the SGR encoding.
        f.write_str(concat!(ratatui::crossterm::csi!("?1000h"), ratatui::crossterm::csi!("?1002h"), ratatui::crossterm::csi!("?1006h")))
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        ratatui::crossterm::Command::execute_winapi(&ratatui::crossterm::event::EnableMouseCapture)
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        false
    }
}

/// One periodic job of the run loop: when it last ran.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Clock {
    last: Instant,
}

impl Clock {
    fn new(now: Instant) -> Clock {
        Clock { last: now }
    }

    /// How long until it is due every `every`; zero when it is.
    fn left(&self, every: Duration, now: Instant) -> Duration {
        every.saturating_sub(now.duration_since(self.last))
    }

    /// Whether it is due at `now`, taking it when it is.
    fn take(&mut self, every: Duration, now: Instant) -> bool {
        if now.duration_since(self.last) >= every {
            self.last = now;
            return true;
        }
        false
    }

    /// Not running: its first run comes a whole period after it starts.
    fn idle(&mut self, now: Instant) {
        self.last = now;
    }
}

/// The run loop's three periodic jobs, each on its own clock: the pane's
/// tick, the shell's tick while an apply or a reading runs, and the watch
/// for files changed on disk while none does. One never waits on another.
pub(crate) struct Schedule {
    pane: Clock,
    shell: Clock,
    watch: Clock,
}

/// What is due on one pass of the loop.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Due {
    pub(crate) pane: bool,
    pub(crate) shell: bool,
    pub(crate) watch: bool,
}

impl Schedule {
    pub(crate) fn new(now: Instant) -> Schedule {
        Schedule { pane: Clock::new(now), shell: Clock::new(now), watch: Clock::new(now) }
    }

    /// How long to wait for input before the next job is due, at most
    /// [`POLL`], so a signal is seen in time.
    pub(crate) fn wait(&self, pane: Option<Duration>, busy: bool, now: Instant) -> Duration {
        let mut wait = POLL;
        if let Some(every) = pane {
            wait = wait.min(self.pane.left(every, now));
        }
        wait.min(if busy { self.shell.left(TICK, now) } else { self.watch.left(WATCH, now) })
    }

    /// The jobs due at `now`, taken. `pane` is how often the pane ticks,
    /// `None` never; `busy` whether an apply or a reading runs.
    pub(crate) fn due(&mut self, pane: Option<Duration>, busy: bool, now: Instant) -> Due {
        let pane = match pane {
            Some(every) => self.pane.take(every, now),
            None => {
                self.pane.idle(now);
                false
            }
        };
        let (shell, watch) = if busy {
            self.watch.idle(now);
            (self.shell.take(TICK, now), false)
        } else {
            self.shell.idle(now);
            (false, self.watch.take(WATCH, now))
        };
        Due { pane, shell, watch }
    }
}

impl App {
    /// Run until quit or a signal. Mouse capture follows `self.mouse`; the
    /// guard the caller holds turns it off with the rest.
    pub fn run(mut self, term: &mut DefaultTerminal, signals: &Signals) -> io::Result<Session> {
        let mut captured = false;
        let mut schedule = Schedule::new(Instant::now());
        if self.pane.as_ref().is_some_and(|p| p.keyboard_enhancement()) {
            // A frame first: the probe waits on the terminal's answer, up
            // to crossterm's timeout where none comes, and a blank screen
            // through that wait reads as a hang.
            term.draw(|f| self.draw(f))?;
            let on = enable_keyboard_enhancement();
            self.set_keyboard_enhanced(on);
        }
        loop {
            if let Some(sig) = signals.caught() {
                signals.take_up();
                self.stop_run("stopped by a signal");
                return Ok(self.session(Some(sig)));
            }
            if self.mouse != captured {
                if self.mouse {
                    execute!(io::stdout(), EnableButtonMouse)?;
                } else {
                    execute!(io::stdout(), DisableMouseCapture)?;
                }
                captured = self.mouse;
            }
            let wait = schedule.wait(self.pane_tick_every(), self.applying(), Instant::now());
            // A terminal that can no longer be drawn on or read from has hung
            // up: end as on SIGHUP.
            let event = match term.draw(|f| self.draw(f)).and_then(|_| event::poll(wait)) {
                Ok(true) => event::read().map(Some),
                Ok(false) => Ok(None),
                Err(e) => Err(e),
            };
            match event {
                Err(_) => {
                    self.stop_run("stopped: the terminal hung up");
                    return Ok(self.session(Some(HANGUP)));
                }
                // A repeat is a key too: holding Backspace in a text entry.
                Ok(Some(Event::Key(k))) if k.kind != KeyEventKind::Release && !self.key(k) => {
                    // A check still running ends with the screen.
                    self.stop_run("stopped: the screen closed");
                    return Ok(self.session(None));
                }
                Ok(Some(Event::Mouse(m))) => self.mouse(m),
                Ok(_) => {}
            }
            // Timed, not on an idle poll: input arriving all the time would
            // otherwise starve them.
            let due = schedule.due(self.pane_tick_every(), self.applying(), Instant::now());
            if due.pane {
                self.tick_pane();
            }
            if due.shell {
                self.tick();
            }
            if due.watch {
                self.watch();
            }
        }
    }

    fn session(self, signal: Option<i32>) -> Session {
        Session { roots: self.roots, queue: self.queue, signal }
    }
}

#[cfg(test)]
mod schedule_tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// A pane ticking every 100ms while an apply runs (the shell's tick is
    /// 150ms): each is due on its own clock, neither riding the other's.
    #[test]
    fn a_pane_and_an_apply_tick_on_their_own_clocks() {
        let t0 = Instant::now();
        let mut s = Schedule::new(t0);
        let pane = Some(ms(100));
        assert_eq!(s.due(pane, true, t0 + ms(100)), Due { pane: true, shell: false, watch: false });
        assert_eq!(s.due(pane, true, t0 + ms(150)), Due { pane: false, shell: true, watch: false });
        assert_eq!(s.due(pane, true, t0 + ms(200)), Due { pane: true, shell: false, watch: false });
    }

    /// The watch runs while a pane ticks and nothing is applying.
    #[test]
    fn the_watch_runs_beside_a_ticking_pane() {
        let t0 = Instant::now();
        let mut s = Schedule::new(t0);
        let (mut ticked, mut watched) = (0, 0);
        for i in 1..=12 {
            let d = s.due(Some(ms(100)), false, t0 + ms(100 * i));
            ticked += usize::from(d.pane);
            watched += usize::from(d.watch);
        }
        assert_eq!((ticked, watched), (12, 1));
    }

    /// A pane that never ticks is not ticked while an apply runs.
    #[test]
    fn a_pane_with_no_tick_is_never_due() {
        let t0 = Instant::now();
        let mut s = Schedule::new(t0);
        for i in 1..=20 {
            assert!(!s.due(None, true, t0 + ms(50 * i)).pane);
        }
    }

    /// The loop waits for input only until the soonest job is due, and
    /// never past POLL, so a signal is seen in time.
    #[test]
    fn the_wait_is_until_the_soonest_job() {
        let t0 = Instant::now();
        let s = Schedule::new(t0);
        assert_eq!(s.wait(Some(ms(40)), false, t0 + ms(10)), ms(30));
        assert_eq!(s.wait(None, true, t0 + ms(100)), ms(50));
        assert_eq!(s.wait(None, false, t0), POLL);
    }
}
