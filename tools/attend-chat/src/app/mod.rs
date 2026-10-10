//! The chat screen on `agent-tui`'s shell (ADR-504 §1, §3): the channels
//! on the shell's tab bar, then the message feed, the compose box and the
//! helper row (legend, slash commands, hints), then the shell's bottom bar
//! with the status and the footer. Colour comes from the one theme
//! (`crate::theme`); identity colours stay the categorical palette.
//!
//! [`ChatPane`] holds the chat's state and is a [`Pane`]: the shell draws
//! the tabs it lists and the footer from the keys it declares, takes the
//! mouse, the exit guard and the key help, and hands it the keys it leaves.
//! [`Chat`] is the shell over that pane, as the terminal runs it
//! ([`Chat::into_app`]) and as the headless `--snap` and the tests drive
//! it. `tick` drains the watcher and keeps the human's heartbeat; `draw`
//! (in [`view`]) paints the pane. The Enter and Tab logic lives in
//! [`keys`] as free functions; the editing keys are the shared input's.

mod keys;
mod menu;
mod view;

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, SystemTime};

use agent_tui::feed::{Entry, FeedState};
use agent_tui::input::Input;
use agent_tui::ratatui::buffer::Cell;
use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};
use agent_tui::ratatui::layout::{Position, Rect};
use agent_tui::ratatui::text::Span;
use agent_tui::ratatui::Frame;
use agent_tui::screen::Screen;
use agent_tui::theme::{Palette, Shape};
use agent_tui::{App, Binding, Keyed, Open, Pane, PaneTab, Tone};
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

/// How many rows a notch of the mouse wheel scrolls the feed.
const WHEEL_ROWS: usize = 3;

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

/// A chip of the helper row, and what a click on it completes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Chip {
    Agent(String),
    Group(String),
    Slash(String),
    Sub(String),
}

/// Where the last frame drew what a click can hit: the feed, the compose
/// box's text and the first of its rows shown, the width its text wraps
/// at, and each chip of the helper row.
#[derive(Debug, Clone, Default)]
struct Hits {
    feed: Rect,
    compose: (Rect, usize),
    text_width: u16,
    chips: Vec<(Rect, Chip)>,
}

/// The chat's state: the messages, the compose box, the shown channel.
pub struct ChatPane {
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
    /// The pane as last drawn, its area and cells, and whether anything
    /// it shows has changed since: a key, new messages, a refresh. An
    /// unchanged pane is copied, not drawn again; idle costs nothing per
    /// frame.
    last_frame: Option<(Rect, Vec<Cell>)>,
    dirty: bool,
    /// A dry run (`--snap`): Enter sends nothing and runs no command.
    dry_run: bool,
    /// Frames drawn afresh, for tests of the skip.
    draws: usize,
    /// The time the last tick saw, to notice the local day changing: the
    /// feed's times are relative to the day (`HH:MM` today, a date before).
    day_seen: Option<SystemTime>,
    hits: Hits,
    /// What a tab's menu asked the shell to open over the pane.
    open: Option<Open>,
    /// A menu step, or a quit, waiting for `y`.
    pending: Option<menu::Pending>,
    /// The compose box lent to a channel's name or description, and the
    /// draft it held before.
    prompt: Option<(menu::Prompt, String)>,
    /// The terminal reports Ctrl+digits as themselves. Without it Ctrl+3
    /// arrives as Esc, so Esc on an empty line asks before quitting.
    enhanced: bool,
    /// The chat's settings (`attend.chat.*`), read at start and again
    /// after a command, so `/config` applies at once.
    config: attend_config::ChatConfig,
}

impl ChatPane {
    /// A chat fed by `rx`, drawn with `palette`.
    pub fn new(rx: Option<Receiver<Signal>>, palette: Palette) -> ChatPane {
        ChatPane {
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
            clock: Clock::Live,
            world: None,
            entries: None,
            heartbeat: true,
            refreshed: None,
            generation: 0,
            last_frame: None,
            dirty: true,
            dry_run: false,
            draws: 0,
            day_seen: None,
            hits: Hits::default(),
            open: None,
            pending: None,
            prompt: None,
            enhanced: false,
            config: crate::settings::load(&std::env::current_dir().unwrap_or_default()),
        }
    }

