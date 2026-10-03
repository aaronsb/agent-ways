//! The introspect screens on agent-tui (ADR-504 §1, §9), as the tabs of
//! one screen (#738): the session picker; a session's timeline, replayed
//! or live, with its why-fired view; the session's semantic fires; and the
//! judge's spend over the sessions in scope. One [`Introspect`] holds them
//! all, so the process runs one screen session. A digit picks a tab, Enter
//! in the picker opens a session on the timeline, and Esc goes back to the
//! picker.
//!
//! Replay and live are one view (#780): Enter on a live session opens its
//! replay at the newest frame, following. New frames append while the
//! cursor rides the newest; moving back stops the follow and End resumes
//! it. The follow re-reads the event log only when a stat of the log or the
//! session's transcript, taken on the backoff of [`super::live`], shows a
//! write. A replay of a quiet session watches its transcript on the same
//! backoff and goes live when it is written: following at the newest
//! frame, paused on an earlier one.

use std::collections::HashMap;
use std::time::Duration;

use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};
use agent_tui::ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use agent_tui::ratatui::style::{Modifier, Style};
use agent_tui::ratatui::text::{Line, Span};
use agent_tui::ratatui::widgets::{Cell, HighlightSpacing, List, ListItem, ListState, Paragraph, Row, Table, TableState};
use agent_tui::ratatui::Frame as Draw;
use agent_tui::screen::Screen;
use agent_tui::{App, Binding, Keyed, Pane, PaneTab, Tone};
use crate::cmd::render;
use crate::cmd::screen_host::pane;
use agent_tui::theme::{self, Ground, Palette, Shape};
use agent_tui::timeline::{Playback, Scrubber};
use ways_agent_core::spend::{self, Group};
use ways_core::introspection::SessionIntrospection;

use super::live::{Follow, SAMPLE_TICK};
use super::agents::Agents;
use super::model::{ActiveWay, Frame, Outcome};
use super::report::Reports;
use super::table;
use super::why::{self, WhyIndex};
use super::fires_tab::Fires;
use super::picker::{draw_picker, Picker};

#[derive(Clone, Copy, PartialEq, Debug)]
enum View {
    Timeline,
    Why,
}

/// What a key in the replay asks of the screens.
enum Step {
    Stay,
    Back,
    Quit,
}

/// One session's timeline: its frames, where the replay is, and the
/// why-fired view's state.
pub(crate) struct Replay {
    pub(crate) session_id: String,
    pub(crate) project: String,
    pub(crate) window_k: u64,
    pub(crate) frames: Vec<Frame>,
    pub(crate) play: Playback,
    view: View,
    /// Which ways the frames show: the injected ones, or with `matched`
    /// every matched candidate, the judge-blocked ones too (#742).
    matched: bool,
    /// Whether the relevance judge saw this session: the header names the
    /// injected view only then.
    pub(crate) judged: bool,
    /// What the judge spent on this session, while it made a call.
    pub(crate) spend: Option<Group>,
    /// Whether the header gives the spend as cost; tokens otherwise.
    pub(crate) cost: bool,
    /// The session's semantic fires, lowest score first, and the one
    /// selected on the fires tab.
    pub(crate) fires: Fires,
    /// The selected way of the frame shown, in both views.
    sel: usize,
    /// The why-fired detail's scroll, and its page from the last frame.
    scroll: usize,
    page: usize,
    /// The model's why index, read the first time the view opens.
    pub(crate) why: Option<WhyIndex>,
    /// Whether the why index and the frames come from the event log, so
    /// they can be read again; tests set them by hand.
    pub(crate) from_log: bool,
    /// Way bodies by file, read once.
    pub(crate) bodies: HashMap<String, Option<String>>,
    /// Live, the session's event log and transcript under stat on the
    /// backoff; quiet, its transcript, watched for the write that makes it
    /// live. A replay read from the log has one while its transcript is
    /// within the cutoff.
    pub(crate) follow: Option<Follow>,
    /// The session's transcript, the follow's to state.
    transcript: Option<std::path::PathBuf>,
    /// The agents that fired, named for the Agent column (#814).
    pub(crate) agents: Agents,
    /// Now, in Unix seconds, for how long ago the newest frame fired.
    pub(crate) now: u64,
    table: TableState,
    list: ListState,
    /// Where the last frame drew the track, the rows and the why-fired text.
    hits: Hits,
}

/// Where a replay's frame drew what a click or the wheel can hit.
#[derive(Default, Clone)]
struct Hits {
    scrub: Rect,
    /// The table of ways, or the why view's list of them, and the lines
    /// each of the table's rows takes.
    rows: Rect,
    heights: Vec<u16>,
    /// The why view's text.
    text: Rect,
}

impl Replay {
    pub(crate) fn new(session_id: String, project: String, window_k: u64, frames: Vec<Frame>, play: Playback) -> Replay {
        let mut r = Replay {
            session_id,
            project,
            window_k,
            frames,
            play,
            view: View::Timeline,
            matched: false,
            judged: false,
            spend: None,
            cost: false,
            fires: Fires::default(),
            sel: 0,
            scroll: 0,
            page: 10,
            why: None,
            from_log: false,
            bodies: HashMap::new(),
            follow: None,
            transcript: None,
            agents: Agents::default(),
            now: agent_fmt::when::now_secs(),
            table: TableState::default(),
            list: ListState::default(),
            hits: Hits::default(),
        };
        r.follow_newest();
        r
    }

    /// Read a session's frames from the event log `content`. `project`
    /// overrides the project the session recorded, which a live view takes
    /// from where it was launched.
    pub(crate) fn load(content: &str, session_id: &str, project: Option<&str>, live: bool) -> Result<Replay, String> {
        let project = project
            .map(str::to_string)
            .or_else(|| super::frames::find_session_project(content, session_id))
            .unwrap_or_else(|| "unknown".to_string());
        let events = super::frames::load_session_events(content, session_id);
        let short = super::short_id(session_id);
        if events.is_empty() {
            return Err(format!("no events for session {short}"));
        }
        let window = crate::session::detect_context_window_for(&project, session_id);
        let frames = super::frames::reconstruct_all(&events, &project, session_id, window);
        if frames.is_empty() {
            return Err(format!("no frames to replay in session {short}"));
        }
        let play = if live { Playback::live(frames.len()) } else { Playback::replay(frames.len()) };
        let mut r = Replay::new(session_id.to_string(), project, window / 1000, frames, play);
        r.from_log = true;
        r.judged = super::frames::has_verdicts(&events);
        r.spend = session_spend(content, session_id);
        r.fires.set(super::semantic_fires(content, session_id));
        r.transcript = ways_core::paths::claude_dir().find_transcript(Some(&r.project), session_id);
        r.agents = Agents::read(&events, r.transcript.as_deref());
        r.follow = if live {
            Some(Follow::session(r.transcript.clone()))
        } else {
            Follow::waking(r.transcript.clone(), super::live::system_stat(), super::live::system_clock())
        };
        Ok(r)
    }

