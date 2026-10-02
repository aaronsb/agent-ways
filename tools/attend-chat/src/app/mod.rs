//! The chat screen on `agent-tui` (ADR-504 §1): a tab strip of channels
//! on top, the message feed, the compose box, the helper row (legend,
//! slash commands, hints) and the status line. Colour comes from the one
//! theme (`crate::theme`); identity colours stay the categorical palette.
//!
//! [`Chat`] holds the state and implements [`Screen`]: `key` is the one
//! key handler, used by the terminal loop, the headless `--snap` and the
//! tests; `tick` drains the watcher and keeps the human's heartbeat; and
//! `draw` (in [`view`]) paints a frame. The Enter and Tab logic lives in
//! [`keys`] as free functions; the editing keys are the shared input's.

mod keys;
mod view;

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, SystemTime};

use agent_tui::feed::{Entry, FeedState};
use agent_tui::input::Input;
use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use agent_tui::ratatui::Frame;
use agent_tui::screen::Screen;
use agent_tui::theme::{Palette, Shape};
use attend_instances::SnapshotCache;

use crate::chip::{known_identities, KnownIdentity};
use crate::groups::{channels, KnownGroup};
use crate::sessions::discover as discover_sessions;
use crate::signal::Signal;
use crate::tabs::{self, Tab};

pub use keys::{destination_label, handle_enter, handle_tab, EnterAction, TabCycle, TabResult};

/// How long a freshly executed command's result asserts itself OVER
/// the single-match slash help in the status slot (#400) — the window
/// in which "why did nothing happen" is answered before the display
/// yields back to the help. A named top-level constant by operator
/// requirement: the tuning knob must not be buried in render code.
const STATUS_ASSERT: Duration = Duration::from_secs(6);

/// Upper bound on the in-memory message buffer. At typical chat rates
/// this is unreachable; the cap only matters for runaway conditions
/// (overnight runs, a misbehaving peer, a loop). When we hit it, drop
/// from the head so the newest history stays visible.
const MAX_SIGNALS: usize = 5000;

/// How often the peers, channels and the human's heartbeat are
/// refreshed without any input (ADR-129, ADR-170). A peer that boots
/// without sending appears in the legend within one refresh; one whose
/// attend stops shows as stale once its heartbeat ages past grace. The
/// chat is a turn-blind surface, so wall-clock time is the right axis.
const REFRESH: Duration = Duration::from_secs(5);

/// How often the watcher's channel is drained.
const TICK: Duration = Duration::from_millis(100);

/// The time a frame shows timestamps against.
#[derive(Debug, Clone, Copy)]
enum Clock {
    Live,
    /// A fixed instant and UTC offset, for frames that must not move.
    Pinned { now: SystemTime, offset: i64 },
}

/// What the screen shows besides the messages: the channels in strip
/// order and the identities the legend and completion offer. Read from
/// disk at most once per [`REFRESH`] or input, never per frame.
struct World {
    groups: Vec<KnownGroup>,
    known: Vec<KnownIdentity>,
    /// The instance registry, for each chip's `-<instance>` suffix
    /// (ADR-129). Built afresh with the world, never kept across it.
    instances: SnapshotCache,
}

pub struct Chat {
    signals: Vec<Signal>,
    input: Input,
    status: String,
    /// When the status last changed, and whether it was a failure
    /// (#400): drives the timed assert-over-help window and the error
    /// styling — provenance from the action, never sniffed from text.
    status_set_at: Option<Instant>,
    status_is_error: bool,
    tab_cycle: Option<TabCycle>,
    /// The foreground tab (#393). Merged at launch: the full stream is
    /// the opening posture; per-channel focus is an explicit gesture.
    foreground: Tab,
    feed: FeedState,
    /// The feed's height in the last frame, for paging.
    feed_rows: u16,
    rx: Option<Receiver<Signal>>,
    palette: Palette,
    shape: Shape,
    clock: Clock,
    world: Option<World>,
    /// The feed's entries for the foreground tab, rebuilt when the world,
    /// the messages, the tab or the palette change.
    entries: Option<Vec<Entry>>,
    /// Whether to keep `heartbeat/<username>` fresh (ADR-170). Off for a
    /// headless frame, which is no presence.
    heartbeat: bool,
    refreshed: Option<Instant>,
    /// Bumped whenever the feed's entries are rebuilt, so the feed keeps
    /// its layout between frames until they change.
    generation: u64,
    /// The last frame drawn and whether anything has changed since: a key,
    /// new messages, a refresh, a status message. An unchanged screen is
    /// copied, not drawn again; idle costs nothing per frame.
    last_frame: Option<agent_tui::ratatui::buffer::Buffer>,
    dirty: bool,
    /// Whether the status line was in its assert window when last drawn;
    /// the frame after the window closes is drawn afresh.
    drawn_fresh: bool,
    /// A dry run (`--snap`): Enter sends nothing and runs no command.
    dry_run: bool,
    /// Frames drawn afresh, for tests of the skip.
    draws: usize,
}