    /// A status message, as a command's result shows it.
    pub fn say(&mut self, s: impl Into<String>, error: bool) {
        self.status = s.into();
        self.status_set_at = Some(Instant::now());
        self.status_is_error = error;
        self.dirty = true;
    }

    /// The feed's entries are out of date: rebuild them, and their layout,
    /// on the next frame.
    fn invalidate(&mut self) {
        self.entries = None;
        self.generation += 1;
        self.dirty = true;
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
            // A question asked on one tab is not answered on another.
            self.drop_question();
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
        let sets_jump = self.input.text().trim_start().starts_with("/config tabs.jump");
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
        self.reload_settings();
        if sets_jump && !self.status_is_error {
            let said = format!("{}{}", self.status, self.jump_warning());
            self.say(said, false);
        }
        self.stale();
    }

    /// Read the chat's settings again: a `/config` set applies at once.
    fn reload_settings(&mut self) {
        self.config = crate::settings::load(&std::env::current_dir().unwrap_or_default());
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
        self.scroll_by(up, step);
    }

    fn scroll_by(&mut self, up: bool, rows: usize) {
        self.feed.scroll = if up { self.feed.scroll + rows } else { self.feed.scroll.saturating_sub(rows) };
        self.dirty = true;
    }

    /// A click on a chip of the helper row: complete what is being typed
    /// to it, as Tab would, the cursor at the end.
    fn complete(&mut self, chip: Chip) {
        let text = self.input.text().to_string();
        let sigil = if matches!(chip, Chip::Group(_)) { '#' } else { '@' };
        let (next, cursor) = match chip {
            Chip::Slash(name) => crate::slash::apply_slash_completion(&name),
            Chip::Agent(name) | Chip::Group(name) => {
                match crate::legend::find_trailing_mention(&text).filter(|m| m.prefix.ends_with(sigil)) {
                    Some(m) => crate::legend::apply_completion(&text, &m, &name),
                    None => {
                        let sep = if text.is_empty() || text.ends_with(char::is_whitespace) { "" } else { " " };
                        let out = format!("{text}{sep}{sigil}{name} ");
                        let n = out.chars().count();
                        (out, n)
                    }
                }
            }
            Chip::Sub(name) => {
                let head = if text.ends_with(char::is_whitespace) { text.as_str() } else { text.trim_end_matches(|c: char| !c.is_whitespace()) };
                let out = format!("{head}{name} ");
                let n = out.chars().count();
                (out, n)
            }
        };
        self.input.set(next, cursor);
        self.tab_cycle = None;
        self.dirty = true;
    }

    /// The time frames show times against.
    fn now(&self) -> SystemTime {
        match self.clock {
            Clock::Live => SystemTime::now(),
            Clock::Pinned { now, .. } => now,
        }
    }

    /// At the local day's change, rebuild the feed: a time shown as
    /// `HH:MM` yesterday shows its date now, and attachment chips are
    /// checked against the files again.
    fn new_day(&mut self) {
        let now = self.now();
        if let Some(prev) = self.day_seen {
            let label = match self.clock {
                Clock::Live => agent_fmt::compact_time(prev, now),
                Clock::Pinned { offset, .. } => agent_fmt::compact_time_with_offset(prev, now, offset),
            };
            // `HH:MM` only on the same local day; a date has a `-`.
            if label.contains('-') {
                self.invalidate();
            }
        }
        self.day_seen = Some(now);
    }

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

impl Pane for ChatPane {
    fn palette(&self) -> Palette {
        self.palette
    }

    fn tabs(&mut self) -> Vec<PaneTab> {
        view::tabs(self)
    }

    /// The bar's index of the tab shown: 0 is the `≡` slot, 1 merged, then
    /// the channels, then the `+` slot.
    fn tab(&mut self) -> usize {
        match self.normal_tab() {
            Tab::Merged => 1,
            Tab::Channel(g) => self.strip_names().iter().position(|n| *n == g).map_or(1, |i| i + 2),
        }
    }

