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

mod activate;
mod cleanup;
mod dm;
mod focus;
mod ix;
mod keys;
mod menu;
mod settings_view;
mod sidebar;
mod view;

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, SystemTime};

use agent_tui::feed::{Entry, FeedState};
use agent_tui::input::Input;
use agent_tui::ratatui::buffer::Cell;
use agent_tui::ratatui::crossterm::event::{KeyEvent, MouseEvent};
use agent_tui::ratatui::layout::{Position, Rect};
use agent_tui::ratatui::Frame;
use agent_tui::screen::Screen;
use agent_tui::theme::{Palette, Shape};
use agent_tui::{App, Binding, Keyed, Open, Pane, PaneTab, Tone};
use attend_instances::SnapshotCache;

use crate::chip::{seen_identities, settle, Seen};
use crate::groups::{all_channels, invites, tab_channels, ChannelRow, Invites, KnownGroup};
use crate::sessions::discover as discover_sessions;
use crate::signal::Signal;
use crate::tabs::{self, Tab};
use crate::watcher::Delivery;

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

/// How many of the newest messages a send's copy is looked for among: the
/// copies arrive together, so a send's others are never far back.
const DEDUPE_WINDOW: usize = 256;

/// How long a file the chat put in its buffer itself, a send read back,
/// waits for the watcher's copy of it, which is dropped when it comes.
/// The copy comes within a tick or two; a copy later than this is shown.
const ECHO_KEPT: Duration = Duration::from_secs(5);

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
    /// Every identity seen, live or not, for the left column (ADR-405).
    seen: Vec<Seen>,
    /// Every channel, for the left column, and each one's live members.
    rows: Vec<ChannelRow>,
    live_members: Vec<usize>,
    /// Every channel's pending invitations, listed under it as invited.
    invites: Invites,
    /// The instance registry, for each chip's `-<instance>` suffix
    /// (ADR-129). Built afresh with the world, never kept across it.
    instances: SnapshotCache,
    /// The tab bar and the column as this world lays them out, for the
    /// interaction state's steps: built with the world, the tab shown
    /// kept against it ([`ChatPane::set_tab`]).
    laid: ix::Laid,
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
    /// The feed's box, border and all: where the wheel scrolls it.
    feed_box: Rect,
    compose: (Rect, usize),
    /// The width of what leads the compose box's first row: `> `.
    lead: u16,
    text_width: u16,
    chips: Vec<(Rect, Chip)>,
    /// The left column's rows, the column's whole area, and the rows of
    /// each of its lists (agents, channels) for the wheel.
    side: Vec<(Rect, sidebar::SideHit)>,
    side_area: Rect,
    side_lists: [Rect; 2],
    /// The compose box's destination flag, when drawn.
    flag: Rect,
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
    rx: Option<Receiver<Delivery>>,
    /// The files put in the buffer other than by the watcher, by tray and
    /// id ([`Signal::same_file`]), and when: the watcher's copy of one is
    /// dropped, however many messages come between.
    echoed: Vec<((String, String), Instant)>,
    /// The copies of a send to several trays that [`ChatPane::hold`] left
    /// out as the same send, by the file of the copy held: when a mark
    /// withdraws that copy, another takes its place.
    copies: std::collections::HashMap<(String, String), Vec<Signal>>,
    palette: Palette,
    clock: Clock,
    world: Option<World>,
    /// The feed's entries for the foreground tab, rebuilt when the world,
    /// the messages, the tab or the palette change, and each one's
    /// sender by membership id.
    entries: Option<Vec<Entry>>,
    entry_members: Vec<Option<String>>,
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
    /// This program's file and its modification time at start, to notice
    /// an update installed while the chat is open; `None` once noticed.
    binary: Option<(std::path::PathBuf, SystemTime)>,
    hits: Hits,
    /// What the chat asks the shell to open over the pane, until the shell
    /// takes it ([`Pane::take_open`]). A request: what is open is the
    /// interaction state's overlay stack, as the shell reports it.
    to_open: Option<Open>,
    /// The theme this session shows: its name, or `terminal` for the
    /// terminal's own palette. Marked in the Theme menu.
    theme: String,
    /// The id of the shell's question and the menu step it asks about,
    /// until it is answered ([`ChatPane::answered`]), and how many questions
    /// were asked, which numbers their ids.
    asked: Option<(String, menu::Step)>,
    asks: u64,
    /// The terminal reports Ctrl+digits as themselves. Without it Ctrl+3
    /// arrives as Esc, which closes and clears and never quits (ADR-409),
    /// and a setting that asks for Ctrl+digits says they do not arrive.
    enhanced: bool,
    /// The draft a quiet refusal offered to send anyway, and the tab it was
    /// refused on: Enter again on it unchanged, on that tab, sends it, as
    /// Ctrl+Enter does where it arrives. Any edit or a change of tab
    /// withdraws the offer, so another destination gets a fresh check.
    send_anyway: Option<(String, Tab)>,
    /// The chat's settings (`attend.chat.*`), read at start and again
    /// after a command, so `/config` applies at once.
    config: attend_config::ChatConfig,
    /// Whether the left column is shown (`attend.chat.sidebar_key`).
    sidebar: bool,
    /// The pane's width in the frame being drawn ([`Pane::layout`]), told
    /// before the tab bar is drawn: whether the column has room this frame.
    width: Option<u16>,
    /// The lozenge shape the left column's rows are drawn in, the shell's.
    shape: Shape,
    /// The interaction state: where the keyboard is, the walk's preview,
    /// the column row under the focus, what the shell holds open ([`ix`]).
    /// Only [`ChatPane::dispatch`] moves it, through the machine's step.
    ix: ix::Machine,
    /// The first row shown of each of the column's lists, agents then
    /// channels: the wheel moves them, and the cursor keeps itself in view
    /// while `side_follow`, until the wheel moves the list away from it.
    side_scroll: [usize; 2],
    side_follow: bool,
    /// The members a Remove menu listed, for the one picked.
    removable: Vec<menu::Member>,
    /// The places Settings ▸ offered a key's write to, each with its row.
    setting_places: Vec<(String, crate::settings_edit::Place)>,
    /// The DM views the column lists under agents' rows, the operator's
    /// DMs with each and each agent's own ([`dm`]). Hiding one takes it off
    /// the list and leaves every message where it was.
    views: sidebar::Views,
    /// The last name the column showed for each agent whose DM view was
    /// listed, by membership id: the view keeps it when the agent leaves
    /// the column ([`dm`]).
    dm_names: std::collections::HashMap<String, String>,
    /// Each tab's Hide messages here watermark, and the file it is kept in
    /// across restarts ([`crate::hidden`]).
    hidden: crate::hidden::Hidden,
    hidden_path: std::path::PathBuf,
    /// The attend CLI the cleanup items run ([`cleanup`]), and the last
    /// delete made, for Undo delete.
    attend: std::path::PathBuf,
    undo: Option<cleanup::LastDelete>,
    /// The delete or tombstone attend is working on, one at a time, with
    /// no time limit ([`cleanup`]).
    removing: Option<cleanup::Removal>,
    /// How long a question that removes something for everyone stays open
    /// ([`cleanup::DELETE_QUESTION_LIFE`]): a delete's once its count is
    /// shown, Delete channel's, Tombstone…'s and Remove agent's at once.
    delete_question_life: Duration,
}