impl Chat {
    /// A chat fed by `rx`, drawn with `palette` and lozenges in `shape`.
    pub fn new(rx: Option<Receiver<Signal>>, palette: Palette, shape: Shape) -> Chat {
        Chat {
            signals: Vec::new(),
            input: Input::new(),
            status: String::new(),
            status_set_at: None,
            status_is_error: false,
            tab_cycle: None,
            foreground: Tab::default(),
            feed: FeedState::default(),
            feed_rows: 0,
            rx,
            palette,
            shape,
            clock: Clock::Live,
            world: None,
            entries: None,
            heartbeat: true,
            refreshed: None,
            generation: 0,
            last_frame: None,
            dirty: true,
            drawn_fresh: false,
            dry_run: false,
            draws: 0,
        }
    }

    /// Show timestamps against a fixed `now` at a UTC `offset` in seconds.
    pub fn pinned(mut self, now: SystemTime, offset: i64) -> Chat {
        self.clock = Clock::Pinned { now, offset };
        self
    }

    /// Keep the human's presence heartbeat, or not.
    pub fn heartbeat(mut self, on: bool) -> Chat {
        self.heartbeat = on;
        self
    }

    /// A dry run: Enter sends nothing to the bus and runs no slash
    /// command; the status line says what it would have done. For a
    /// headless frame (`--snap`), which must never post or change state.
    pub fn dry_run(mut self, on: bool) -> Chat {
        self.dry_run = on;
        self
    }

    /// A status message, as a command's result shows it.
    pub fn say(&mut self, s: impl Into<String>, error: bool) {
        self.status = s.into();
        self.status_set_at = Some(Instant::now());
        self.status_is_error = error;
        self.dirty = true;
    }

    /// How far the feed is paged back, in rows from the bottom.
    pub fn scroll(&self) -> usize {
        self.feed.scroll
    }

    /// How many frames were drawn afresh rather than copied.
    pub fn draws(&self) -> usize {
        self.draws
    }

    /// The feed's entries are out of date: rebuild them, and their layout,
    /// on the next frame.
    fn invalidate(&mut self) {
        self.entries = None;
        self.generation += 1;
        self.dirty = true;
    }

    pub fn input(&self) -> &Input {
        &self.input
    }