    /// The session is being written to: a replay goes live, following at
    /// the newest frame and paused on an earlier one, and from the log its
    /// follow states the log and the transcript from the floor and the
    /// frames are read again. The log's text when it was read.
    pub(crate) fn wake(&mut self) -> Option<String> {
        if self.play.is_live() {
            return None;
        }
        self.play.go_live();
        self.follow_newest();
        if !self.from_log {
            self.follow = None;
            return None;
        }
        self.follow = Some(Follow::session(self.transcript.clone()));
        Some(self.refresh())
    }

    fn frame(&self) -> &Frame {
        &self.frames[self.play.pos()]
    }

    /// The frame shown, its ways filtered by the view.
    fn shown(&self) -> Frame {
        self.frame().shown(self.matched)
    }

    /// Widen the view to every matched candidate, or narrow it back to the
    /// injected ways, the cursor kept on its way where it can be.
    fn toggle_matched(&mut self) {
        let anchor = self.anchor();
        self.matched = !self.matched;
        self.sel = anchor.map_or(0, |(id, agent, epoch)| reselect_by_anchor(&self.shown(), &id, &agent, epoch));
        self.scroll = 0;
    }

    /// Frame indexes where a compaction window starts.
    fn window_starts(&self) -> Vec<usize> {
        self.frames.windows(2).enumerate().filter(|(_, p)| p[0].window != p[1].window).map(|(i, _)| i + 1).collect()
    }

    /// Frame indexes where the subagent switch held ways back (#786).
    fn suppressed_at(&self) -> Vec<usize> {
        self.frames.iter().enumerate().filter(|(_, f)| !f.suppressed.is_empty()).map(|(i, _)| i).collect()
    }

    /// How many dispatches and agents the switch held ways back from in
    /// the whole session: the summary's count.
    fn suppressed_total(&self) -> usize {
        self.frames.iter().map(|f| f.suppressed.len()).sum()
    }

    fn ways_len(&self) -> usize {
        self.shown().ways.len()
    }

    /// The selected way's id, agent and the epoch it fired at, carried
    /// across a frame change so the cursor stays on the same row.
    fn anchor(&self) -> Option<(String, String, u64)> {
        self.shown().ways.get(self.sel.min(self.ways_len().saturating_sub(1))).map(|w| (w.id.clone(), w.agent.clone(), w.epoch_fired))
    }

    /// Move along the timeline with `go`, keeping the selection on the
    /// same way, or the nearest that fired at or before it. Resuming a live
    /// follow puts the cursor back on the newest way.
    fn travel(&mut self, go: impl FnOnce(&mut Playback)) {
        let anchor = self.anchor();
        go(&mut self.play);
        self.sel = match anchor {
            Some((id, agent, epoch)) => reselect_by_anchor(&self.shown(), &id, &agent, epoch),
            None => 0,
        };
        self.follow_newest();
        self.scroll = 0;
    }

    /// While a live timeline follows, the cursor rides the newest way, so
    /// each way injected as the session runs scrolls into view. Reviewing
    /// history (a step back, the cursor moved up, the why-fired reader)
    /// leaves the cursor where it is.
    fn follow_newest(&mut self) {
        if self.view == View::Timeline && self.play.is_live() && self.play.following() {
            self.sel = self.ways_len().saturating_sub(1);
        }
    }

    /// Moving the cursor up a live timeline is reviewing it: the follow
    /// stops on the frame shown, and space or End resumes it.
    fn stop_following(&mut self) {
        if self.play.is_live() && self.play.following() {
            let at = self.play.pos();
            self.play.go(at);
        }
    }

    fn open_why(&mut self) {
        self.view = View::Why;
        self.play.pause();
        self.scroll = 0;
        if self.why.is_none() {
            self.load_why();
        }
    }

    /// Read the why index from the event log. It leaves the view, the
    /// playback and the reader's place alone.
    fn load_why(&mut self) {
        if self.from_log {
            let model = SessionIntrospection::from_session(&self.session_id, &self.project, self.window_k);
            self.why = Some(why::build_why_index(&model));
        }
    }

    /// Whether the why-fired reader is open, where Esc goes back to the
    /// timeline rather than to the sessions.
    fn reading(&self) -> bool {
        self.view == View::Why
    }

    /// The bottom bar's lozenge and its ground: what the replay is doing.
    fn mode(&self) -> (&'static str, Ground) {
        match (self.view, self.play.is_live(), self.play.following(), self.play.playing()) {
            (View::Why, ..) => ("why", Ground::Accent),
            (_, true, true, _) => ("live", Ground::Ok),
            (_, true, false, _) => ("paused", Ground::Warn),
            (_, false, _, true) => ("playing", Ground::Ok),
            _ => ("replay", Ground::Accent),
        }
    }

    /// The replay's keys, as its bar showed them, then the rest for the
    /// key help.
    fn bindings(&self) -> Vec<Binding> {
        let mut keys = match self.view {
            View::Timeline => vec![Binding::new("↑↓", "select"), Binding::new("⏎", "why"), Binding::new("←→", "frame")],
            View::Why => vec![Binding::new("↑↓", "way"), Binding::new("j/k", "read"), Binding::new("←→", "frame")],
        };
        if self.view == View::Timeline {
            if self.play.is_live() {
                keys.push(Binding::new("space", if self.play.following() { "pause" } else { "follow" }));
            } else {
                keys.push(Binding::new("space", if self.play.playing() { "pause" } else { "play" }));
                keys.push(Binding::new("+-", self.play.speed_label()));
            }
        } else {
            keys.push(Binding::new("esc", "timeline"));
        }
        keys.push(Binding::new("f", if self.matched { "injected" } else { "matched" }));
        if self.spend.is_some() {
            keys.push(Binding::new("$", if self.cost { "tokens" } else { "cost" }));
        }
        match self.view {
            View::Timeline => keys.extend([
                Binding::help("Tab", "why the selected way fired"),
                Binding::help("Home End", "the first frame, the newest (live: follow again)"),
                Binding::help("PgUp PgDn", "move the selection by ten"),
                Binding::help("click the track", "seeks to that frame; the wheel there steps one"),
                Binding::help("click a row", "selects it; a click on the selected row opens why it fired"),
            ]),
            View::Why => keys.extend([
                Binding::help("PgUp PgDn g G", "read by a page, to the top, to the end"),
                Binding::help("click a way", "shows why it fired; the wheel over the text reads on"),
            ]),
        }
        keys
    }