    /// Show the bar's tab `i`: merged, or a channel (tab number `i`, as
    /// Alt+N and Ctrl+N count them). The slots and an index past the tabs
    /// do nothing.
    fn set_tab(&mut self, i: usize) {
        let names = self.strip_names();
        if let Some(t) = i.checked_sub(1).and_then(|n| tabs::jump(n as u32 + 1, &names)) {
            ChatPane::set_tab(self, t);
        }
    }

    fn trailer(&mut self) -> Vec<Span<'static>> {
        view::trailer(self)
    }

    fn draw(&mut self, f: &mut Frame, area: Rect) {
        if !self.dirty {
            if let Some((at, cells)) = &self.last_frame {
                if *at == area {
                    let buf = f.buffer_mut();
                    let mut it = cells.iter();
                    for y in area.top()..area.bottom() {
                        for x in area.left()..area.right() {
                            if let (Some(c), Some(to)) = (it.next(), buf.cell_mut((x, y))) {
                                *to = c.clone();
                            }
                        }
                    }
                    return;
                }
            }
        }
        view::draw(self, f, area);
        self.draws += 1;
        self.dirty = false;
        let buf = f.buffer_mut();
        let cells = (area.top()..area.bottom())
            .flat_map(|y| (area.left()..area.right()).map(move |x| (x, y)))
            .filter_map(|p| buf.cell(p).cloned())
            .collect();
        self.last_frame = Some((area, cells));
    }