    pub fn foreground(&self) -> &Tab {
        &self.foreground
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn signals(&self) -> &[Signal] {
        &self.signals
    }

    /// Append `sig`, dropping from the head past [`MAX_SIGNALS`]. Both
    /// writers into the buffer — the watcher drain and the directed-send
    /// echo — go through here so the cap has a single home.
    pub fn push(&mut self, sig: Signal) {
        self.signals.push(sig);
        if self.signals.len() > MAX_SIGNALS {
            let drop_n = self.signals.len() - MAX_SIGNALS;
            self.signals.drain(0..drop_n);
        }
        self.invalidate();
    }

    /// Take every signal the watcher has sent. True when any arrived.
    pub fn drain(&mut self) -> bool {
        let Some(rx) = &self.rx else { return false };
        let got: Vec<Signal> = rx.try_iter().collect();
        let any = !got.is_empty();
        for s in got {
            self.push(s);
        }
        if any {
            // A new sender joins the legend and the completion pool.
            self.stale();
        }
        any
    }

    /// Read the peers and channels again on the next frame.
    fn stale(&mut self) {
        self.world = None;
        self.invalidate();
    }

    fn world(&mut self) -> &World {
        if self.world.is_none() {
            let depth = self.palette.depth();
            let instances = SnapshotCache::new();
            let known = known_identities(&self.signals, &discover_sessions(), depth, &instances);
            self.world = Some(World { groups: channels(depth), known, instances });
        }
        self.world.as_ref().expect("just built")
    }

    /// The channel names in strip order.
    fn strip_names(&mut self) -> Vec<String> {
        tabs::strip_names(&self.world().groups)
    }

    /// The foreground tab against the live strip: a channel dissolved by a
    /// peer falls to `#open` instead of pointing at a tab no longer drawn.
    fn normal_tab(&mut self) -> Tab {
        let names = self.strip_names();
        tabs::normalize(self.foreground.clone(), &names)
    }

    fn set_tab(&mut self, t: Tab) {
        if t != self.foreground {
            self.foreground = t;
            self.feed = FeedState::default();
            self.invalidate();
        }
    }

    fn enter(&mut self) {
        if self.dry_run {
            return self.enter_dry();
        }
        // Normalize against the live strip (PR #395 finding 6): after a
        // peer dissolves the foregrounded channel, Enter agrees with the
        // destination flag, which has degraded to #open.
        let fg = self.normal_tab();
        match handle_enter(self.input.text(), &self.signals, &fg) {
            EnterAction::None => {}
            EnterAction::ClearWithStatus(s) => {
                self.say(s, false);
                self.input.clear();
                self.show_newest();
            }
            EnterAction::ClearWithStatusAndEcho { status, echo } => {
                self.say(status, false);
                self.input.clear();
                self.show_newest();
                // A directed send lands in the recipient's inbox, which
                // this chat does not watch: echo it into the transcript.
                self.push(echo);
            }
            EnterAction::StatusOnly(s) => self.say(s, true),
            EnterAction::ClearWithStatusAndFocus { status, focus } => {
                self.say(status, false);
                self.input.clear();
                // `/dissolve` closed the foreground tab: move focus where
                // the handler advanced it.
                self.set_tab(focus);
            }
            EnterAction::ClearTranscript => {
                // Display-only: nothing on the bus is touched, and fresh
                // traffic streams back in.
                self.signals.clear();
                self.say("transcript cleared", false);
                self.input.clear();
                self.show_newest();
            }
        }
        self.stale();
    }

    /// Enter in a dry run: say what would happen, change nothing.
    fn enter_dry(&mut self) {
        let text = self.input.text().trim_end().to_string();
        if text.is_empty() {
            return;
        }
        let what = match crate::slash::parse(&text) {
            Some((cmd, _)) => format!("dry run: /{cmd} not run"),
            None => {
                let scope = tabs::send_scope(&self.normal_tab());
                let to = destination_label(&text, &scope).unwrap_or(format!("#{scope}"));
                format!("dry run: not sent to {to}")
            }
        };
        self.say(what, false);
        self.input.clear();
    }

    /// Back to the newest messages: after a success, so the sent message
    /// is in view. A failure leaves the view where it was.
    fn show_newest(&mut self) {
        self.feed.scroll = 0;
    }

    fn tab_key(&mut self) {
        if self.input.is_empty() {
            // Empty compose line: Tab cycles the foreground tab (#393).
            // With content it stays completion.
            let names = self.strip_names();
            let cur = tabs::normalize(self.foreground.clone(), &names);
            self.set_tab(tabs::cycle_next(&cur, &names));
            return;
        }
        let r = handle_tab(self.input.text(), self.input.cursor(), self.tab_cycle.take(), &self.signals);
        self.input.set(r.new_buf, r.new_cursor);
        self.tab_cycle = r.new_cycle;
    }

    fn page(&mut self, up: bool) {
        let step = (self.feed_rows as usize).saturating_sub(1).max(1);
        self.feed.scroll = if up { self.feed.scroll + step } else { self.feed.scroll.saturating_sub(step) };
    }
}

impl Screen for Chat {
    fn palette(&self) -> Palette {
        self.palette
    }

    /// The watcher is drained, and the world refreshed when due, ten times
    /// a second.
    fn tick_every(&self) -> Option<Duration> {
        Some(TICK)
    }