    /// A click: on the track, seek to the frame drawn there, the newest
    /// following again as End does; on a row, select it, and on the
    /// selected row open why it fired.
    fn click(&mut self, at: Position) {
        if self.hits.scrub.contains(at) {
            let marks = (self.play.len(), self.play.pos());
            let s = Scrubber { len: marks.0, pos: marks.1, marks: &[], notes: &[] };
            if let Some(to) = s.frame_at(self.hits.scrub.width, at.x - self.hits.scrub.x) {
                if to + 1 >= self.play.len() {
                    self.travel(Playback::end);
                } else {
                    self.travel(|p| p.go(to));
                }
            }
            return;
        }
        let hit = match self.view {
            View::Timeline => agent_tui::hit::row_in(self.hits.rows, at, 1, self.table.offset(), &self.hits.heights),
            View::Why => agent_tui::hit::row_at(self.hits.rows, at, 0, self.list.offset()),
        };
        let Some(i) = hit else { return };
        if i >= self.ways_len() {
            return;
        }
        if i < self.sel {
            self.stop_following();
        }
        let mut sel = self.sel;
        let again = agent_tui::hit::pick(&mut sel, i, self.ways_len());
        if sel != self.sel {
            self.sel = sel;
            self.scroll = 0;
        }
        if again && self.view == View::Timeline {
            self.open_why();
        }
    }

    /// The wheel: on the track a frame back or on; over the why-fired text
    /// a line; elsewhere the selection, as the arrows move it.
    fn wheel(&mut self, up: bool, at: Position) {
        let press = |c| KeyEvent::new(c, KeyModifiers::NONE);
        let key = if self.hits.scrub.contains(at) {
            if up { KeyCode::Left } else { KeyCode::Right }
        } else if self.view == View::Why && self.hits.text.contains(at) {
            if up { KeyCode::Char('k') } else { KeyCode::Char('j') }
        } else if up {
            KeyCode::Up
        } else {
            KeyCode::Down
        };
        self.key(press(key));
    }

    fn key(&mut self, k: KeyEvent) -> Step {
        let n = self.ways_len();
        match (self.view, k.code) {
            (_, KeyCode::Char('q')) => return Step::Quit,
            (View::Timeline, KeyCode::Esc) => return Step::Back,
            (View::Why, KeyCode::Esc | KeyCode::Tab) => {
                self.view = View::Timeline;
                self.scroll = 0;
                self.follow_newest();
            }
            (View::Timeline, KeyCode::Tab) => self.open_why(),
            (View::Timeline, KeyCode::Enter) if n > 0 => self.open_why(),
            (View::Timeline, KeyCode::Up | KeyCode::Char('k')) | (View::Why, KeyCode::Up) => {
                self.stop_following();
                self.sel = self.sel.saturating_sub(1);
                self.scroll = 0;
            }
            (View::Timeline, KeyCode::Down | KeyCode::Char('j')) | (View::Why, KeyCode::Down) => {
                self.sel = (self.sel + 1).min(n.saturating_sub(1));
                self.scroll = 0;
            }
            (View::Timeline, KeyCode::PageUp) => {
                self.stop_following();
                self.sel = self.sel.saturating_sub(10);
            }
            (View::Timeline, KeyCode::PageDown) => self.sel = (self.sel + 10).min(n.saturating_sub(1)),
            (View::Why, KeyCode::Char('j')) => self.scroll = self.scroll.saturating_add(1),
            (View::Why, KeyCode::Char('k')) => self.scroll = self.scroll.saturating_sub(1),
            (View::Why, KeyCode::PageDown) => self.scroll = self.scroll.saturating_add(self.page),
            (View::Why, KeyCode::PageUp) => self.scroll = self.scroll.saturating_sub(self.page),
            (View::Why, KeyCode::Home | KeyCode::Char('g')) => self.scroll = 0,
            (View::Why, KeyCode::End | KeyCode::Char('G')) => self.scroll = usize::MAX,
            (_, KeyCode::Right | KeyCode::Char('l')) => self.travel(|p| p.step(true)),
            (_, KeyCode::Left | KeyCode::Char('h')) => self.travel(|p| p.step(false)),
            (View::Timeline, KeyCode::Home | KeyCode::Char('g')) => self.travel(Playback::home),
            (View::Timeline, KeyCode::End | KeyCode::Char('G')) => self.travel(Playback::end),
            (View::Timeline, KeyCode::Char(' ')) => self.travel(Playback::toggle),
            (_, KeyCode::Char('f')) => self.toggle_matched(),
            (_, KeyCode::Char('$')) if self.spend.is_some() => self.cost = !self.cost,
            (View::Timeline, KeyCode::Char('+') | KeyCode::Char('=')) => self.play.faster(),
            (View::Timeline, KeyCode::Char('-') | KeyCode::Char('_')) => self.play.slower(),
            _ => {}
        }
        Step::Stay
    }

    /// How often the replay wants the screen's tick: live, for its
    /// follow's stats; playing on its timeline, a frame time; quiet with a
    /// watch on its transcript, for the watch's stats.
    fn tick_every(&self, on_timeline: bool) -> Option<Duration> {
        if self.play.is_live() {
            // The follow's stat is due on its backoff; the tick asks for it.
            self.follow.as_ref().is_none_or(Follow::watching).then_some(SAMPLE_TICK)
        } else if on_timeline && self.play.playing() {
            // The watch is polled at the frame time: its stats are seldom due.
            Some(self.play.frame_time())
        } else {
            self.follow.as_ref().filter(|f| f.watching()).map(|_| SAMPLE_TICK)
        }
    }

