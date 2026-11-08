// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The run loop on a real terminal: drawing, input, the tick of a running
//! apply and the file watch, and the guard that restores the terminal on
//! every way out: a quit, an error, a panic, or a signal (SIGTERM, SIGHUP,
//! SIGINT), which the loop sees between polls.

use std::io::Write;
use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use ratatui::crossterm::cursor::Show;

use super::*;
use super::paneshell::QUESTION_TICK;

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
    let _ = execute!(io::stdout(), DisableMouseCapture, ratatui::crossterm::event::DisableBracketedPaste);
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
        let term = ManuallyDrop::new(ratatui::init());
        // A paste arrives as one event, not as keys: its newlines would
        // otherwise read as Enter, one per line.
        let _ = execute!(io::stdout(), ratatui::crossterm::event::EnableBracketedPaste);
        TermGuard { term }
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
        ["?1000l", "?1002l", "?1003l", "?1006l", "?1015l", "?2004l", "?1049l", "?25h"]
            .iter()
            .flat_map(|s| [&[0x1b, b'['][..], s.as_bytes()].concat())
            .collect()
    });
}

/// What the application keeps before a forced exit, set once by
/// [`on_forced_exit`].
static BEFORE_FORCED_EXIT: OnceLock<fn()> = OnceLock::new();

/// Set once a forced exit has begun, so a signal that comes while the hook
/// runs ends the process from the handler.
static FORCING: AtomicBool = AtomicBool::new(false);

/// A forcing signal the handler hands to the watch thread ([`forces`]); 0
/// while there is none.
static FORCE: AtomicI32 = AtomicI32::new(0);

/// Run `keep` before every forced exit: the watch thread's two limits
/// ([`UNSEEN`], [`SEEN`]) and a second SIGTERM or SIGINT ([`forces`]).
/// The pane's ending ([`crate::Pane::ending`]) does not run on those
/// exits, so this is what keeps the application's unsaved work there.
/// `keep` runs on a thread of its own while the run loop may be wedged,
/// even inside crossterm's read: it may write files from state the
/// application noted, and must not wait on a lock the loop could hold
/// without a bound. The terminal is put back before it. It has
/// [`HOOK_BOUND`] to end; past that the process exits without it, so a
/// write that blocks, on a hung network mount for one, never holds the
/// exit. Set once; false when a hook was set already.
pub fn on_forced_exit(keep: fn()) -> bool {
    BEFORE_FORCED_EXIT.set(keep).is_ok()
}

/// How long a forced exit waits for the hook ([`on_forced_exit`]) before
/// it exits without it.
const HOOK_BOUND: Duration = Duration::from_secs(3);

/// The thread that runs the hook, started with the watch
/// ([`Signals::install`]) so a forced exit starts no thread.
static KEEPER: OnceLock<std::thread::Thread> = OnceLock::new();

/// Set by a forced exit to start the hook on [`KEEPER`].
static KEEP_START: AtomicBool = AtomicBool::new(false);

/// Set by [`KEEPER`] once the hook has returned.
static KEEP_DONE: AtomicBool = AtomicBool::new(false);

/// The keeper thread: it waits for a forced exit, runs the hook once, and
/// says so.
fn keeper() {
    while !KEEP_START.load(Ordering::SeqCst) {
        std::thread::park();
    }
    if let Some(keep) = BEFORE_FORCED_EXIT.get() {
        keep();
    }
    KEEP_DONE.store(true, Ordering::SeqCst);
}