    /// The chat's keys. Esc is the shell's: it quits, asking first over a
    /// draft. Alt+1..9 (a tab), Alt+m (the mouse) and F1 (the keys) are
    /// the shell's too, and never reach here.
    fn key(&mut self, k: KeyEvent) -> Keyed {
        self.dirty = true;
        let m = k.modifiers;
        let plain = !m.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        if self.pending.is_some() {
            let yes = plain && matches!(k.code, KeyCode::Char('y' | 'Y'));
            if self.answer(yes) {
                return Keyed::Quit;
            }
            // Any other character keeps things as they are and is typing:
            // it goes on into the draft. Other keys only answer.
            if yes || !(plain && matches!(k.code, KeyCode::Char(_))) {
                return Keyed::Done;
            }
        }
        if self.prompt.is_some() {
            match k.code {
                KeyCode::Esc => self.prompt_cancel(),
                KeyCode::Enter if !m.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) => self.prompt_enter(),
                KeyCode::Tab | KeyCode::PageUp | KeyCode::PageDown => {}
                _ => {
                    self.input.key(k);
                }
            }
            return Keyed::Done;
        }
        match k.code {
            // Without the keyboard enhancement Ctrl+3 is this same Esc:
            // ask, so a tab key never closes the chat.
            KeyCode::Esc if !self.enhanced && self.input.is_empty() => self.pending = Some(menu::Pending::Quit),
            KeyCode::Esc => return Keyed::Pass,
            KeyCode::Enter if !m.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) => self.enter(),
            KeyCode::Tab => self.tab_key(),
            KeyCode::PageUp => self.page(true),
            KeyCode::PageDown => self.page(false),
            _ => {
                self.input.key(k);
            }
        }
        Keyed::Done
    }

    fn wheel(&mut self, up: bool) {
        self.scroll_by(up, WHEEL_ROWS);
    }

    /// A click on a message selects it and brings it whole into view; on
    /// the compose box's text it puts the cursor there; on a chip of the
    /// helper row it completes what is being typed to it.
    fn click(&mut self, at: Position) {
        self.dirty = true;
        if self.hits.feed.contains(at) {
            if let Some(i) = self.feed.entry_at(at.y - self.hits.feed.y) {
                self.feed.reveal(i);
            }
            return;
        }
        let (text, start) = self.hits.compose;
        let with_prompt = Rect { x: text.x.saturating_sub(2), width: text.width + 2, ..text };
        if with_prompt.contains(at) {
            let col = at.x.saturating_sub(text.x) as usize;
            self.input.click(self.hits.text_width, start + (at.y - text.y) as usize, col);
            self.tab_cycle = None;
            return;
        }
        if let Some((_, chip)) = self.hits.chips.iter().find(|(r, _)| r.contains(at)).cloned() {
            self.complete(chip);
        }
    }

    fn bindings(&self) -> Vec<Binding> {
        vec![
            Binding::new("Enter", "send"),
            Binding::new("Tab", "complete"),
            Binding::new("PgUp PgDn", "scroll"),
            Binding::help("S-Enter M-Enter", "new line"),
            Binding::help("← → Home End", "move the cursor"),
            Binding::help("Bksp Del", "edit"),
            Binding::help("click a message", "selects it; in the compose box it places the cursor; a chip completes it"),
        ]
    }

    fn mode(&self) -> String {
        "chat".into()
    }

    /// The status slot (#398, #400): a fresh result asserts itself, an
    /// error in the error role; past [`STATUS_ASSERT`] the help of a
    /// command being typed, or the last result, set back.
    fn status(&mut self) -> Option<(String, Tone)> {
        if let Some(s) = self.menu_status() {
            return Some((s, Tone::Said));
        }
        let fresh = self.status_set_at.is_some_and(|t| t.elapsed() < STATUS_ASSERT);
        let (line, how) = status_slot(fresh, self.status_is_error, crate::slash::contextual_help(self.input.text()), &self.status);
        let tone = match how {
            Some(true) => Tone::Err,
            Some(false) => Tone::Said,
            None => Tone::Back,
        };
        (!line.is_empty()).then_some((line, tone))
    }

    fn owns_text(&self) -> bool {
        true
    }

    /// Off at the start: what people copy from a chat (a message, a path,
    /// a command's output) is the terminal's selection, and middle-click
    /// pastes into the compose box. Alt+m gives the shell the mouse.
    fn mouse_default(&self) -> bool {
        self.config.mouse
    }

    fn unsaved(&self) -> Option<String> {
        let held = self.prompt.as_ref().is_some_and(|(_, draft)| !draft.is_empty());
        (!self.input.is_empty() || held).then(|| "a draft in the compose box".to_string())
    }

    fn has_tab_menus(&self) -> bool {
        true
    }

    fn tab_menu(&mut self, i: usize) {
        self.open_tab_menu(i);
    }

    fn interrupted(&mut self) {
        self.drop_question();
    }

    fn quit_help(&self) -> Option<String> {
        Some(if self.enhanced {
            "quit; asks first over a draft".into()
        } else {
            "quit; asks over a draft; else Esc asks and y quits".into()
        })
    }

    fn take_open(&mut self) -> Option<Open> {
        self.open.take()
    }

    fn picked(&mut self, id: &str, values: Vec<String>) {
        if let Some(v) = values.first() {
            self.menu_picked(id, v);
        }
    }

    fn keyboard_enhancement(&self) -> bool {
        true
    }

    fn tab_keys(&self) -> agent_tui::TabKeys {
        crate::settings::tab_keys(&self.config)
    }

    fn set_keyboard_enhanced(&mut self, on: bool) {
        self.enhanced = on;
    }

    fn discard(&mut self) {
        self.input.clear();
    }

    fn help(&self) -> Option<String> {
        Some(
            "Enter sends to the shown channel; merged sends to #open. @name or #channel\n\
             at the start addresses the message; Tab completes them, and on an empty\n\
             line shows the next tab. /help lists the slash commands. `attend inbox`\n\
             and `attend send` are the same bus outside the screen."
                .into(),
        )
    }

    /// The watcher is drained, and the world refreshed when due, ten times
    /// a second.
    fn tick_every(&self) -> Option<Duration> {
        Some(TICK)
    }

    fn tick(&mut self) {
        self.drain();
        self.new_day();
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

/// The chat on the shell: [`ChatPane`] inside `agent_tui::App`. The
/// terminal runs [`Chat::into_app`] through `agent_tui::run`; the headless
/// `--snap` and the tests drive the same shell as a [`Screen`].
pub struct Chat {
    app: App,
}

impl Chat {
    /// A chat fed by `rx`, drawn with `palette` and lozenges in `shape`.
    pub fn new(rx: Option<Receiver<Signal>>, palette: Palette, shape: Shape) -> Chat {
        Chat { app: App::with_pane("chat", ChatPane::new(rx, palette)).shape(shape) }
    }

    fn pane(&self) -> &ChatPane {
        self.app.pane_ref().expect("the chat's pane")
    }

    fn pane_mut(&mut self) -> &mut ChatPane {
        self.app.pane_mut().expect("the chat's pane")
    }

    /// Show timestamps against a fixed `now` at a UTC `offset` in seconds.
    pub fn pinned(mut self, now: SystemTime, offset: i64) -> Chat {
        self.pane_mut().clock = Clock::Pinned { now, offset };
        self
    }

    /// Keep the human's presence heartbeat, or not.
    pub fn heartbeat(mut self, on: bool) -> Chat {
        self.pane_mut().heartbeat = on;
        self
    }

    /// A dry run: Enter sends nothing to the bus and runs no slash
    /// command; the status line says what it would have done. For a
    /// headless frame (`--snap`), which must never post or change state.
    pub fn dry_run(mut self, on: bool) -> Chat {
        self.pane_mut().dry_run = on;
        self
    }

    /// As if the terminal did, or did not, take the keyboard enhancement
    /// (`--snap` and the tests: no terminal answers there).
    pub fn enhanced(mut self, on: bool) -> Chat {
        self.app.set_keyboard_enhanced(on);
        self
    }

    /// The shell, to run on the terminal.
    pub fn into_app(self) -> App {
        self.app
    }

    /// The shell, as it stands.
    pub fn app(&self) -> &App {
        &self.app
    }

    /// A status message, as a command's result shows it.
    pub fn say(&mut self, s: impl Into<String>, error: bool) {
        self.pane_mut().say(s, error);
    }

    /// How far the feed is paged back, in rows from the bottom.
    pub fn scroll(&self) -> usize {
        self.pane().feed.scroll
    }

    /// How many frames of the pane were drawn afresh rather than copied.
    pub fn draws(&self) -> usize {
        self.pane().draws
    }

    pub fn input(&self) -> &Input {
        &self.pane().input
    }

    pub fn foreground(&self) -> &Tab {
        &self.pane().foreground
    }

    pub fn status(&self) -> &str {
        &self.pane().status
    }

    pub fn signals(&self) -> &[Signal] {
        &self.pane().signals
    }

    pub fn push(&mut self, sig: Signal) {
        self.pane_mut().push(sig);
    }

    pub fn drain(&mut self) -> bool {
        self.pane_mut().drain()
    }

    pub fn refresh(&mut self) {
        self.pane_mut().refresh();
    }

    /// A mouse event, through the shell's mouse handler.
    pub fn mouse(&mut self, m: MouseEvent) {
        self.app.mouse(m);
    }
}

impl Screen for Chat {
    fn palette(&self) -> Palette {
        self.pane().palette
    }

    fn draw(&mut self, f: &mut Frame) {
        self.app.draw(f);
    }

    fn key(&mut self, k: KeyEvent) -> bool {
        self.app.key(k)
    }

    fn mouse(&mut self, m: MouseEvent) {
        self.app.mouse(m);
    }

    fn tick_every(&self) -> Option<Duration> {
        self.app.pane_tick_every()
    }

    fn tick(&mut self) {
        self.app.tick_pane();
        self.app.tick();
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

#[cfg(test)]
mod binding_tests {
    use super::*;

    /// The footer and the key help read one declaration; a key in it twice
    /// would name two meanings.
    #[test]
    fn the_chats_keys_are_bound_once_each() {
        let c = Chat::new(None, Palette::default(), Shape::PLAIN);
        assert_eq!(agent_tui::binding_conflicts(&c.app().bindings()), Vec::<String>::new());
    }
}