    /// Whether the replay asks for a tick on some tab.
    fn ticking(&self) -> bool {
        self.play.is_live() || self.follow.is_some()
    }

    /// Play a frame on, or, live, state the event log when its stat is
    /// due and read it again when it was written. The event log's text when
    /// it was read.
    /// `advance`: the timeline is shown, so a playing replay moves on.
    fn tick(&mut self, advance: bool) -> Option<String> {
        if self.play.is_live() {
            self.now = agent_fmt::when::now_secs();
            let wrote = self.follow.as_mut().is_some_and(Follow::poll);
            return (wrote && self.from_log).then(|| self.refresh());
        }
        if advance && self.play.playing() {
            self.travel(|p| {
                p.tick();
            });
        }
        if self.follow.as_mut().is_some_and(Follow::poll) {
            self.now = agent_fmt::when::now_secs();
            return self.wake();
        }
        None
    }

    /// Read the live session again, the event log having been written:
    /// new frames, the newest shown while following, and the why index
    /// read afresh. The log's text.
    fn refresh(&mut self) -> String {
        // At launch the transcript may hold no model turn yet, so the window
        // read then is the 200K default; a later read corrects it. Only an
        // upgrade is taken: a real window never shrinks mid-session.
        let detected = crate::session::detect_context_window_for(&self.project, &self.session_id) / 1000;
        self.window_k = self.window_k.max(detected);
        let content = ways_core::firing::load_events_text();
        let events = super::frames::load_session_events(&content, &self.session_id);
        let frames = super::frames::reconstruct_all(&events, &self.project, &self.session_id, self.window_k * 1000);
        self.spend = session_spend(&content, &self.session_id);
        self.fires.set(super::semantic_fires(&content, &self.session_id));
        if !frames.is_empty() {
            self.judged = super::frames::has_verdicts(&events);
            self.agents = Agents::read(&events, self.transcript.as_deref());
            self.take_frames(frames);
        }
        content
    }

    /// The frames read again: the newest shown while following, the
    /// selection kept on its way, and the why index read afresh.
    pub(crate) fn take_frames(&mut self, frames: Vec<Frame>) {
        let anchor = self.anchor();
        self.frames = frames;
        self.play.resize(self.frames.len());
        self.sel = anchor.map_or(0, |(id, agent, epoch)| reselect_by_anchor(&self.shown(), &id, &agent, epoch));
        self.follow_newest();
        // The why index is read again in place: the reader keeps its scroll.
        // Out of the view it is dropped and read when the view opens.
        if self.why.is_some() && self.from_log {
            if self.view == View::Why {
                self.load_why();
            } else {
                self.why = None;
            }
        }
    }
}

/// The judge's spend on `session` in the event log `content`; `None`
/// while it made no call.
pub(super) fn session_spend(content: &str, session: &str) -> Option<Group> {
    let calls = spend::filter(spend::parse_log(content), None, Some(session));
    (!calls.is_empty()).then(|| spend::total(&calls))
}

/// The row in `frame` that best keeps an anchor across a frame change: the
/// same row, by id, agent and epoch, which tells a way's blocked row from
/// its active one; else the same way and agent; else the same way; else the nearest active way that fired
/// at or before the anchor's epoch (the ways are in epoch order, so the
/// last such row), else the first row. Epochs restart at each compaction
/// window, so across one the id match does the work and the fallback only
/// places the cursor.
pub(super) fn reselect_by_anchor(frame: &Frame, anchor_id: &str, anchor_agent: &str, anchor_epoch: u64) -> usize {
    let same = |w: &&ActiveWay| w.id == anchor_id;
    let mine = |w: &&ActiveWay| same(w) && w.agent == anchor_agent;
    if let Some(i) = frame
        .ways
        .iter()
        .position(|w| mine(&w) && w.epoch_fired == anchor_epoch)
        .or_else(|| frame.ways.iter().position(|w| mine(&w)))
        .or_else(|| frame.ways.iter().position(|w| same(&w)))
    {
        return i;
    }
    frame.ways.iter().rposition(|w| w.epoch_fired <= anchor_epoch).unwrap_or(0)
}

/// Opens a session the picker chose; following it when the flag says it
/// is live.
pub(crate) type Opener = Box<dyn Fn(&str, bool) -> Result<Replay, String>>;

/// The tabs of the screen, in the order a digit picks them.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Tab {
    Sessions,
    Timeline,
    Fires,
    Spend,
    Stats,
    Precision,
}

impl Tab {
    fn name(self) -> &'static str {
        match self {
            Tab::Sessions => "sessions",
            Tab::Timeline => "timeline",
            Tab::Fires => "fires",
            Tab::Spend => "spend",
            Tab::Stats => "stats",
            Tab::Precision => "precision",
        }
    }
}

/// The introspect screens as a pane of agent-tui's shell (ADR-504 §3):
/// the picker when there is one, the session it opened, and the reports
/// over the scope. The shell draws the tabs and the bottom bar, and takes
/// the mouse, the key help and quitting.
pub(crate) struct Sessions {
    palette: Palette,
    pub(crate) tab: Tab,
    picker: Option<Picker>,
    pub(crate) replay: Option<Replay>,
    pub(crate) reports: Reports,
    open: Opener,
    msg: String,
}

impl Sessions {
    #[cfg(test)]
    pub(crate) fn picker_mut(&mut self) -> Option<&mut Picker> {
        self.picker.as_mut()
    }

    /// Whether `r` takes the screen's tick on the tab shown: a replay
    /// plays on its timeline; a live one follows, and a quiet one watches
    /// its transcript, from every tab.
    fn ticks(&self, r: &Replay) -> bool {
        self.tab == Tab::Timeline || r.ticking()
    }

    /// The tabs shown: the sessions tab only with a picker.
    fn shown_tabs(&self) -> Vec<Tab> {
        let all = [Tab::Sessions, Tab::Timeline, Tab::Fires, Tab::Spend, Tab::Stats, Tab::Precision];
        all.into_iter().filter(|t| *t != Tab::Sessions || self.picker.is_some()).collect()
    }

    /// Esc on a tab: back to the picker, or out when there is none.
    fn back(&mut self) -> bool {
        if self.picker.is_some() && self.tab != Tab::Sessions {
            self.tab = Tab::Sessions;
            true
        } else {
            false
        }
    }