/// Run the hook on [`KEEPER`] and wait until it returns or `bound` has
/// passed, whichever comes first. With no keeper, which only a failed
/// [`Signals::install`] leaves, the hook runs here.
fn keep_within(bound: Duration) {
    let Some(keep) = BEFORE_FORCED_EXIT.get() else { return };
    let Some(keeper) = KEEPER.get() else {
        keep();
        return;
    };
    KEEP_START.store(true, Ordering::SeqCst);
    keeper.unpark();
    let start = Instant::now();
    while !KEEP_DONE.load(Ordering::SeqCst) && start.elapsed() < bound {
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// End the process without the pane's ending: the terminal back first,
/// then the hook ([`on_forced_exit`]) for at most [`HOOK_BOUND`], then the
/// command's process group and the exit, by [`emergency_exit`], which
/// takes no lock the loop could hold.
fn forced_exit(sig: i32) -> ! {
    FORCING.store(true, Ordering::SeqCst);
    reset_terminal();
    keep_within(HOOK_BOUND);
    emergency_exit(sig)
}

/// Set once a forced exit has written the terminal's reset, so it is
/// written once.
static RESET_WRITTEN: AtomicBool = AtomicBool::new(false);

/// Put the terminal back with no lock taken: the keyboard enhancement
/// popped, the reset escapes written once, the line settings from before.
/// Only async-signal-safe calls: atomics, write and tcsetattr.
#[cfg(unix)]
fn reset_terminal() {
    // SAFETY: each call below is async-signal-safe and touches only data
    // set up before the handler was installed.
    unsafe {
        if !RESET_WRITTEN.swap(true, Ordering::SeqCst) {
            if KEYBOARD_PUSHED.load(Ordering::SeqCst) {
                libc::write(1, KEYBOARD_POP.as_ptr().cast(), KEYBOARD_POP.len());
            }
            if let Some(r) = RESET.get() {
                libc::write(1, r.as_ptr().cast(), r.len());
            }
        }
        if let Some(t) = SAVED_TERMIOS.get() {
            libc::tcsetattr(0, libc::TCSANOW, t);
        }
    }
}

/// Elsewhere [`emergency_exit`] puts the terminal back, by [`restore`],
/// which takes the stdout lock.
#[cfg(not(unix))]
fn reset_terminal() {}

/// End the command's process group, put the terminal back and exit with
/// 128 plus `sig`, with no lock taken. Only async-signal-safe calls: an
/// atomic load, killpg, [`reset_terminal`]'s and _exit, so a signal that
/// comes while [`forced_exit`]'s hook runs ends the process from the
/// handler.
#[cfg(unix)]
fn emergency_exit(sig: i32) -> ! {
    let pg = JOB_GROUP.load(Ordering::SeqCst);
    // SAFETY: killpg and _exit are async-signal-safe.
    unsafe {
        if pg > 0 {
            libc::killpg(pg, libc::SIGKILL);
        }
    }
    reset_terminal();
    // SAFETY: as above.
    unsafe { libc::_exit(128 + sig) }
}

#[cfg(not(unix))]
fn emergency_exit(sig: i32) -> ! {
    kill_job_group();
    restore();
    std::process::exit(128 + sig)
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
/// stop a running command, let the pane end its work ([`crate::Pane::ending`])
/// and restore the terminal before the process ends.
///
/// The loop can be kept from reading it: on a terminal that hung up,
/// crossterm's read returns nothing over and over inside one poll. So a
/// watch thread gives the loop [`UNSEEN`] to take the signal up, and
/// [`SEEN`] to end once it has, then makes a forced exit: the terminal,
/// the hook the application set ([`on_forced_exit`]) for at most
/// [`HOOK_BOUND`], the command's process group and an exit with 128 plus
/// the first signal. A hung-up
/// terminal, seen as `tcgetattr` failing on a stdin that was a terminal,
/// counts as SIGHUP.
///
/// Which signal forces an exit at once is [`forces`]: a second SIGTERM or
/// SIGINT, while the loop has not yet taken the first signal up. The
/// handler hands it to the watch thread, which makes the same forced exit
/// within its next 50 ms. A SIGHUP never forces. A terminal
/// that closes sends several: the kernel signals the foreground group, the
/// shell sends it again to its jobs, and tmux signals the pane's group.
/// Once the loop has taken a signal up, no signal forces, so the pane's
/// ending runs to its end or to [`SEEN`].
#[derive(Clone, Default)]
pub struct Signals {
    caught: Arc<AtomicUsize>,
    /// Set by the loop when it acts on the signal.
    seen: Arc<AtomicBool>,
}

/// Whether a SIGTERM or SIGINT has come, for [`forces`].
#[cfg(unix)]
static DELIBERATE: AtomicBool = AtomicBool::new(false);

/// Whether a signal forces an exit at once, without the pane's ending:
/// only a SIGTERM or SIGINT (`deliberate`) that follows an earlier one
/// (`earlier`), while the loop has not yet taken the first signal up
/// (`seen`). There the loop may be wedged, even on the stdout lock, so the
/// forced exit takes no lock of the loop's ([`forced_exit`]). A SIGHUP is
/// never the forcing signal.
/// Once the loop has taken a signal up, its exit runs, the pane's ending
/// with it, and the watch thread's [`SEEN`] bounds an exit that hangs.
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) fn forces(deliberate: bool, earlier: bool, seen: bool) -> bool {
    deliberate && earlier && !seen
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
        stdin_was_tty();
        for sig in caught {
            #[cfg(unix)]
            {
                let (first, seen) = (s.caught.clone(), s.seen.clone());
                // SAFETY: the handler makes only async-signal-safe calls:
                // atomic loads and stores, and `emergency_exit`'s.
                unsafe {
                    signal_hook::low_level::register(sig, move || {
                        // The first signal is the one the exit reports.
                        let _ = first.compare_exchange(0, sig as usize, Ordering::SeqCst, Ordering::SeqCst);
                        let deliberate = sig != libc::SIGHUP;
                        let earlier = deliberate && DELIBERATE.swap(true, Ordering::SeqCst);
                        if forces(deliberate, earlier, seen.load(Ordering::SeqCst)) {
                            // The watch thread runs the forced exit; a
                            // signal that comes once the exit has begun
                            // ends the process here.
                            if FORCING.load(Ordering::SeqCst) {
                                emergency_exit(sig);
                            }
                            let _ = FORCE.compare_exchange(0, sig, Ordering::SeqCst, Ordering::SeqCst);
                        }
                    })?;
                }
            }
            #[cfg(not(unix))]
            signal_hook::flag::register_usize(sig, s.caught.clone(), sig as usize)?;
        }
        if KEEPER.get().is_none() {
            let keeper = std::thread::Builder::new().name("agent-tui-keeper".into()).spawn(keeper)?;
            let _ = KEEPER.set(keeper.thread().clone());
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
        let tty = stdin_was_tty();
        let mut since: Option<Instant> = None;
        loop {
            std::thread::sleep(Duration::from_millis(50));
            match FORCE.load(Ordering::SeqCst) {
                0 => {}
                sig => forced_exit(sig),
            }
            if self.caught().is_none() && tty && hung_up() {
                #[cfg(unix)]
                self.caught.store(libc::SIGHUP as usize, Ordering::SeqCst);
            }
            let Some(sig) = self.caught() else { continue };
            let start = *since.get_or_insert_with(Instant::now);
            let limit = if self.seen.load(Ordering::SeqCst) { SEEN } else { UNSEEN };
            if start.elapsed() >= limit {
                forced_exit(sig);
            }
        }
    }
}

/// The next input event within `wait`, or none; an error once the terminal
/// has hung up. crossterm's poll never returns on a terminal that hung up:
/// its read loop takes a read of nothing, or an error, as no event yet and
/// reads again. So on Unix the terminal is polled here first, and crossterm
/// is asked without waiting, only while the terminal is still there.
/// `more` is set after an event, when crossterm may hold more already read.
pub(crate) fn next_event(wait: Duration, more: bool) -> io::Result<Option<Event>> {
    #[cfg(unix)]
    if stdin_was_tty() {
        let ms = if more { 0 } else { wait.as_millis().min(i32::MAX as u128) as i32 };
        let mut pfd = libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 };
        // SAFETY: one pollfd we own, on fd 0.
        if unsafe { libc::poll(&mut pfd, 1, ms) } < 0 {
            let e = io::Error::last_os_error();
            return if e.kind() == io::ErrorKind::Interrupted { Ok(None) } else { Err(e) };
        }
        if pfd.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 || hung_up() {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "the terminal hung up"));
        }
        return if event::poll(Duration::ZERO)? { event::read().map(Some) } else { Ok(None) };
    }
    if event::poll(wait)? {
        event::read().map(Some)
    } else {
        Ok(None)
    }
}