    fn draw(&mut self, f: &mut Frame) {
        let fresh = self.status_set_at.is_some_and(|t| t.elapsed() < STATUS_ASSERT);
        if !self.dirty && fresh == self.drawn_fresh {
            if let Some(last) = &self.last_frame {
                if last.area == f.area() {
                    f.buffer_mut().content.clone_from_slice(&last.content);
                    return;
                }
            }
        }
        view::draw(self, f);
        self.draws += 1;
        self.dirty = false;
        self.drawn_fresh = fresh;
        self.last_frame = Some(f.buffer_mut().clone());
    }

    /// The one key handler. Esc and Ctrl-C end the chat.
    fn key(&mut self, k: KeyEvent) -> bool {
        self.dirty = true;
        let m = k.modifiers;
        match k.code {
            KeyCode::Esc => return false,
            KeyCode::Char('c') if m.contains(KeyModifiers::CONTROL) => return false,
            KeyCode::Enter if !m.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) => self.enter(),
            KeyCode::Tab => self.tab_key(),
            KeyCode::PageUp => self.page(true),
            KeyCode::PageDown => self.page(false),
            // Alt+1..9 jumps straight to a tab (IRC prior): 1 = merged,
            // 2 = #open, 3.. = named channels in strip order. Empty slots
            // are no-ops.
            KeyCode::Char(c) if m.contains(KeyModifiers::ALT) && c.is_ascii_digit() => {
                let names = self.strip_names();
                if let Some(t) = c.to_digit(10).and_then(|slot| tabs::jump(slot, &names)) {
                    self.set_tab(t);
                }
            }
            _ => {
                self.input.key(k);
            }
        }
        true
    }

    fn tick(&mut self) {
        self.drain();
        if self.refreshed.is_none_or(|t| t.elapsed() >= REFRESH) {
            // Human presence (ADR-170): while the chat is open, keep
            // `heartbeat/<username>` fresh so this human counts as a live
            // channel member. Best-effort, like every heartbeat write.
            if self.heartbeat {
                let _ = attend_presence::heartbeat::touch(&crate::signal::human_member_id());
            }
            self.refresh();
            self.refreshed = Some(Instant::now());
        }
    }
}

impl Chat {
    /// Read the peers and channels again, and rebuild the feed only when
    /// what it shows of them changed: an idle refresh of an unchanged
    /// world costs a read, not a re-layout of every message.
    pub fn refresh(&mut self) {
        let before = self.world.as_ref().map(World::fingerprint);
        self.world = None;
        let after = self.world().fingerprint();
        if before.as_ref() != Some(&after) {
            self.invalidate();
        }
    }
}

impl World {
    /// What the frame shows of the world: the channels with their members
    /// and descriptions, and the identities with their instance names.
    fn fingerprint(&self) -> String {
        let mut out = String::new();
        for g in &self.groups {
            out += &format!("#{}|{:?}|{:?}\n", g.group.name, g.membership.members, g.membership.description);
        }
        for k in &self.known {
            out += &format!("@{}|{}|{}\n", k.nickname, k.cwd, k.is_claude);
        }
        out
    }
}

/// The status-slot content under the #398/#400 priority rule: a fresh
/// result (error or success) > single-match help > the stored result.
/// The second value is how it reads: `Some(true)` an error, `Some(false)`
/// a success, `None` set back.
fn status_slot(fresh: bool, is_error: bool, help: Option<String>, stored: &str) -> (String, Option<bool>) {
    if fresh {
        return (stored.to_string(), Some(is_error));
    }
    (help.unwrap_or_else(|| stored.to_string()), None)
}

#[cfg(test)]
mod status_slot_tests {
    use super::*;

    #[test]
    fn fresh_error_asserts_over_help() {
        let (text, how) = status_slot(true, true, Some("/dissolve — help".into()), "1 live member — not dissolving");
        assert_eq!(text, "1 live member — not dissolving");
        assert_eq!(how, Some(true));
    }

    #[test]
    fn fresh_success_asserts() {
        let (text, how) = status_slot(true, false, Some("help".into()), "joined #x");
        assert_eq!(text, "joined #x");
        assert_eq!(how, Some(false));
    }

    #[test]
    fn expired_window_yields_to_help_then_stored() {
        let (text, how) = status_slot(false, true, Some("/dissolve — help".into()), "err");
        assert_eq!(text, "/dissolve — help");
        assert_eq!(how, None);
        let (text, _) = status_slot(false, true, None, "err");
        assert_eq!(text, "err");
    }
}