    /// Open the session selected in the picker on the timeline, as Enter
    /// and a click on the selected row do.
    fn open_selected(&mut self) {
        let Some(p) = &mut self.picker else { return };
        let Some(id) = p.sessions.get(p.sel).map(|s| s.id.clone()) else { return };
        // The session already open is shown as it was left, live now if
        // the list says it is being written to.
        if let Some(r) = self.replay.as_mut().filter(|r| r.session_id == id) {
            if p.selected_live() {
                if let Some(content) = r.wake() {
                    self.reports.reload(&content);
                }
            }
            self.tab = Tab::Timeline;
            return;
        }
        match (self.open)(&id, p.selected_live()) {
            Ok(r) => {
                self.replay = Some(r);
                self.tab = Tab::Timeline;
                self.msg.clear();
            }
            Err(e) => self.msg = e,
        }
    }

    /// A key on a list tab other than the picker's.
    fn list_key(&mut self, c: KeyCode) {
        match (self.tab, &mut self.replay) {
            (Tab::Spend, _) => self.reports.spend().key(c),
            (Tab::Stats, _) => self.reports.stats().key(c),
            (Tab::Precision, _) => self.reports.precision().key(c),
            (Tab::Fires, Some(r)) => r.fires.key(c),
            _ => {}
        }
    }
}

impl Pane for Sessions {
    fn palette(&self) -> Palette {
        self.palette
    }

    fn tabs(&mut self) -> Vec<PaneTab> {
        self.shown_tabs().into_iter().map(|t| PaneTab::new(t.name())).collect()
    }

    fn tab(&mut self) -> usize {
        self.shown_tabs().iter().position(|t| *t == self.tab).unwrap_or(0)
    }

    fn set_tab(&mut self, i: usize) {
        if let Some(t) = self.shown_tabs().get(i) {
            self.tab = *t;
        }
    }

    fn draw(&mut self, f: &mut Draw, area: Rect) {
        match (self.tab, &mut self.replay, &mut self.picker) {
            (Tab::Sessions, _, Some(p)) => draw_picker(f, p, area),
            (Tab::Timeline, Some(r), _) => draw_replay(f, r, area),
            (Tab::Fires, Some(r), _) => r.fires.draw(f, &r.session_id, area),
            (Tab::Spend, ..) => self.reports.spend().draw(f, area),
            (Tab::Stats, ..) => self.reports.stats().draw(f, area),
            (Tab::Precision, ..) => self.reports.precision().draw(f, area),
            // A session tab before a session is open.
            (tab, ..) => {
                let hint = Line::styled("no session open: pick one on the sessions tab", theme::muted());
                f.render_widget(Paragraph::new(hint).block(pane(format!(" {} ", tab.name()))), area);
            }
        }
    }

    /// The keys the shell leaves: a digit, `q`, `?` and `m` are its own,
    /// and Esc it takes when the pane passes it, as on the sessions tab or
    /// a session opened without one.
    fn key(&mut self, k: KeyEvent) -> Keyed {
        let done = |on: bool| if on { Keyed::Done } else { Keyed::Pass };
        match (self.tab, &mut self.replay, &mut self.picker) {
            (Tab::Sessions, _, Some(p)) => match k.code {
                KeyCode::Esc => Keyed::Pass,
                KeyCode::Enter => {
                    self.open_selected();
                    Keyed::Done
                }
                c => {
                    p.key(c);
                    Keyed::Done
                }
            },
            (Tab::Timeline, Some(r), _) => match r.key(k) {
                Step::Stay => Keyed::Done,
                Step::Quit => Keyed::Pass,
                Step::Back => done(self.back()),
            },
            (..) => match k.code {
                KeyCode::Esc => done(self.back()),
                c => {
                    self.list_key(c);
                    Keyed::Done
                }
            },
        }
    }

    /// The wheel moves a list's selection as the arrows do; on the
    /// timeline's track it steps a frame, and over the why-fired text it
    /// reads on.
    fn wheel_at(&mut self, up: bool, at: Position) {
        let arrow = if up { KeyCode::Up } else { KeyCode::Down };
        match (self.tab, &mut self.replay, &mut self.picker) {
            (Tab::Sessions, _, Some(p)) => p.key(arrow),
            (Tab::Timeline, Some(r), _) => r.wheel(up, at),
            _ => self.list_key(arrow),
        }
    }

    /// A click on a row selects it, and on the selected row acts as Enter:
    /// the picker opens the session, the timeline its why-fired page. A
    /// click on the timeline's track seeks to the frame drawn there.
    fn click(&mut self, at: Position) {
        match (self.tab, &mut self.replay, &mut self.picker) {
            (Tab::Sessions, _, Some(p)) => {
                if p.click(at) {
                    self.open_selected();
                }
            }
            (Tab::Timeline, Some(r), _) => r.click(at),
            (Tab::Fires, Some(r), _) => r.fires.click(at),
            (Tab::Spend, ..) => self.reports.spend().click(at),
            (Tab::Stats, ..) => self.reports.stats().click(at),
            (Tab::Precision, ..) => self.reports.precision().click(at),
            _ => {}
        }
    }

    fn bindings(&self) -> Vec<Binding> {
        let back = self.picker.is_some();
        let mut out = match (self.tab, &self.replay, &self.picker) {
            (Tab::Sessions, _, Some(p)) => {
                let enter = if p.selected_live() { "follow" } else { "replay" };
                vec![
                    Binding::new("↑↓", "select"),
                    Binding::new("⏎", enter),
                    Binding::help("PgUp PgDn Home End", "move by a page or to an end"),
                    Binding::help("click a row", "selects it; a click on the selected row opens it"),
                ]
            }
            (Tab::Timeline, Some(r), _) => r.bindings(),
            (Tab::Spend, ..) => {
                let by = if self.reports.by_day() { "by month" } else { "by day" };
                vec![Binding::new("↑↓", "select"), Binding::new("d", by), Binding::help("click a row", "selects it")]
            }
            (Tab::Fires | Tab::Stats | Tab::Precision, ..) => vec![Binding::new("↑↓", "select"), Binding::help("click a row", "selects it")],
            _ => Vec::new(),
        };
        let leaves = !(self.tab == Tab::Sessions || (self.tab == Tab::Timeline && self.replay.as_ref().is_some_and(Replay::reading)));
        if back && leaves {
            out.push(Binding::new("esc", "sessions"));
        }
        out
    }