/// Whether stdin was a terminal when first asked, which [`Signals::install`]
/// does at startup. The answer is kept: `isatty` fails on a terminal that
/// has hung up, and asked again then, it would send [`next_event`] to
/// crossterm's poll, which spins on the dead terminal.
fn stdin_was_tty() -> bool {
    static TTY: OnceLock<bool> = OnceLock::new();
    *TTY.get_or_init(|| {
        use std::io::IsTerminal;
        io::stdin().is_terminal()
    })
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
/// tick, the shell's tick while an apply or a reading runs or a question
/// is on the clock, and the watch for files changed on disk while nothing
/// applies. One never waits on another.
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
    /// [`POLL`], so a signal is seen in time. POLL is shorter than
    /// [`QUESTION_TICK`], so a question on the clock never shortens the
    /// wait past it; it is counted all the same, so the wait stays right
    /// if POLL grows.
    pub(crate) fn wait(&self, pane: Option<Duration>, busy: bool, question: bool, now: Instant) -> Duration {
        let mut wait = POLL;
        if let Some(every) = pane {
            wait = wait.min(self.pane.left(every, now));
        }
        if busy {
            return wait.min(self.shell.left(TICK, now));
        }
        if question {
            wait = wait.min(self.shell.left(QUESTION_TICK, now));
        }
        wait.min(self.watch.left(WATCH, now))
    }

    /// The jobs due at `now`, taken. `pane` is how often the pane ticks,
    /// `None` never; `busy` whether an apply or a reading runs; `question`
    /// whether a question is on the clock ([`App::question_on_the_clock`]),
    /// which ticks the shell every [`QUESTION_TICK`] whether or not the
    /// pane ticks.
    pub(crate) fn due(&mut self, pane: Option<Duration>, busy: bool, question: bool, now: Instant) -> Due {
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
        } else if question {
            (self.shell.take(QUESTION_TICK, now), self.watch.take(WATCH, now))
        } else {
            self.shell.idle(now);
            (false, self.watch.take(WATCH, now))
        };
        Due { pane, shell, watch }
    }
}