impl ChatPane {
    /// A chat fed by `rx`, drawn with `palette`.
    pub fn new(rx: Option<Receiver<Delivery>>, palette: Palette) -> ChatPane {
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
            echoed: Vec::new(),
            copies: Default::default(),
            palette,
            clock: Clock::Live,
            world: None,
            entries: None,
            entry_members: Vec::new(),
            heartbeat: true,
            refreshed: None,
            generation: 0,
            last_frame: None,
            dirty: true,
            dry_run: false,
            draws: 0,
            day_seen: None,
            binary: running_binary(),
            hits: Hits::default(),
            to_open: None,
            theme: "terminal".into(),
            asked: None,
            asks: 0,
            enhanced: false,
            send_anyway: None,
            config: crate::settings::load(&std::env::current_dir().unwrap_or_default()),
            sidebar: true,
            width: None,
            shape: Shape::PLAIN,
            ix: ix::Machine::new(),
            side_scroll: [0, 0],
            side_follow: true,
            removable: Vec::new(),
            setting_places: Vec::new(),
            views: Default::default(),
            dm_names: Default::default(),
            hidden: crate::hidden::Hidden::load(&crate::hidden::default_path()),
            hidden_path: crate::hidden::default_path(),
            attend: crate::attend_cli::program(),
            undo: None,
            removing: None,
            delete_question_life: cleanup::DELETE_QUESTION_LIFE,
        }
    }

    /// A status message, as a command's result shows it.
    pub fn say(&mut self, s: impl Into<String>, error: bool) {
        self.status = s.into();
        self.status_set_at = Some(Instant::now());
        self.status_is_error = error;
        self.dirty = true;
    }

    /// The compose box's text and its prompt, as last drawn.
    fn compose_rect(&self) -> Rect {
        let (text, _) = self.hits.compose;
        let lead = self.hits.lead;
        Rect { x: text.x.saturating_sub(lead), width: text.width + lead, ..text }
    }

    /// The feed's entries are out of date: rebuild them, and their layout,
    /// on the next frame.
    fn invalidate(&mut self) {
        self.entries = None;
        self.generation += 1;
        self.dirty = true;
    }

    /// Append `sig`, a file the chat read itself, such as a send read back,
    /// or a local block. The watcher's copy of the file is dropped when it
    /// comes ([`ChatPane::drain`]).
    pub fn push(&mut self, sig: Signal) {
        if !sig.tray.is_empty() {
            self.echoed.push((file_of(&sig), Instant::now()));
        }
        let at = self.signals.len();
        self.hold(sig, at);
    }

    /// Put `sig` in the buffer at `at`. Every writer into the buffer — the
    /// watcher drain, a send reading back what it wrote, a command's local
    /// block — goes through here, so the cap and the copies have a single
    /// home.
    ///
    /// - A file read twice ([`Signal::same_file`]) is dropped, and so is a
    ///   copy of a send already held from another tray
    ///   ([`Signal::same_send`]), kept aside in `copies`.
    /// - The person's direct message sent beside a channel post of the same
    ///   send is marked so, whichever copy arrives first
    ///   ([`Signal::is_beside`]): the operator view shows the post alone.
    /// - Past [`MAX_SIGNALS`] the oldest direct message between others goes
    ///   first, then the oldest message, so others' DMs never push out what
    ///   the person was party to.
    fn hold(&mut self, mut sig: Signal, at: usize) {
        let near = at.saturating_sub(DEDUPE_WINDOW)..(at + DEDUPE_WINDOW).min(self.signals.len());
        if self.signals[near.clone()].iter().any(|s| s.same_file(&sig)) {
            return;
        }
        if let Some(held) = self.signals[near.clone()].iter().find(|s| s.same_send(&sig)) {
            let aside = self.copies.entry(file_of(held)).or_default();
            if !aside.iter().any(|c| c.same_file(&sig)) {
                aside.push(sig);
            }
            return;
        }
        for s in &mut self.signals[near] {
            if sig.is_beside(s) {
                sig.beside_post = true;
            }
            if s.is_beside(&sig) {
                s.beside_post = true;
            }
        }
        self.signals.insert(at, sig);
        while self.signals.len() > MAX_SIGNALS {
            let oldest = self.signals.iter().position(|s| matches!(s.channel, crate::signal::Channel::Elsewhere(_))).unwrap_or(0);
            let gone = self.signals.remove(oldest);
            self.copies.remove(&file_of(&gone));
        }
        self.invalidate();
    }

    /// Where a signal restored by an undo goes: after every message sent
    /// no later than it.
    fn place(&self, sig: &Signal) -> usize {
        let key = |s: &Signal| (s.ts, agent_identity::signal_send_order(&s.id));
        let at = key(sig);
        self.signals.iter().rposition(|s| key(s) <= at).map_or(0, |i| i + 1)
    }

    /// Take the files `gone` out of the buffer, and so out of every view.
    /// A copy held for a send to several trays gives its place to the first
    /// other copy still shown; a direct message held back from the operator
    /// view beside a channel post shows again once no post is left beside it.
    fn withdraw(&mut self, gone: &std::collections::HashSet<(String, String)>) {
        let is_gone = |s: &Signal| !s.tray.is_empty() && gone.contains(&file_of(s));
        let mut kept = Vec::with_capacity(self.signals.len());
        let mut moved = Vec::new();
        for s in std::mem::take(&mut self.signals) {
            if !is_gone(&s) {
                kept.push(s);
                continue;
            }
            let Some(mut aside) = self.copies.remove(&file_of(&s)) else { continue };
            aside.retain(|c| !is_gone(c));
            if aside.is_empty() {
                continue;
            }
            let next = aside.remove(0);
            if !aside.is_empty() {
                self.copies.insert(file_of(&next), aside);
            }
            moved.push(kept.len());
            kept.push(next);
        }
        self.signals = kept;
        for aside in self.copies.values_mut() {
            aside.retain(|c| !is_gone(c));
        }
        self.copies.retain(|_, aside| !aside.is_empty());
        let flagged: Vec<usize> = (0..self.signals.len()).filter(|&i| self.signals[i].beside_post || moved.contains(&i)).collect();
        for i in flagged {
            let near = i.saturating_sub(DEDUPE_WINDOW)..(i + DEDUPE_WINDOW + 1).min(self.signals.len());
            let beside = self.signals[near].iter().any(|p| self.signals[i].is_beside(p));
            self.signals[i].beside_post = beside;
        }
        self.invalidate();
    }

    /// Take what the watcher has sent. True when anything changed.
    pub fn drain(&mut self) -> bool {
        let Some(rx) = &self.rx else { return false };
        let got: Vec<Delivery> = rx.try_iter().collect();
        let any = !got.is_empty();
        for d in got {
            match d {
                Delivery::Arrived(s) => {
                    let key = file_of(&s);
                    if let Some(i) = self.echoed.iter().position(|(k, _)| *k == key) {
                        self.echoed.swap_remove(i);
                        continue;
                    }
                    let at = self.signals.len();
                    self.hold(s, at);
                }
                Delivery::Restored(s) => {
                    let at = self.place(&s);
                    self.hold(s, at);
                }
                Delivery::Withdrawn(files) => self.withdraw(&files.into_iter().collect()),
            }
        }
        // After the copies drained, so a copy that waited in the channel
        // while the chat was stopped still finds its entry.
        self.echoed.retain(|(_, at)| at.elapsed() < ECHO_KEPT);
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
            let alive = |id: &str| attend_presence::alive(id, &Default::default());
            let human = |u: &str| alive(&agent_identity::sanitize_id_component(u));
            // One pass each: the sessions read and the signals walked once,
            // the channels scanned once; the legend and the tab bar take
            // their lists from the column's.
            let rows = all_channels(depth);
            let in_channel = |id: &str| rows.iter().any(|r| r.known.membership.members.iter().any(|m| m == id));
            let seen = seen_identities(&self.signals, &discover_sessions(), depth, alive, human, in_channel, &instances);
            let earlier = earlier_ids(&seen, &instances);
            let seen = settle(seen, &earlier, &tombstoned(), in_channel);
            let live_members = rows.iter().map(|r| r.known.membership.members.iter().filter(|m| alive(m)).count()).collect();
            self.world = Some(World::new(tab_channels(&rows), seen, rows, live_members, invites(!self.dry_run), instances, &self.foreground, &self.views));
            self.remember_dm_names();
            self.refocus();
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

    /// Show `t`. The tab bar holds the channel shown, so it is laid out
    /// again, and a focus on a tab the bar no longer draws follows to the
    /// tab shown ([`ChatPane::refocus`]).
    fn set_tab(&mut self, t: Tab) {
        if t != self.foreground {
            self.send_anyway = None;
            if let Some(w) = &mut self.world {
                let names = tabs::strip_names(&w.groups);
                w.laid.shown = tabs::normalize(t.clone(), &names);
                w.laid.bar = tabs::bar(&names, &w.laid.shown);
            }
            self.foreground = t;
            self.feed = FeedState::default();
            self.invalidate();
            if self.world.is_some() {
                self.refocus();
            }
        }
    }

    /// The membership id of the person at this chat (ADR-170), or `None`
    /// when the chat is no live person: without its heartbeat (`--snap`, a
    /// test's chat) or in a dry run, it joins nothing.
    fn person(&self) -> Option<String> {
        (self.heartbeat && !self.dry_run).then(crate::signal::human_member_id)
    }

    /// Enter in the compose box. On a draft a quiet refusal offered to
    /// send anyway, unchanged since, it sends anyway.
    fn enter(&mut self) {
        let here = (self.input.text().to_string(), self.normal_tab());
        let again = self.send_anyway.as_ref() == Some(&here);
        self.send(again);
    }

    /// Send the draft, or run the command. `anyway` sends to an agent that
    /// is quiet but has not exited, as Claude Code's `chat:sendNow` sends at
    /// once: Ctrl+Enter, or Enter again after the refusal.
    fn send(&mut self, anyway: bool) {
        self.send_anyway = None;
        if self.dry_run {
            return self.enter_dry();
        }
        // Normalize against the live strip (PR #395 finding 6): after a
        // peer dissolves the foregrounded channel, Enter agrees with the
        // destination flag, which has degraded to #open.
        let fg = self.normal_tab();
        let sets_jump = self.input.text().trim_start().starts_with("/config tabs.jump");
        match handle_enter(self.input.text(), &self.signals, &fg, self.person().as_deref(), anyway) {
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
                for s in echo {
                    self.push(s);
                }
            }
            EnterAction::StatusOnly(s) => self.say(s, true),
            EnterAction::Quiet(name) => {
                // The key first, so a narrow status line keeps it; only a
                // key that arrives on this terminal is named (ADR-409).
                let how = if self.enhanced { "Ctrl+Enter or Enter again to send now" } else { "press Enter again to send" };
                self.say(format!("{how}: @{name} is quiet (no heartbeat)"), true);
                self.send_anyway = Some((self.input.text().to_string(), self.normal_tab()));
            }
            EnterAction::ClearWithStatusAndFocus { status, focus } => {
                self.say(status, false);
                self.input.clear();
                // `/dissolve` closed the foreground tab: move focus where
                // the handler advanced it.
                self.set_tab(focus);
            }
            EnterAction::HideHere => {
                self.input.clear();
                self.ask_hide(fg);
            }
            // Refused while a delete runs, the command stays in the box.
            EnterAction::DeleteRead(c) => {
                if !self.refused_while_removing() {
                    self.input.clear();
                    self.delete_read(crate::attend_cli::Scope::channel(&c), "all");
                }
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
        self.apply_settings(crate::settings::load(&std::env::current_dir().unwrap_or_default()));
    }

    /// Take `config` as the chat's settings. A `sidebar_key` newly set to
    /// `none` shows the column: no key would bring a hidden one back.
    fn apply_settings(&mut self, config: attend_config::ChatConfig) {
        if config.sidebar_key == "none" && self.config.sidebar_key != "none" {
            self.sidebar = true;
            self.invalidate();
        }
        self.config = config;
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
                let shown = self.normal_tab();
                let to = self.flag_label(&text, &shown).unwrap_or(format!("#{}", tabs::send_scope(&shown)));
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

    /// Tab on a word to complete ([`keys::completable`]): complete it, or
    /// the next candidate.
    fn tab_key(&mut self) {
        let world = self.world();
        let (agents, channels) = sidebar::completion_pool(&world.seen, &world.rows);
        let r = handle_tab(self.input.text(), self.input.cursor(), self.tab_cycle.take(), &agents, &channels);
        self.input.set(r.new_buf, r.new_cursor);
        self.tab_cycle = r.new_cycle;
    }

    /// Esc with nothing open: the message selected and what the bar says
    /// are cleared; the draft stays.
    fn clear(&mut self) {
        self.feed.selected = None;
        self.status.clear();
        self.status_set_at = None;
        self.status_is_error = false;
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

    fn layout(&mut self, area: Rect) {
        self.width = Some(area.width);
    }

    fn tabs(&mut self) -> Vec<PaneTab> {
        view::tabs(self)
    }

    /// The bar's index of the tab shown, in [`tabs::bar`]'s order.
    fn tab(&mut self) -> usize {
        let shown = self.normal_tab();
        self.slot_of(&shown).unwrap_or(0)
    }

    /// Show the bar's tab `i`: a channel or the operator view. The slots
    /// and an index past the tabs do nothing.
    fn set_tab(&mut self, i: usize) {
        self.dispatch(ix::Ev::Jump(i));
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

    /// The chat's keys, by where the focus is ([`focus`]). Alt+1..9 (a
    /// tab), Alt+m (the mouse), F1 (the keys), F2 (the tab bar) and Ctrl-C
    /// (quit) are the shell's, and never reach here; outside the compose
    /// box their single-key forms (`1`..`9`, `m`, `?`, `q`) are the shell's
    /// too.
    ///
    /// Esc never quits (ADR-409): in the compose box it clears the message
    /// selected and what the bar says, the draft kept; elsewhere it brings
    /// the focus back to the box. Where Ctrl+3 arrives as Esc it is the same
    /// Esc, so a tab key can never close the chat.
    fn key(&mut self, k: KeyEvent) -> Keyed {
        self.dirty = true;
        if self.is_sidebar_key(&k) {
            self.dispatch(ix::Ev::KeyTaken);
            self.toggle_sidebar();
            return Keyed::Done;
        }
        self.dispatch(ix::Ev::Key(k));
        Keyed::Done
    }

    fn wheel(&mut self, up: bool) {
        self.scroll_by(up, WHEEL_ROWS);
    }

    /// The chat owns the focus, the tab bar's included, and Esc never quits
    /// (ADR-409). The compose box and the feed take every plain key as
    /// text; on the tab bar and the column the shell's single keys work as
    /// the key help names them, and on a channel's row a plain digit picks a
    /// channel by its number ([`ix::Ix::ctx`]).
    fn keymap_ctx(&self) -> agent_tui::Ctx {
        self.ix.ctx()
    }

    /// The column numbers its channels, from 1, for Ctrl+digit, Alt+digit
    /// and a plain digit on a channel's row; the shell's tab bar carries no
    /// numbers of its own, and the chat's tabs carry the column's while it
    /// is hidden ([`view::tabs`]).
    fn own_numbers(&self) -> Option<&'static str> {
        Some("channel")
    }

    /// Channel `n` of the column, or with 0 the operator view, chosen as a
    /// click on its row chooses ([`ix::Ev::Numbered`]).
    fn jump(&mut self, n: usize, menu: bool) {
        self.dirty = true;
        self.dispatch(ix::Ev::Numbered { n, menu });
    }

    /// An overlay holds the keyboard: the compose box draws no caret and
    /// its border is set back, so the focus shows in one place.
    fn overlay_opened(&mut self, o: agent_tui::Overlay) {
        self.dispatch(ix::Ev::OverlayOpened(o));
    }

    fn overlay_closed(&mut self, o: agent_tui::Overlay) {
        self.dispatch(ix::Ev::OverlayClosed(o));
    }

    /// The bar's index of the focused slot, against the bar as the world
    /// laid it out; with the world unread, the bar of no channels.
    fn focus_tab(&self) -> Option<usize> {
        let focus::Focus::Tab(slot) = self.ix.focus() else { return None };
        match &self.world {
            Some(w) => w.laid.bar.iter().position(|s| s == slot),
            None => tabs::bar(&[], &self.foreground).iter().position(|s| s == slot),
        }
    }

    fn focus_tabs(&mut self) {
        self.dispatch(ix::Ev::ToggleBar);
    }

    /// A paste goes into the compose box whole, at the cursor, and the focus
    /// with it: its lines stay one draft, and nothing is sent until Enter. A
    /// waiting question the shell answers no first, as a typed character
    /// answers it, so a `y` in the paste never does. Control characters other
    /// than newline and tab are left out: an escape sequence in the
    /// clipboard is not drawn into the box or sent. An agent's own DMs take
    /// no paste.
    fn paste(&mut self, text: &str) {
        // A view with nothing to send has no box to paste into.
        if !self.foreground.composes() {
            return self.say("read only: nothing pasted", false);
        }
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        self.dispatch(ix::Ev::Paste);
        let kept: String = text.chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\t')).collect();
        self.input.insert_str(&kept);
        self.tab_cycle = None;
        self.dirty = true;
    }

    /// The wheel scrolls what is under it: one of the column's lists, or
    /// the feed. Over the compose box and the helper row it does nothing.
    fn wheel_at(&mut self, up: bool, at: Position) {
        match self.hits.side_lists.iter().position(|r| r.contains(at)) {
            Some(list) => self.side_wheel(list, up),
            None if self.hits.feed_box.contains(at) => self.wheel(up),
            None => {}
        }
    }

    /// A click on a message selects it and brings it whole into view; on
    /// the compose box's text it puts the cursor there; on a chip of the
    /// helper row it completes what is being typed to it. The keyboard goes
    /// back to the compose box, but for a click on the column while it has
    /// the focus, which moves it to the row clicked, and one on a message
    /// while the feed has it.
    fn click(&mut self, at: Position) {
        self.dirty = true;
        let (text, start) = self.hits.compose;
        let flag = self.hits.flag.contains(at);
        let feed = self.hits.feed.contains(at);
        let compose = self.compose_rect().contains(at);
        let chip = self.hits.chips.iter().find(|(r, _)| r.contains(at)).map(|(_, c)| c.clone());
        let row = (!flag && !feed && !compose && chip.is_none()).then(|| self.hits.side.iter().find(|(r, _)| r.contains(at)).map(|(_, h)| h.clone())).flatten();
        self.dispatch(ix::Ev::Click(ix::Click::Pane { column: self.hits.side_area.contains(at), feed, row }));
        if flag {
            return self.flag_menu();
        }
        if feed {
            if let Some(i) = self.feed.entry_at(at.y - self.hits.feed.y) {
                self.feed.reveal(i);
            }
            return;
        }
        if compose {
            let col = at.x.saturating_sub(text.x) as usize;
            self.input.click(self.hits.text_width, start + (at.y - text.y) as usize, col);
            self.tab_cycle = None;
            return;
        }
        if let Some(chip) = chip {
            self.complete(chip);
        }
    }

    /// The column passes a click that closes a menu or a question on to
    /// its row, as the tab bar does.
    fn click_through(&self, at: Position) -> bool {
        self.side_drawn() && self.hits.side_area.contains(at)
    }

    /// The keys of the element the focus is on, for the footer, then the
    /// rest for the key help ([`ChatPane::focus_bindings`]).
    fn bindings(&self) -> Vec<Binding> {
        self.focus_bindings()
    }

    fn mode(&self) -> String {
        self.focus_mode()
    }

    /// The status slot (#398, #400): a fresh result asserts itself, an
    /// error in the error role; past [`STATUS_ASSERT`] a delete or a
    /// tombstone still running says so, else the help of a command being
    /// typed, or the last result, set back.
    fn status(&mut self) -> Option<(String, Tone)> {
        let fresh = self.status_set_at.is_some_and(|t| t.elapsed() < STATUS_ASSERT);
        let help = crate::slash::contextual_help(self.input.text()).map(|h| (h, self.input.text().starts_with('/')));
        let (line, how) = status_slot(fresh, self.status_is_error, self.busy_line(), help, &self.status);
        let tone = match how {
            Some(true) => Tone::Err,
            Some(false) => Tone::Said,
            None => Tone::Back,
        };
        (!line.is_empty()).then_some((line, tone))
    }

    /// `attend.chat.mouse`, on unless set off: the chat is worked by
    /// clicks (ADR-405), and Shift-drag still selects text in most
    /// terminals. Alt+m gives the terminal its mouse back, for selection
    /// and middle-click paste.
    fn mouse_default(&self) -> bool {
        self.config.mouse
    }

    /// What quitting now would lose: a draft, and a delete or tombstone
    /// still running, which would go on unseen.
    fn unsaved(&self) -> Option<String> {
        let parts: Vec<&str> = [(!self.input.is_empty()).then_some("a draft in the compose box"), self.running_what()].into_iter().flatten().collect();
        (!parts.is_empty()).then(|| parts.join(" and "))
    }

    fn has_tab_menus(&self) -> bool {
        true
    }

    /// A tab's digit, a click on it, a right click on it or Enter on the
    /// focused bar: the one activation path ([`ChatPane::activate`]). A
    /// repeat on the tab shown opens its menu when
    /// `attend.chat.tabs.menu_on_repeat` says so.
    fn activate_tab(&mut self, i: usize, menu: bool) {
        self.dispatch(ix::Ev::TabActivated { i, menu });
    }

    /// A right click: the menu of the column row, the destination flag or
    /// the sender's name under it, at once.
    fn context(&mut self, at: Position) {
        self.dirty = true;
        self.context_at(at);
    }

    fn new_item_label(&self) -> Option<&'static str> {
        Some("new channel")
    }

    fn new_item(&mut self) {
        self.new_channel();
    }

    /// The shell took a click (a tab, the bottom bar): the keyboard goes
    /// back to the compose box, as a click in the pane sends it.
    fn interrupted(&mut self) {
        self.dispatch(ix::Ev::Click(ix::Click::Shell));
    }

    /// A click on a tab or a channel row shows it and again opens its menu;
    /// on a claude's row it shows the DMs with it, and again opens the
    /// agent's menu. A human's row and the destination flag open theirs at
    /// once.
    fn click_help(&self) -> Option<String> {
        Some("shows a channel or agent's DMs, again its menu; flag: menu".into())
    }

    /// A right click opens the menu at once on a tab, a column row or a
    /// sender's name in the feed.
    fn right_click_help(&self) -> Option<String> {
        Some("a tab, a row or a sender's name: its menu at once".into())
    }

    fn quit_help(&self) -> Option<String> {
        Some("quit; q asks first, ^C over a draft; so does ≡ → Quit".into())
    }

    fn take_open(&mut self) -> Option<Open> {
        self.to_open.take()
    }

    /// The shell took a key of the operator's (the key help, the mouse,
    /// quitting, a tab, a new channel).
    fn shell_key_taken(&mut self) {
        self.dispatch(ix::Ev::KeyTaken);
    }

    fn open_dropped(&mut self, open: Open) {
        self.question_dropped(open);
    }

    fn picked(&mut self, id: &str, values: Vec<String>) {
        if values.get(1).is_some_and(|v| v == agent_tui::EXPIRED) {
            return self.answered_expired(id);
        }
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
            "Enter sends to the shown channel; ⋯all, every channel together, sends to\n\
             #open. @name or #channel at the start addresses the message; Tab completes\n\
             them, and with nothing to complete moves on. /help lists the slash commands.\n\
             The column numbers the channels: Ctrl+digit shows one, 0 the operator view.\n\
             Tab and Shift+Tab walk the tabs, the column, the feed and the box, and Enter\n\
             opens a menu. Esc goes to the feed, then the ≡ menu, then back to where you\n\
             were. `attend message` reads and sends on the same bus outside the screen."
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
        self.attend_answered();
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
            if self.binary.as_ref().is_some_and(|(path, at)| updated_on_disk(path, *at)) {
                self.binary = None;
                self.say(UPDATED_ON_DISK, false);
            }
        }
    }
}

/// What the status line says once the chat's program changed on disk.
pub const UPDATED_ON_DISK: &str =
    "attend-chat was updated on disk: restart it to read new messages and direct replies correctly";

/// A signal's file: its tray and its id ([`Signal::same_file`]).
fn file_of(sig: &Signal) -> (String, String) {
    (sig.tray.clone(), sig.id.clone())
}

/// Each id a live session had before a `/clear`, with that session: from
/// the enrollment that carried it (`attend_presence::enrollment::carry`),
/// and from the instance registry's alias of a dead sighting's id, read
/// through the world's registry snapshot.
fn earlier_ids(seen: &[Seen], registry: &SnapshotCache) -> std::collections::HashMap<String, String> {
    let mut earlier = std::collections::HashMap::new();
    for s in seen.iter().filter(|s| s.identity.is_claude) {
        if s.live {
            for id in attend_presence::enrollment::previous_ids(&s.member) {
                earlier.insert(id, s.member.clone());
            }
        } else if let Some(next) = registry.successor(&s.identity.cwd, &s.member) {
            earlier.entry(s.member.clone()).or_insert(next);
        }
    }
    earlier
}

/// The sessions tombstoned in the mark store (ADR-404 D3e).
fn tombstoned() -> std::collections::HashSet<String> {
    attend_groups::tombstones::sessions(&crate::signal::signals_base()).into_iter().collect()
}

/// This program's file and its modification time now.
fn running_binary() -> Option<(std::path::PathBuf, SystemTime)> {
    let path = std::env::current_exe().ok()?;
    let at = std::fs::metadata(&path).and_then(|m| m.modified()).ok()?;
    Some((path, at))
}

/// Whether the file at `path` is no longer the one modified at `at`: an
/// update replaced or rewrote it. A file that is gone reads as unchanged,
/// since the program may be moved aside while it is replaced.
pub fn updated_on_disk(path: &std::path::Path, at: SystemTime) -> bool {
    std::fs::metadata(path).and_then(|m| m.modified()).is_ok_and(|now| now != at)
}

/// The chat on the shell: [`ChatPane`] inside `agent_tui::App`. The
/// terminal runs [`Chat::into_app`] through `agent_tui::run`; the headless
/// `--snap` and the tests drive the same shell as a [`Screen`].
pub struct Chat {
    app: App,
}

impl Chat {
    /// A chat fed by `rx`, drawn with `palette` and lozenges in `shape`.
    pub fn new(rx: Option<Receiver<Delivery>>, palette: Palette, shape: Shape) -> Chat {
        let mut pane = ChatPane::new(rx, palette);
        pane.shape = shape;
        Chat { app: App::with_pane("chat", pane).shape(shape) }
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

    /// Draw with the theme `name` names, or the terminal's own palette for
    /// `None`, at the depth the chat was made with. A theme that did not
    /// load falls back and the status line says so; the Theme menu marks
    /// what the screen shows.
    pub fn theme(mut self, name: Option<&str>) -> Chat {
        if let Some(w) = self.pane_mut().use_theme(name) {
            self.say(format!("theme: {w}"), true);
        }
        self
    }

    /// Show the left column of agents and channels, or not (ADR-405);
    /// shown by default.
    pub fn sidebar(mut self, on: bool) -> Chat {
        self.pane_mut().sidebar = on;
        self
    }

    /// Whether the left column is shown, as `attend.chat.sidebar_key`
    /// last left it.
    pub fn sidebar_shown(&self) -> bool {
        self.pane().sidebar
    }

    /// `attend.chat.tabs.menu_on_repeat` for this chat, whatever the
    /// settings file says.
    pub fn menu_on_repeat(mut self, on: bool) -> Chat {
        self.pane_mut().config.menu_on_repeat = on;
        self
    }

    /// `attend.chat.tabs.jump` and `attend.chat.tabs.focus_key` for this
    /// chat, whatever the settings file says.
    pub fn tab_keys(mut self, jump: &str, focus_key: &str) -> Chat {
        let config = &mut self.pane_mut().config;
        config.jump = jump.into();
        config.focus_key = focus_key.into();
        self
    }

    /// Keep Hide messages here's watermarks in `path` rather than
    /// attend-chat's state file, reading what it holds.
    pub fn hidden_file(mut self, path: impl Into<std::path::PathBuf>) -> Chat {
        let pane = self.pane_mut();
        pane.hidden_path = path.into();
        pane.hidden = crate::hidden::Hidden::load(&pane.hidden_path);
        pane.invalidate();
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

    /// What a running delete or tombstone is doing (`deleting…`), while one
    /// runs.
    pub fn removing(&self) -> Option<String> {
        self.pane().removing_now()
    }

    /// How long a question that removes something for everyone stays open
    /// (a delete's once its count is shown), rather than
    /// [`cleanup::DELETE_QUESTION_LIFE`].
    pub fn delete_question_life(mut self, life: Duration) -> Chat {
        self.pane_mut().delete_question_life = life;
        self
    }

    /// How long a question's yes stays set back once it is ready to answer,
    /// rather than the shell's second ([`agent_tui::App::set_question_arm`]).
    pub fn question_arm(mut self, arm: Duration) -> Chat {
        self.app.set_question_arm(arm);
        self
    }

    /// Stop the delete or tombstone running, as `≡` ▸ Stop does.
    pub fn pane_stop_removing(&mut self) {
        self.pane_mut().stop_removing();
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

    fn paste(&mut self, text: &str) {
        self.app.paste(text);
    }

    fn tick_every(&self) -> Option<Duration> {
        self.app.tick_every()
    }

    fn tick(&mut self) {
        self.app.tick_pane();
        self.app.tick();
    }
}

impl World {
    /// A world of these channels, invitations and identities, laid out for
    /// the tab `shown` against its channels, with the DM views `views` lists.
    #[allow(clippy::too_many_arguments)]
    fn new(groups: Vec<KnownGroup>, seen: Vec<Seen>, rows: Vec<ChannelRow>, live_members: Vec<usize>, invites: Invites, instances: SnapshotCache, shown: &Tab, views: &sidebar::Views) -> World {
        let names = tabs::strip_names(&groups);
        let shown = tabs::normalize(shown.clone(), &names);
        let laid = ix::Laid { bar: tabs::bar(&names, &shown), rows: sidebar::side_items_of(&seen, &rows, &invites, views), shown };
        World { groups, seen, rows, live_members, invites, instances, laid }
    }

    /// What the frame shows of the world: the channels with their members,
    /// invitations and descriptions, and the identities with their instance
    /// names.
    fn fingerprint(&self) -> String {
        let mut out = String::new();
        for g in &self.groups {
            out += &format!("#{}|{:?}|{:?}\n", g.group.name, g.membership.members, g.membership.description);
        }
        for s in &self.seen {
            out += &format!("@{}|{}|{}|{}\n", s.identity.nickname, s.identity.cwd, s.identity.is_claude, s.live);
        }
        for (r, live) in self.rows.iter().zip(&self.live_members) {
            out += &format!("={}|{:?}|{}|{live}\n", r.known.group.name, r.last, r.on_disk);
        }
        let mut invites: Vec<&attend_groups::Invite> = self.invites.values().flatten().collect();
        invites.sort_by(|a, b| (&a.channel, &a.member).cmp(&(&b.channel, &b.member)));
        for i in invites {
            out += &format!("+{}|{}|{:?}|{:?}\n", i.channel, i.member, i.by, i.at);
        }
        out
    }
}

/// The status-slot content under the #398/#400 priority rule: a fresh
/// result (error or success) > the help of a slash command being typed >
/// what a running delete or tombstone is doing (`busy`) > other
/// single-match help > the stored result. `help` carries whether it is a
/// slash command's. The second value is how it reads: `Some(true)` an
/// error, `Some(false)` a success or the work running, `None` set back.
fn status_slot(fresh: bool, is_error: bool, busy: Option<String>, help: Option<(String, bool)>, stored: &str) -> (String, Option<bool>) {
    if fresh {
        return (stored.to_string(), Some(is_error));
    }
    match (help, busy) {
        (Some((help, true)), _) => (help, None),
        (_, Some(busy)) => (busy, Some(false)),
        (help, None) => (help.map_or_else(|| stored.to_string(), |(h, _)| h), None),
    }
}

#[cfg(test)]
mod world_tests {
    use super::*;

    /// An invitation sent or answered redraws the column at the next
    /// refresh: the watcher reads only messages, so the refresh's
    /// fingerprint is what notices it.
    #[test]
    fn the_fingerprint_changes_when_an_invitation_comes_or_goes() {
        let world = |invites: Invites| World::new(Vec::new(), Vec::new(), Vec::new(), Vec::new(), invites, SnapshotCache::new(), &Tab::Merged, &Default::default()).fingerprint();
        let invite = attend_groups::Invite { channel: "infra".into(), member: "s2".into(), by: Some("aaron".into()), at: None };
        let none = world(Invites::new());
        let one = world([("infra".to_string(), vec![invite.clone()])].into());
        assert_ne!(none, one, "an invitation sent");
        let renewed = world([("infra".to_string(), vec![attend_groups::Invite { at: Some(std::time::UNIX_EPOCH), ..invite }])].into());
        assert_ne!(one, renewed, "an invitation sent again rewrites its time");
    }
}

#[cfg(test)]
mod status_slot_tests {
    use super::*;

    #[test]
    fn fresh_error_asserts_over_help() {
        let (text, how) = status_slot(true, true, None, Some(("/dissolve — help".into(), true)), "1 live member — not dissolving");
        assert_eq!(text, "1 live member — not dissolving");
        assert_eq!(how, Some(true));
    }

    #[test]
    fn fresh_success_asserts() {
        let (text, how) = status_slot(true, false, None, Some(("help".into(), false)), "joined #x");
        assert_eq!(text, "joined #x");
        assert_eq!(how, Some(false));
    }

    #[test]
    fn expired_window_yields_to_help_then_stored() {
        let (text, how) = status_slot(false, true, None, Some(("/dissolve — help".into(), true)), "err");
        assert_eq!(text, "/dissolve — help");
        assert_eq!(how, None);
        let (text, _) = status_slot(false, true, None, None, "err");
        assert_eq!(text, "err");
    }

    #[test]
    fn a_running_delete_holds_the_line_past_a_fresh_result() {
        let busy = || Some("deleting…".to_string());
        let (text, how) = status_slot(true, true, busy(), None, "still deleting: one delete or tombstone runs at a time");
        assert_eq!((text.as_str(), how), ("still deleting: one delete or tombstone runs at a time", Some(true)), "a fresh note first");
        let (text, how) = status_slot(false, false, busy(), Some(("@Ada — an agent".into(), false)), "hid 3 messages");
        assert_eq!((text.as_str(), how), ("deleting…", Some(false)), "then the running delete, over other help and the last result");
        let (text, how) = status_slot(false, false, busy(), Some(("/dissolve — help".into(), true)), "hid 3 messages");
        assert_eq!((text.as_str(), how), ("/dissolve — help", None), "a slash command's help while it is typed");
        let (text, _) = status_slot(false, false, busy(), None, "hid 3 messages");
        assert_eq!(text, "deleting…", "typing with no help to show leaves the running line");
    }
}

#[cfg(test)]
mod settings_tests {
    use super::*;

    #[test]
    fn a_sidebar_key_set_to_none_shows_a_hidden_column() {
        let mut c = ChatPane::new(None, Palette::default());
        c.config.sidebar_key = "f3".into();
        c.sidebar = false;
        let mut none = c.config.clone();
        none.sidebar_key = "none".into();
        c.apply_settings(none.clone());
        assert!(c.sidebar, "no key could show it again");
        c.sidebar = false;
        c.apply_settings(none);
        assert!(!c.sidebar, "a reload that leaves the key at none keeps what the menu chose");
    }
}

#[cfg(test)]
mod paste_tests {
    use super::*;

    #[test]
    fn a_paste_leaves_out_control_characters() {
        let mut c = ChatPane::new(None, Palette::default());
        // An escape sequence as a clipboard can carry one: ESC, then `[31m`.
        let esc = char::from(0x1b);
        c.paste(&format!("yes{esc}[31m red\x07\tok\r\nnext"));
        assert_eq!(c.input.text(), "yes[31m red\tok\nnext", "escape and bell dropped; tab and newline kept");
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

#[cfg(test)]
mod delivery_tests {
    use super::*;
    use crate::signal::Channel;

    fn sig(id: &str, ts: u64, channel: Channel) -> Signal {
        Signal {
            id: id.into(),
            from: "claude:22222222-2222-4222-8222-222222222222".into(),
            project: "x".into(),
            cwd: "/home/x".into(),
            reply_to: None,
            message: format!("body of {id}"),
            ts,
            tray: match &channel {
                Channel::Open => "_broadcast".into(),
                _ => "aaron".into(),
            },
            channel,
            beside_post: false,
        }
    }

    fn fed() -> (ChatPane, std::sync::mpsc::Sender<Delivery>) {
        let (tx, rx) = std::sync::mpsc::channel();
        (ChatPane::new(Some(rx), Palette::default()), tx)
    }

    fn ids(c: &ChatPane) -> Vec<&str> {
        c.signals.iter().map(|s| s.id.as_str()).collect()
    }

    /// A message a mark hides leaves the buffer, which every view reads;
    /// restored by an undo, it goes back among the others by its time.
    #[test]
    fn a_withdrawn_message_leaves_every_view_and_a_restored_one_takes_its_place() {
        let (mut c, tx) = fed();
        for s in [sig("a", 10, Channel::Open), sig("b", 20, Channel::Open), sig("dm", 25, Channel::Person), sig("c", 30, Channel::Open)] {
            tx.send(s.into()).unwrap();
        }
        c.drain();
        tx.send(Delivery::Withdrawn(vec![("_broadcast".into(), "b".into()), ("aaron".into(), "dm".into())])).unwrap();
        c.drain();
        assert_eq!(ids(&c), ["a", "c"]);
        for tab in [Tab::Merged, Tab::Channel("open".into()), Tab::Dm("22222222-2222-4222-8222-222222222222".into())] {
            assert!(!c.signals.iter().any(|s| tabs::shows(s, &tab) && (s.id == "b" || s.id == "dm")), "{tab:?}");
        }
        tx.send(Delivery::Restored(sig("b", 20, Channel::Open))).unwrap();
        tx.send(Delivery::Restored(sig("dm", 25, Channel::Person))).unwrap();
        c.drain();
        assert_eq!(ids(&c), ["a", "b", "dm", "c"]);
    }

    /// A send read back holds its file; the watcher's copy is dropped
    /// however many messages arrive between the write and the copy.
    #[test]
    fn a_send_read_back_drops_its_watcher_copy_however_much_comes_between() {
        let (mut c, tx) = fed();
        let mine = sig("mine", 5, Channel::Open);
        c.push(mine.clone());
        for i in 0..(DEDUPE_WINDOW + 44) {
            tx.send(sig(&format!("other-{i}"), 6, Channel::Open).into()).unwrap();
        }
        tx.send(mine.into()).unwrap();
        c.drain();
        assert_eq!(c.signals.iter().filter(|s| s.id == "mine").count(), 1);
        assert!(c.echoed.is_empty(), "the copy took its entry");
    }

    /// An entry waits a few seconds for its copy, and no longer.
    #[test]
    fn an_echo_entry_is_dropped_after_its_time() {
        let (mut c, _tx) = fed();
        c.push(sig("mine", 5, Channel::Open));
        c.echoed[0].1 = c.echoed[0].1.checked_sub(ECHO_KEPT).expect("the clock runs past the keeping time");
        c.drain();
        assert!(c.echoed.is_empty());
    }

    /// A drain with nothing for an entry keeps it, and the copy that comes
    /// later still finds it.
    #[test]
    fn an_echo_entry_outlasts_a_drain_without_its_copy() {
        let (mut c, tx) = fed();
        let mine = sig("mine", 5, Channel::Open);
        c.push(mine.clone());
        c.drain();
        assert_eq!(c.echoed.len(), 1, "nothing came for it");
        tx.send(mine.into()).unwrap();
        c.drain();
        assert_eq!(ids(&c), ["mine"]);
    }

    /// The person's send to agents in two projects: one copy in each tray.
    fn sent_to_two(tray: &str) -> Signal {
        Signal {
            id: format!("aaron-{tray}"),
            from: "external:aaron@kitty".into(),
            tray: tray.into(),
            channel: Channel::Sent("s1,s2".into()),
            ..sig("x", 40, Channel::Open)
        }
    }

    /// A mark on the copy held of a send to several trays leaves the send
    /// shown by another copy, until every copy is marked.
    #[test]
    fn a_withdrawn_copy_of_a_send_gives_its_place_to_another() {
        let (mut c, tx) = fed();
        for s in [sig("a", 10, Channel::Open), sent_to_two("-work-one"), sent_to_two("-work-two"), sig("c", 50, Channel::Open)] {
            tx.send(s.into()).unwrap();
        }
        c.drain();
        assert_eq!(ids(&c), ["a", "aaron--work-one", "c"], "one copy is held");
        tx.send(Delivery::Withdrawn(vec![("-work-one".into(), "aaron--work-one".into())])).unwrap();
        c.drain();
        assert_eq!(ids(&c), ["a", "aaron--work-two", "c"], "the other copy takes its place");
        tx.send(Delivery::Withdrawn(vec![("-work-two".into(), "aaron--work-two".into())])).unwrap();
        c.drain();
        assert_eq!(ids(&c), ["a", "c"]);
        assert!(c.copies.is_empty());
    }

    /// The copies kept aside go with the copy held when the cap evicts it.
    #[test]
    fn the_copies_aside_go_with_their_held_copy() {
        let (mut c, tx) = fed();
        tx.send(sent_to_two("-work-one").into()).unwrap();
        tx.send(sent_to_two("-work-two").into()).unwrap();
        c.drain();
        assert_eq!(c.copies.len(), 1);
        for i in 0..MAX_SIGNALS {
            tx.send(sig(&format!("n-{i}"), 60, Channel::Open).into()).unwrap();
        }
        c.drain();
        assert!(c.copies.is_empty());
    }

    /// The direct message sent beside a channel post shows in the operator
    /// view again once a mark withdraws the post.
    #[test]
    fn a_withdrawn_post_lets_its_beside_message_show() {
        let (mut c, tx) = fed();
        let post = Signal { id: "post".into(), tray: "_broadcast".into(), channel: Channel::Open, ..sent_to_two("_broadcast") };
        let dm = sent_to_two("-work-one");
        tx.send(post.into()).unwrap();
        tx.send(dm.into()).unwrap();
        c.drain();
        let shown = |c: &ChatPane| c.signals.iter().filter(|s| tabs::shows(s, &Tab::Merged)).map(|s| s.id.clone()).collect::<Vec<_>>();
        assert_eq!(shown(&c), ["post"], "the post alone");
        tx.send(Delivery::Withdrawn(vec![("_broadcast".into(), "post".into())])).unwrap();
        c.drain();
        assert_eq!(shown(&c), ["aaron--work-one"]);
    }
}