    fn mode(&self) -> String {
        match (self.tab, &self.replay, &self.picker) {
            (Tab::Sessions, _, Some(_)) => "pick".into(),
            (Tab::Timeline, Some(r), _) => r.mode().0.into(),
            (tab, ..) => tab.name().into(),
        }
    }

    fn mode_ground(&self) -> Ground {
        match (self.tab, &self.replay) {
            (Tab::Timeline, Some(r)) => r.mode().1,
            _ => Ground::Accent,
        }
    }

    /// The picker's error, or where its selection is and when its session
    /// was last written; the fires tab's place in its list.
    fn status(&mut self) -> Option<(String, Tone)> {
        match (self.tab, &self.replay, &self.picker) {
            (Tab::Sessions, _, Some(_)) if !self.msg.is_empty() => Some((self.msg.clone(), Tone::Err)),
            (Tab::Sessions, _, Some(p)) => Some((picker_status(p), Tone::Back)),
            (Tab::Fires, Some(r), _) => Some((r.fires.place(), Tone::Back)),
            _ => None,
        }
    }

    fn help(&self) -> Option<String> {
        Some(
            "Sessions lists the sessions in scope; Enter opens one on the timeline,\n\
             following it when it is being written to. On the timeline ←→ step a\n\
             frame, space plays or follows, Enter or Tab opens why the selected way\n\
             fired, and f widens the table to every matched way. A way's mark and\n\
             colour say what happened to it: green fired, ↩ re-disclosed, blue its\n\
             check fired, ◌ shadow-judged, ⊘ judged out, ◷ held by its refire\n\
             window, ⊟ over the context cap; bold is this frame. A click on the\n\
             track seeks there. `ways session replay --json` prints the timeline."
                .into(),
        )
    }

    /// A replay plays only while its timeline is shown; a live one reads
    /// the log on every tab, so the fires and spend tabs keep up. The list
    /// states its transcripts only while it is shown.
    fn tick_every(&self) -> Option<Duration> {
        let replay = self.replay.as_ref().filter(|r| self.ticks(r)).and_then(|r| r.tick_every(self.tab == Tab::Timeline));
        let list = self.picker.as_ref().filter(|_| self.tab == Tab::Sessions).and_then(Picker::tick_every);
        replay.into_iter().chain(list).min()
    }

    fn tick(&mut self) {
        if self.tab == Tab::Sessions {
            if let Some(p) = &mut self.picker {
                p.tick();
            }
        }
        if !self.replay.as_ref().is_some_and(|r| self.ticks(r)) {
            return;
        }
        let advance = self.tab == Tab::Timeline;
        if let Some(content) = self.replay.as_mut().and_then(|r| r.tick(advance)) {
            self.reports.reload(&content);
        }
    }
}

/// The picker's message: when the selected session's transcript was last
/// written, live or not, and where the selection is.
fn picker_status(p: &Picker) -> String {
    let place = format!("{}/{}", (p.sel + 1).min(p.sessions.len()), p.sessions.len());
    match p.sampler.last_write(p.sel) {
        Some(at) => {
            let ago = agent_fmt::when::ago(p.sampler.now().saturating_sub(at) / 1000);
            if p.sampler.live(p.sel) {
                format!("{} live · written {ago} · {place}", super::picker::LIVE_MARK)
            } else {
                format!("written {ago} · {place}")
            }
        }
        None => place,
    }
}

/// The session screen on the shell: [`Sessions`] inside `agent_tui::App`.
/// The terminal runs [`Introspect::into_app`]; the headless `--snap` and
/// the tests drive the same shell as a [`Screen`]; [`Introspect::pane`]
/// reaches the screen's state.
pub(crate) struct Introspect {
    app: App,
}

impl Introspect {
    fn of(pane: Sessions, shape: Shape) -> Introspect {
        Introspect { app: App::with_pane("session", pane).shape(shape) }
    }

    pub(crate) fn picking(picker: Picker, open: Opener, reports: Reports, palette: Palette, shape: Shape) -> Introspect {
        Introspect::of(Sessions { palette, tab: Tab::Sessions, picker: Some(picker), replay: None, reports, open, msg: String::new() }, shape)
    }

    /// The picker with `replay` open on the timeline, as `ways session live`
    /// opens it: Esc goes back to the list.
    pub(crate) fn opened(mut self, replay: Replay) -> Introspect {
        let p = self.pane_mut();
        p.replay = Some(replay);
        p.tab = Tab::Timeline;
        self
    }

    /// The screen's state: the pane the shell hosts, which it never swaps.
    pub(crate) fn pane(&self) -> &Sessions {
        self.app.pane_ref().expect("the session screen's pane")
    }

    pub(crate) fn pane_mut(&mut self) -> &mut Sessions {
        self.app.pane_mut().expect("the session screen's pane")
    }

    pub(crate) fn showing(replay: Replay, reports: Reports, palette: Palette, shape: Shape) -> Introspect {
        let open = Box::new(|_: &str, _: bool| Err("no picker".into()));
        Introspect::of(Sessions { palette, tab: Tab::Timeline, picker: None, replay: Some(replay), reports, open, msg: String::new() }, shape)
    }

    /// The shell, to run on the terminal.
    pub(crate) fn into_app(self) -> App {
        self.app
    }

    /// The shell, as it stands.
    #[cfg(test)]
    pub(crate) fn app(&self) -> &App {
        &self.app
    }
}

impl Screen for Introspect {
    fn palette(&self) -> Palette {
        self.pane().palette
    }

    fn draw(&mut self, f: &mut Draw) {
        Screen::draw(&mut self.app, f);
    }

    fn key(&mut self, k: KeyEvent) -> bool {
        Screen::key(&mut self.app, k)
    }

    fn mouse(&mut self, m: MouseEvent) {
        Screen::mouse(&mut self.app, m);
    }

    fn tick_every(&self) -> Option<Duration> {
        Screen::tick_every(&self.app)
    }

    fn tick(&mut self) {
        Screen::tick(&mut self.app);
    }
}