impl App {
    /// How long the run loop waits for input, by `schedule`.
    pub(crate) fn schedule_wait(&self, schedule: &Schedule, now: Instant) -> Duration {
        schedule.wait(self.pane_tick_every(), self.applying(), self.question_on_the_clock(), now)
    }

    /// The jobs due at `now` by `schedule`, run: the pane's tick, the
    /// shell's (which closes an expired question), the watch.
    pub(crate) fn run_due(&mut self, schedule: &mut Schedule, now: Instant) {
        let due = schedule.due(self.pane_tick_every(), self.applying(), self.question_on_the_clock(), now);
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

impl App {
    /// Run until quit or a signal. Mouse capture follows `self.mouse`; the
    /// guard the caller holds turns it off with the rest.
    pub fn run(mut self, term: &mut DefaultTerminal, signals: &Signals) -> io::Result<Session> {
        let mut captured = false;
        let mut more = false;
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
                return Ok(self.end_unasked(sig, "stopped by a signal"));
            }
            if self.mouse != captured {
                if self.mouse {
                    execute!(io::stdout(), EnableButtonMouse)?;
                } else {
                    execute!(io::stdout(), DisableMouseCapture)?;
                }
                captured = self.mouse;
            }
            // Text a pane copied goes to the terminal's clipboard by OSC 52.
            if let Some(osc) = self.take_osc52() {
                let mut out = io::stdout();
                let _ = out.write_all(osc.as_bytes()).and_then(|_| out.flush());
            }
            let wait = self.schedule_wait(&schedule, Instant::now());
            // A terminal that can no longer be drawn on or read from has hung
            // up: end as on SIGHUP, taken up so the watch thread gives the
            // pane's ending its time.
            let event = term.draw(|f| self.draw(f)).and_then(|_| next_event(wait, more));
            more = matches!(event, Ok(Some(_)));
            match event {
                Err(_) => {
                    signals.take_up();
                    return Ok(self.end_unasked(HANGUP, "stopped: the terminal hung up"));
                }
                // A repeat is a key too: holding Backspace in a text entry.
                Ok(Some(Event::Key(k))) if k.kind != KeyEventKind::Release && !self.key(k) => {
                    // A check still running ends with the screen.
                    self.stop_run("stopped: the screen closed");
                    return Ok(self.session(None));
                }
                Ok(Some(Event::Mouse(m))) => {
                    self.mouse(m);
                    // A click that quit ([`crate::Open::Quit`]).
                    if self.ended() {
                        self.stop_run("stopped: the screen closed");
                        return Ok(self.session(None));
                    }
                }
                Ok(Some(Event::Paste(mut text))) => {
                    self.paste(&text);
                    crate::tree::wipe(&mut text);
                }
                Ok(_) => {}
            }
            // Timed, not on an idle poll: input arriving all the time would
            // otherwise starve them.
            self.run_due(&mut schedule, Instant::now());
        }
    }