/// `2026-07-03T16:52:00Z` as `2026-07-03 16:52`.
fn friendly_ts(ts: &str) -> String {
    let spaced = ts.replace('T', " ");
    spaced.get(..16).unwrap_or(&spaced).to_string()
}

/// The two header lines: the session and its project, then where the
/// frame shown sits, which ways the table holds, and for a live session
/// whether it follows. When the line is wider than `width`, the timestamp
/// goes first, then the session's count of suppressions, then the count of
/// ways judged out.
fn header(r: &Replay, width: u16) -> Vec<Line<'static>> {
    let fr = r.frame();
    let windows = r.frames.last().map_or(1, |l| l.window);
    // Each span with the order it is dropped in for width; 0 stays.
    let mut metrics: Vec<(u8, Span<'static>)> = vec![
        (
            0,
            Span::styled(
                format!("epoch {} · {}K ctx · {} ways · window {}/{}", fr.epoch, r.window_k, fr.shown(r.matched).ways.len(), fr.window, windows),
                theme::muted(),
            ),
        ),
        (1, Span::styled(format!(" · {}", friendly_ts(&fr.timestamp)), theme::muted())),
    ];
    // The filter, named as the follow state is: which ways the table holds.
    // A session the judge never saw is all injected, and keeps the room.
    if r.matched {
        metrics.push((0, Span::styled("  ◆ matched", theme::accent().add_modifier(Modifier::BOLD))));
    } else if r.judged {
        metrics.push((0, Span::styled("  ◇ injected", theme::accent())));
        let blocked = fr.blocked();
        if blocked > 0 {
            metrics.push((2, Span::styled(format!(" · {blocked} judged out"), theme::muted())));
        }
    }
    // The session's suppressions, counted as the JSON summary counts them.
    let suppressed = r.suppressed_total();
    if suppressed > 0 {
        metrics.push((3, Span::styled(format!(" · ⊝ {suppressed} suppressed"), theme::warn())));
    }
    if r.play.is_live() {
        if r.play.following() {
            metrics.push((0, Span::styled("  ● LIVE", theme::ok().add_modifier(Modifier::BOLD))));
            if let Some(then) = agent_fmt::when::parse_utc_iso(&fr.timestamp) {
                metrics.push((0, Span::styled(format!(" · {}", agent_fmt::when::ago(r.now.saturating_sub(then))), theme::muted())));
            }
        } else {
            metrics.push((0, Span::styled("  ● LIVE paused", theme::warn().add_modifier(Modifier::BOLD))));
        }
    }
    for drop in [1u8, 3, 2] {
        if metrics.iter().map(|(_, s)| s.width()).sum::<usize>() <= width as usize {
            break;
        }
        metrics.retain(|(p, _)| *p != drop);
    }
    let metrics: Vec<Span<'static>> = metrics.into_iter().map(|(_, s)| s).collect();
    // The spend goes before the project, so a narrow line cuts the path;
    // when the spend itself would not fit, the session id is shortened.
    let spend = r.spend.as_ref().map(|g| {
        let amount = if r.cost { g.cost_short() } else { format!("{} tokens", g.tokens_short()) };
        Span::styled(format!("  judge ×{} · {amount}", g.calls), theme::accent())
    });
    let mut id = r.session_id.clone();
    if "Session ".len() + id.chars().count() + spend.as_ref().map_or(0, Span::width) > width as usize {
        id = super::short_id(&id);
    }
    let mut title = vec![Span::styled("Session ", Style::new().add_modifier(Modifier::BOLD)), Span::raw(id)];
    title.extend(spend);
    title.push(Span::styled(format!("  {}", r.project), theme::muted()));
    vec![
        Line::from(title),
        Line::from(metrics),
    ]
}

fn draw_replay(f: &mut Draw, r: &mut Replay, area: Rect) {
    let [head, scrub, body] = Layout::vertical([Constraint::Length(2), Constraint::Length(1), Constraint::Min(3)]).areas(area);
    let n = r.ways_len();
    r.sel = r.sel.min(n.saturating_sub(1));
    f.render_widget(Paragraph::new(header(r, head.width)), head);
    let marks = r.window_starts();
    let notes = r.suppressed_at();
    f.render_widget(Scrubber { len: r.play.len(), pos: r.play.pos(), marks: &marks, notes: &notes }, scrub);
    r.hits = Hits { scrub, ..Hits::default() };
    match r.view {
        View::Timeline => draw_timeline(f, r, body),
        View::Why => draw_why(f, r, body),
    }
}

fn draw_timeline(f: &mut Draw, r: &mut Replay, area: Rect) {
    let inner_w = area.width.saturating_sub(2) as usize;
    let all = &r.frames[r.play.pos()];
    // The gauge, zones and forecast count what was injected, in either view.
    let fr = &all.shown(false);
    let mut ctx = table::context(fr, r.window_k, inner_w);
    // Keep what fits with at least six rows for the table: the forecast
    // goes first, then the zones, then the gauge.
    for drop in [3u8, 1, 0, 2] {
        if area.height as usize >= ctx.len() + 2 + 6 || ctx.is_empty() {
            break;
        }
        ctx.retain(|(p, _)| *p != drop);
    }
    let ctx_h = if ctx.is_empty() { 0 } else { ctx.len() as u16 + 2 };
    let [ways, context] = Layout::vertical([Constraint::Min(3), Constraint::Length(ctx_h)]).areas(area);
    let title = format!(" ways at epoch {} ", fr.epoch);
    let shown = all.shown(r.matched);
    if shown.ways.is_empty() {
        f.render_widget(Paragraph::new(Line::styled("no ways fired yet", theme::muted())).block(pane(title)), ways);
    } else {
        // The injected rows, then, in the matched view, the rows the judge,
        // the refire window or the context cap kept out: the order
        // `Frame::ways` keeps.
        let agent_w = table::agent_width(&shown.ways, &r.agents, inner_w);
        let mut rows = table::rows(fr, &r.agents, agent_w, r.window_k, inner_w);
        let way_w = table::way_width(inner_w, agent_w);
        let withheld = || shown.ways.iter().filter(|w| !w.outcome.injected());
        // A way whose check fired takes a second line.
        let heights: Vec<u16> = fr.ways.iter().map(|w| 1 + u16::from(w.check_fires > 0)).chain(withheld().map(|_| 1)).collect();
        rows.extend(withheld().map(|w| withheld_row(w, &r.agents, agent_w, way_w, shown.epoch)));
        let t = Table::new(rows, table::widths(agent_w))
            .header(table::header())
            .column_spacing(2)
            .block(pane(title))
            .row_highlight_style(theme::selected())
            .highlight_symbol(Line::styled(theme::SELECTED_MARK, theme::accent()))
            .highlight_spacing(HighlightSpacing::Always);
        // The scroll is kept between frames, so a row stays under the
        // pointer for a second click, but never past where the last rows
        // fill the view: a frame with fewer ways never hides its first.
        let view = ways.height.saturating_sub(3) as usize;
        let (mut fill, mut max_off) = (0usize, heights.len());
        for (i, h) in heights.iter().enumerate().rev() {
            fill += *h as usize;
            if fill > view {
                break;
            }
            max_off = i;
        }
        let kept = r.table.offset().min(max_off);
        *r.table.offset_mut() = kept;
        r.table.select(Some(r.sel));
        f.render_stateful_widget(t, ways, &mut r.table);
        r.hits.rows = ways;
        r.hits.heights = heights;
    }
    if ctx_h > 0 {
        let lines: Vec<Line> = ctx.into_iter().map(|(_, l)| l).collect();
        f.render_widget(Paragraph::new(lines).block(pane(" context ")), context);
    }
}