    /// End the session without the exit guard, by `signal`, which is
    /// SIGHUP when the terminal hung up: a check still running stops with
    /// `why`, the pane does what it does with unsaved work at such an end
    /// ([`crate::Pane::ending`]), and the session is handed back. The run
    /// loop's two unasked exits come here.
    pub fn end_unasked(mut self, signal: i32, why: &str) -> Session {
        self.stop_run(why);
        if let Some(p) = self.pane.as_mut() {
            p.ending();
        }
        self.session(Some(signal))
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
        assert_eq!(s.due(pane, true, false, t0 + ms(100)), Due { pane: true, shell: false, watch: false });
        assert_eq!(s.due(pane, true, false, t0 + ms(150)), Due { pane: false, shell: true, watch: false });
        assert_eq!(s.due(pane, true, false, t0 + ms(200)), Due { pane: true, shell: false, watch: false });
    }

    /// The watch runs while a pane ticks and nothing is applying.
    #[test]
    fn the_watch_runs_beside_a_ticking_pane() {
        let t0 = Instant::now();
        let mut s = Schedule::new(t0);
        let (mut ticked, mut watched) = (0, 0);
        for i in 1..=12 {
            let d = s.due(Some(ms(100)), false, false, t0 + ms(100 * i));
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
            assert!(!s.due(None, true, false, t0 + ms(50 * i)).pane);
        }
    }

    /// The loop waits for input only until the soonest job is due, and
    /// never past POLL, so a signal is seen in time.
    #[test]
    fn the_wait_is_until_the_soonest_job() {
        let t0 = Instant::now();
        let s = Schedule::new(t0);
        assert_eq!(s.wait(Some(ms(40)), false, false, t0 + ms(10)), ms(30));
        assert_eq!(s.wait(None, true, false, t0 + ms(100)), ms(50));
        assert_eq!(s.wait(None, false, false, t0), POLL);
    }

    /// A question on the clock ticks the shell every QUESTION_TICK, with
    /// no pane tick and nothing applying, and the watch goes on beside it;
    /// with none, the shell is never due.
    #[test]
    fn a_question_on_the_clock_ticks_the_shell() {
        let t0 = Instant::now();
        let mut s = Schedule::new(t0);
        assert_eq!(s.due(None, false, true, t0 + ms(100)), Due { pane: false, shell: false, watch: false });
        assert_eq!(s.due(None, false, true, t0 + QUESTION_TICK), Due { pane: false, shell: true, watch: false });
        assert_eq!(s.wait(None, false, true, t0 + QUESTION_TICK + ms(200)), ms(50));
        assert_eq!(s.due(None, false, true, t0 + ms(1000)), Due { pane: false, shell: true, watch: true });
        let mut s = Schedule::new(t0);
        assert!((1..=20).all(|i| !s.due(None, false, false, t0 + ms(100 * i)).shell), "no question, no shell tick");
    }
}

#[cfg(test)]
mod signal_tests {
    use super::*;

    /// A terminal closing sends SIGHUP more than once; none of them, nor a
    /// SIGHUP after a SIGTERM, ends the process before the pane's ending.
    #[test]
    fn a_hangup_never_forces_an_exit() {
        for earlier in [false, true] {
            for seen in [false, true] {
                assert!(!forces(false, earlier, seen), "SIGHUP, earlier {earlier}, seen {seen}");
            }
        }
    }

    /// A second SIGTERM or SIGINT forces only while the loop has not taken
    /// the first up; once it has, the pane's ending runs, bounded by SEEN.
    #[test]
    fn a_second_deliberate_signal_forces_only_before_the_loop_acts() {
        assert!(!forces(true, false, false), "the first is caught, not forced");
        assert!(forces(true, true, false), "a second before the loop acts");
        assert!(!forces(true, true, true), "a second while the ending runs");
    }