/// A candidate kept out in this frame, in the table's columns: its name
/// marked and coloured by what kept it out, its trigger (a judged way's
/// P(yes)), and nothing to re-disclose, for it injected nothing. A way
/// blocked with its ancestor names it.
fn withheld_row(w: &ActiveWay, agents: &Agents, agent_w: u16, way_w: usize, epoch: u64) -> Row<'static> {
    let muted = |t: String| Span::styled(t, theme::muted());
    let right = |t: String| Cell::from(Line::from(muted(t)).alignment(Alignment::Right));
    let with = if w.ancestor.is_empty() { String::new() } else { format!(" (with {})", w.ancestor) };
    let trigger = if w.outcome == Outcome::Blocked { format!("{} {}", w.trigger, w.p_yes) } else { render::format_trigger(&w.trigger) };
    Row::new(vec![
        Cell::from(Line::from(vec![table::way_name(w, way_w), muted(with)])),
        Cell::from(table::agent_cell(agents, &w.agent, agent_w)),
        right(w.epoch_fired.to_string()),
        right(epoch.saturating_sub(w.epoch_fired).to_string()),
        Cell::from(muted(trigger)),
        Cell::from(" "),
        Cell::from(muted("not injected".into())),
    ])
}

fn draw_why(f: &mut Draw, r: &mut Replay, area: Rect) {
    let left_w = (area.width / 3).clamp(16, 40).min(area.width.saturating_sub(14));
    let [left, right] = Layout::horizontal([Constraint::Length(left_w), Constraint::Min(10)]).areas(area);
    let fr = &r.frames[r.play.pos()].shown(r.matched);
    let ew = fr.ways.iter().map(|w| w.epoch_fired).max().unwrap_or(0).to_string().len();
    let facet = |id: &str, trigger: &str| r.why.as_ref().and_then(|ix| ix.get(&(id.to_string(), trigger.to_string())));
    let items: Vec<ListItem> = fr
        .ways
        .iter()
        .map(|w| {
            // A filled bullet marks a way the model has a record of on this
            // channel; a judge-blocked way's channel is `judge`.
            let bullet = if facet(&w.id, &w.trigger).is_some() { "•" } else { "·" };
            ListItem::new(Line::from(vec![
                Span::raw(format!("{bullet} ")),
                Span::styled(format!("e{:>ew$} ", w.epoch_fired), theme::muted()),
                table::way_name(w, usize::MAX),
            ]))
        })
        .collect();
    let list = List::new(items)
        .block(pane(" ways · epoch "))
        .highlight_style(theme::selected())
        .highlight_symbol(Line::styled(theme::SELECTED_MARK, theme::accent()))
        .highlight_spacing(HighlightSpacing::Always);
    // Kept between frames, as the table's is, and clamped so the last ways
    // fill the view.
    let kept = r.list.offset().min(fr.ways.len().saturating_sub(left.height.saturating_sub(2) as usize));
    *r.list.offset_mut() = kept;
    r.list.select(if fr.ways.is_empty() { None } else { Some(r.sel) });

    let text_w = right.width.saturating_sub(2);
    let mut lines: Vec<Line> = match fr.ways.get(r.sel) {
        None => vec![Line::styled("no ways fired in this frame", theme::muted())],
        Some(w) if r.why.is_none() => vec![Line::styled(w.id.clone(), Style::new().add_modifier(Modifier::BOLD)), Line::styled("no introspection model for this session", theme::muted())],
        Some(w) => {
            let entry = facet(&w.id, &w.trigger);
            let path = entry.and_then(|e| e.way_path.clone());
            let body = path.and_then(|p| r.bodies.entry(p.clone()).or_insert_with(|| why::read_way_body(&p)).clone());
            why::detail_lines(&w.id, entry, body.as_deref(), text_w).iter().flat_map(|l| agent_tui::wrap::wrap_line(l, text_w as usize)).collect()
        }
    };
    // The table's `✓ ×N decay` line in full, under the way's name.
    if let Some(w) = fr.ways.get(r.sel).filter(|w| w.check_fires > 0) {
        let n = w.check_fires;
        let line = format!("✓ check fired {n} time{} in {}, decay={:.2}", if n == 1 { "" } else { "s" }, r.agents.label(&w.agent), table::decay(n));
        lines.insert(1.min(lines.len()), Line::styled(line, theme::muted()));
    }
    let inner_h = right.height.saturating_sub(2) as usize;
    r.page = inner_h.saturating_sub(1).max(1);
    r.scroll = r.scroll.min(lines.len().saturating_sub(inner_h));
    let mut title = vec![Span::raw(" why it fired ")];
    if lines.len() > inner_h {
        title.push(Span::styled(format!("{}–{}/{} ", r.scroll + 1, (r.scroll + inner_h).min(lines.len()), lines.len()), theme::muted()));
    }
    f.render_stateful_widget(list, left, &mut r.list);
    f.render_widget(Paragraph::new(lines).scroll((r.scroll as u16, 0)).block(pane(Line::from(title))), right);
    r.hits.rows = left;
    r.hits.text = right;
}