    /// The child of [`a_forced_exit_bounds_a_hook_that_blocks`]: what its
    /// hook does, `fifo:<path>` to open a FIFO no one reads, which blocks,
    /// or `quick` to return at once.
    const CHILD: &str = "AGENT_TUI_FORCED_EXIT_CHILD";

    /// Says on stdout whether the terminal was put back before it, then
    /// does what [`CHILD`] says.
    fn child_hook() {
        let said: &[u8] = if RESET_WRITTEN.load(Ordering::SeqCst) { b"hook after reset\n" } else { b"hook before reset\n" };
        // SAFETY: a write of bytes we own to fd 1.
        unsafe { libc::write(1, said.as_ptr().cast(), said.len()) };
        if let Some(fifo) = std::env::var(CHILD).ok().and_then(|m| m.strip_prefix("fifo:").map(String::from)) {
            let _ = std::fs::OpenOptions::new().write(true).open(fifo);
        }
    }

    /// Run by [`a_forced_exit_bounds_a_hook_that_blocks`] in a process of
    /// its own: two SIGTERMs force an exit, whose hook is [`child_hook`].
    #[test]
    #[ignore = "run by a_forced_exit_bounds_a_hook_that_blocks"]
    fn child_forced_exit() {
        if std::env::var(CHILD).is_err() {
            return;
        }
        on_forced_exit(child_hook);
        let _signals = Signals::install().unwrap();
        // SAFETY: raise sends a signal to this thread.
        unsafe {
            libc::raise(libc::SIGTERM);
            libc::raise(libc::SIGTERM);
        }
        loop {
            std::thread::sleep(Duration::from_secs(1));
        }
    }

    /// Run the child with `mode`: its exit code, how long it ran, and its
    /// stdout. A child still running after 15 s is killed, and its code is
    /// `None`.
    fn run_child(mode: &str) -> (Option<i32>, Duration, String) {
        use std::io::Read;
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "app::term::signal_tests::child_forced_exit", "--ignored", "--nocapture", "--test-threads=1"])
            .env(CHILD, mode)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let start = Instant::now();
        let status = loop {
            if let Some(st) = child.try_wait().unwrap() {
                break st;
            }
            if start.elapsed() > Duration::from_secs(15) {
                let _ = child.kill();
                break child.wait().unwrap();
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let took = start.elapsed();
        let mut out = String::new();
        child.stdout.take().unwrap().read_to_string(&mut out).unwrap();
        (status.code(), took, out)
    }

    /// A forced exit whose hook blocks, here opening a FIFO no one reads as
    /// a write to a hung network mount would, ends once the hook has had
    /// HOOK_BOUND; the terminal is put back before the hook. A hook that
    /// returns at once is not waited on past it.
    #[cfg(unix)]
    #[test]
    fn a_forced_exit_bounds_a_hook_that_blocks() {
        let fifo = std::env::temp_dir().join(format!("agent-tui-forced-exit-{}", std::process::id()));
        let c = std::ffi::CString::new(fifo.to_string_lossy().as_bytes()).unwrap();
        // SAFETY: mkfifo on a path we own.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0, "mkfifo {}", fifo.display());
        let blocked = run_child(&format!("fifo:{}", fifo.display()));
        let quick = run_child("quick");
        let _ = std::fs::remove_file(&fifo);
        for (what, (code, _, out)) in [("blocked", &blocked), ("quick", &quick)] {
            assert_eq!(*code, Some(128 + libc::SIGTERM), "{what}: exit code; None is a forced exit that never ended");
            let reset = out.find("\x1b[?1049l").unwrap_or_else(|| panic!("{what}: no reset in {out:?}"));
            let hook = out.find("hook after reset").unwrap_or_else(|| panic!("{what}: the hook ran before the reset: {out:?}"));
            assert!(reset < hook, "{what}: the terminal is put back before the hook: {out:?}");
        }
        let (_, took, _) = blocked;
        assert!(took >= HOOK_BOUND && took < HOOK_BOUND + Duration::from_secs(2), "blocked: ended after {took:?}, the bound is {HOOK_BOUND:?}");
        let (_, took, _) = quick;
        assert!(took < Duration::from_millis(1500), "quick: ended after {took:?}, not waiting out the bound");
    }
}
