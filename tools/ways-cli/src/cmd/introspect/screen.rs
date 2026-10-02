//! The introspect screens on agent-tui (ADR-504 §1, §9): the session
//! picker, and a session's timeline, replayed or live, with its why-fired
//! view. One [`Introspect`] holds them all, so the process runs one screen
//! session: Enter in the picker opens a session, Esc goes back to it.

use std::collections::HashMap;
use std::time::Duration;

use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use agent_tui::ratatui::layout::{Alignment, Constraint, Layout, Rect};
use agent_tui::ratatui::style::{Modifier, Style};
use agent_tui::ratatui::text::{Line, Span};
use agent_tui::ratatui::widgets::{Block, Borders, Cell, HighlightSpacing, List, ListItem, ListState, Paragraph, Row, Table, TableState};
use agent_tui::ratatui::Frame as Draw;
use agent_tui::screen::Screen;
use agent_tui::theme::{self, Ground, Palette, Seg, Shape};
use agent_tui::timeline::{key_bar, Playback, Scrubber};
use ways_agent_core::spend::{self, Group};
use ways_core::introspection::SessionIntrospection;

use super::model::{ActiveWay, Frame, Outcome};
use super::sessions::SessionInfo;
use super::table;
use super::why::{self, WhyIndex};

/// How often a live timeline reads the event log again (ADR-154 §3); the
/// stat check keeps an unchanged log cheap.
const LIVE_REFRESH: Duration = Duration::from_millis(250);

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
    /// The live source's (length, mtime) at the last read.
    sig: (u64, u64),
    /// Now, in Unix seconds, for how long ago the newest frame fired.
    pub(crate) now: u64,
    table: TableState,
    list: ListState,
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
            sel: 0,
            scroll: 0,
            page: 10,
            why: None,
            from_log: false,
            bodies: HashMap::new(),
            sig: (0, 0),
            now: agent_fmt::when::now_secs(),
            table: TableState::default(),
            list: ListState::default(),
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
        if live {
            r.sig = events_signature();
        }
        Ok(r)
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
        self.sel = anchor.map_or(0, |(id, epoch)| reselect_by_anchor(&self.shown(), &id, epoch));
        self.scroll = 0;
    }

    /// Frame indexes where a compaction window starts.
    fn window_starts(&self) -> Vec<usize> {
        self.frames.windows(2).enumerate().filter(|(_, p)| p[0].window != p[1].window).map(|(i, _)| i + 1).collect()
    }

    fn ways_len(&self) -> usize {
        self.shown().ways.len()
    }

    /// The selected way's id and the epoch it fired at, carried across a
    /// frame change so the cursor stays on the same way.
    fn anchor(&self) -> Option<(String, u64)> {
        self.shown().ways.get(self.sel.min(self.ways_len().saturating_sub(1))).map(|w| (w.id.clone(), w.epoch_fired))
    }

    /// Move along the timeline with `go`, keeping the selection on the
    /// same way, or the nearest that fired at or before it. Resuming a live
    /// follow puts the cursor back on the newest way.
    fn travel(&mut self, go: impl FnOnce(&mut Playback)) {
        let anchor = self.anchor();
        go(&mut self.play);
        self.sel = match anchor {
            Some((id, epoch)) => reselect_by_anchor(&self.shown(), &id, epoch),
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

    fn tick_every(&self) -> Option<Duration> {
        if self.play.is_live() {
            Some(LIVE_REFRESH)
        } else if self.play.playing() {
            Some(self.play.frame_time())
        } else {
            None
        }
    }

    fn tick(&mut self) {
        if self.play.is_live() {
            self.now = agent_fmt::when::now_secs();
            if self.from_log {
                self.refresh();
            }
        } else {
            self.travel(|p| {
                p.tick();
            });
        }
    }

    /// Read the live session again when the event log changed: new frames,
    /// the newest shown while following, and the why index read afresh.
    fn refresh(&mut self) {
        let sig = events_signature();
        if sig == self.sig {
            return;
        }
        self.sig = sig;
        // At launch the transcript may hold no model turn yet, so the window
        // read then is the 200K default; a later read corrects it. Only an
        // upgrade is taken: a real window never shrinks mid-session.
        let detected = crate::session::detect_context_window_for(&self.project, &self.session_id) / 1000;
        self.window_k = self.window_k.max(detected);
        let content = ways_core::firing::load_events_text();
        let events = super::frames::load_session_events(&content, &self.session_id);
        let frames = super::frames::reconstruct_all(&events, &self.project, &self.session_id, self.window_k * 1000);
        if frames.is_empty() {
            return;
        }
        self.judged = super::frames::has_verdicts(&events);
        self.spend = session_spend(&content, &self.session_id);
        self.take_frames(frames);
    }

    /// The frames read again: the newest shown while following, the
    /// selection kept on its way, and the why index read afresh.
    pub(crate) fn take_frames(&mut self, frames: Vec<Frame>) {
        let anchor = self.anchor();
        self.frames = frames;
        self.play.resize(self.frames.len());
        self.sel = anchor.map_or(0, |(id, epoch)| reselect_by_anchor(&self.shown(), &id, epoch));
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
/// same row, by id and epoch, which tells a way's blocked row from its
/// active one; else the same way; else the nearest active way that fired
/// at or before the anchor's epoch (the ways are in epoch order, so the
/// last such row), else the first row. Epochs restart at each compaction
/// window, so across one the id match does the work and the fallback only
/// places the cursor.
pub(super) fn reselect_by_anchor(frame: &Frame, anchor_id: &str, anchor_epoch: u64) -> usize {
    let same = |w: &&ActiveWay| w.id == anchor_id;
    if let Some(i) = frame.ways.iter().position(|w| same(&w) && w.epoch_fired == anchor_epoch).or_else(|| frame.ways.iter().position(|w| same(&w))) {
        return i;
    }
    frame.ways.iter().rposition(|w| w.epoch_fired <= anchor_epoch).unwrap_or(0)
}

/// The stat signature of the event sources: their combined length and the
/// newest mtime. A change means new events to read.
fn events_signature() -> (u64, u64) {
    let mut len = 0u64;
    let mut mtime = 0u64;
    for p in ways_core::paths::events_log_sources() {
        if let Ok(meta) = std::fs::metadata(&p) {
            len = len.saturating_add(meta.len());
            if let Ok(d) = meta.modified().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).map_err(std::io::Error::other)) {
                mtime = mtime.max(d.as_secs());
            }
        }
    }
    (len, mtime)
}

/// The sessions to pick from, newest first.
pub(crate) struct Picker {
    pub(crate) sessions: Vec<SessionInfo>,
    /// Where the sessions come from: a project, or every project.
    pub(crate) scope: String,
    sel: usize,
    table: TableState,
}

impl Picker {
    pub(crate) fn new(mut sessions: Vec<SessionInfo>, scope: String) -> Picker {
        sessions.reverse();
        Picker { sessions, scope, sel: 0, table: TableState::default() }
    }

    fn key(&mut self, k: KeyCode) {
        let last = self.sessions.len().saturating_sub(1);
        self.sel = match k {
            KeyCode::Up | KeyCode::Char('k') => self.sel.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => (self.sel + 1).min(last),
            KeyCode::PageUp => self.sel.saturating_sub(10),
            KeyCode::PageDown => (self.sel + 10).min(last),
            KeyCode::Home | KeyCode::Char('g') => 0,
            KeyCode::End | KeyCode::Char('G') => last,
            _ => self.sel,
        };
    }
}

/// Opens a session the picker chose.
pub(crate) type Opener = Box<dyn Fn(&str) -> Result<Replay, String>>;

/// The introspect screens: the picker, a session, or both.
pub(crate) struct Introspect {
    palette: Palette,
    shape: Shape,
    picker: Option<Picker>,
    pub(crate) replay: Option<Replay>,
    open: Opener,
    msg: String,
}

impl Introspect {
    pub(crate) fn picking(picker: Picker, open: Opener, palette: Palette, shape: Shape) -> Introspect {
        Introspect { palette, shape, picker: Some(picker), replay: None, open, msg: String::new() }
    }

    pub(crate) fn showing(replay: Replay, palette: Palette, shape: Shape) -> Introspect {
        Introspect { palette, shape, picker: None, replay: Some(replay), open: Box::new(|_| Err("no picker".into())), msg: String::new() }
    }
}

impl Screen for Introspect {
    fn palette(&self) -> Palette {
        self.palette
    }

    fn draw(&mut self, f: &mut Draw) {
        let shape = self.shape;
        match (&mut self.replay, &mut self.picker) {
            (Some(r), _) => draw_replay(f, r, shape),
            (None, Some(p)) => draw_picker(f, p, shape, &self.msg),
            (None, None) => {}
        }
    }

    fn key(&mut self, k: KeyEvent) -> bool {
        if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
            return false;
        }
        if let Some(r) = &mut self.replay {
            return match r.key(k) {
                Step::Stay => true,
                Step::Quit => false,
                Step::Back if self.picker.is_some() => {
                    self.replay = None;
                    true
                }
                Step::Back => false,
            };
        }
        let Some(p) = &mut self.picker else { return false };
        match k.code {
            KeyCode::Esc | KeyCode::Char('q') => return false,
            KeyCode::Enter => {
                if let Some(s) = p.sessions.get(p.sel) {
                    match (self.open)(&s.id) {
                        Ok(r) => {
                            self.replay = Some(r);
                            self.msg.clear();
                        }
                        Err(e) => self.msg = e,
                    }
                }
            }
            c => p.key(c),
        }
        true
    }

    fn tick_every(&self) -> Option<Duration> {
        self.replay.as_ref().and_then(Replay::tick_every)
    }

    fn tick(&mut self) {
        if let Some(r) = &mut self.replay {
            r.tick();
        }
    }
}

/// A bordered pane in the theme, as the settings screens draw theirs.
fn pane(title: impl Into<Line<'static>>) -> Block<'static> {
    Block::default().borders(Borders::ALL).border_style(theme::rule()).title(title).title_style(theme::title())
}

/// The tab line: the application, then each view, the shown one in the accent.
fn tabs(shape: Shape, views: &[(&str, bool)]) -> Line<'static> {
    let mut spans = shape.lozenge(&[Seg::on(" introspect ", Ground::AccentDim)]);
    for (name, on) in views {
        spans.push(Span::raw("  "));
        let seg = if *on { Seg::on(format!(" {name} "), Ground::Accent).bold() } else { Seg::faded(format!(" {name} ")) };
        spans.extend(shape.lozenge(&[seg]));
    }
    Line::from(spans)
}

fn draw_picker(f: &mut Draw, p: &mut Picker, shape: Shape, msg: &str) {
    let [bar, main, status] = Layout::vertical([Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)]).areas(f.area());
    f.render_widget(Paragraph::new(tabs(shape, &[("sessions", true)])), bar);
    // The count first: a long scope is cut at its end, never the count.
    let title = format!(" {} sessions in {} ", p.sessions.len(), p.scope);
    if p.sessions.is_empty() {
        f.render_widget(Paragraph::new(Line::styled("no sessions recorded", theme::muted())).block(pane(title)), main);
    } else {
        let right = |t: String| Cell::from(Line::from(t).alignment(Alignment::Right));
        let header = Row::new(vec![
            Cell::from("Session"),
            Cell::from("Date"),
            Cell::from("Project"),
            right("Events".into()),
            right("Ways".into()),
            right("Duration".into()),
            Cell::from("Transcript"),
        ])
        .style(Style::new().add_modifier(Modifier::BOLD));
        let rows: Vec<Row> = p
            .sessions
            .iter()
            .map(|s| {
                let project = s.project.rsplit('/').next().unwrap_or(&s.project).to_string();
                let date = s.ts.replace('T', " ");
                let transcript = if s.transcript { Span::styled("yes", theme::ok()) } else { Span::styled("gone", theme::muted()) };
                Row::new(vec![
                    Cell::from(s.id.chars().take(12).collect::<String>()),
                    Cell::from(date.chars().take(16).collect::<String>()),
                    Cell::from(project),
                    right(s.event_count.to_string()),
                    right(s.way_fires.to_string()),
                    right(agent_fmt::when::duration(s.duration_secs)),
                    Cell::from(transcript),
                ])
            })
            .collect();
        let widths = [
            Constraint::Length(12),
            Constraint::Length(16),
            Constraint::Min(8),
            Constraint::Length(6),
            Constraint::Length(5),
            Constraint::Length(8),
            Constraint::Length(10),
        ];
        let t = Table::new(rows, widths)
            .header(header)
            .column_spacing(1)
            .block(pane(title))
            .row_highlight_style(theme::selected())
            .highlight_symbol(Line::styled(theme::SELECTED_MARK, theme::accent()))
            .highlight_spacing(HighlightSpacing::Always);
        p.table.select(Some(p.sel));
        f.render_stateful_widget(t, main, &mut p.table);
    }
    let right = if msg.is_empty() {
        vec![Span::styled(format!("{}/{}", (p.sel + 1).min(p.sessions.len()), p.sessions.len()), theme::muted())]
    } else {
        vec![Span::styled(msg.to_string(), theme::err().add_modifier(Modifier::BOLD))]
    };
    let keys = [("↑↓", "select"), ("⏎", "replay"), ("q", "quit")];
    f.render_widget(Paragraph::new(key_bar(shape, "pick", Ground::Accent, &keys, right, status.width)), status);
}

/// `2026-07-03T16:52:00Z` as `2026-07-03 16:52`.
fn friendly_ts(ts: &str) -> String {
    let spaced = ts.replace('T', " ");
    spaced.get(..16).unwrap_or(&spaced).to_string()
}

/// The two header lines: the session and its project, then where the
/// frame shown sits, which ways the table holds, and for a live session
/// whether it follows. When the line is wider than `width`, the timestamp
/// goes first, then the count of ways judged out.
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
    for drop in [1u8, 2] {
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

fn draw_replay(f: &mut Draw, r: &mut Replay, shape: Shape) {
    let [bar, head, scrub, body, status] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(2), Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)]).areas(f.area());
    let n = r.ways_len();
    r.sel = r.sel.min(n.saturating_sub(1));
    f.render_widget(Paragraph::new(tabs(shape, &[("timeline", r.view == View::Timeline), ("why fired", r.view == View::Why)])), bar);
    f.render_widget(Paragraph::new(header(r, head.width)), head);
    let marks = r.window_starts();
    f.render_widget(Scrubber { len: r.play.len(), pos: r.play.pos(), marks: &marks }, scrub);
    match r.view {
        View::Timeline => draw_timeline(f, r, body),
        View::Why => draw_why(f, r, body),
    }

    let (mode, ground) = match (r.view, r.play.is_live(), r.play.following(), r.play.playing()) {
        (View::Why, ..) => ("why", Ground::Accent),
        (_, true, true, _) => ("live", Ground::Ok),
        (_, true, false, _) => ("paused", Ground::Warn),
        (_, false, _, true) => ("playing", Ground::Ok),
        _ => ("replay", Ground::Accent),
    };
    let speed = r.play.speed_label();
    let mut keys: Vec<(&str, &str)> = match r.view {
        View::Timeline => vec![("↑↓", "select"), ("⏎", "why"), ("←→", "frame")],
        View::Why => vec![("↑↓", "way"), ("j/k", "read"), ("←→", "frame")],
    };
    if r.view == View::Timeline {
        if r.play.is_live() {
            keys.push(("space", if r.play.following() { "pause" } else { "follow" }));
        } else {
            keys.push(("space", if r.play.playing() { "pause" } else { "play" }));
            keys.push(("+-", speed));
        }
        // Tab opens the why view too; `⏎ why` already names it, and the
        // bar has no room at 80 columns to say it twice.
    } else {
        keys.push(("esc", "timeline"));
    }
    keys.push(("f", if r.matched { "injected" } else { "matched" }));
    if r.spend.is_some() {
        keys.push(("$", if r.cost { "tokens" } else { "cost" }));
    }
    keys.push(("q", "quit"));
    f.render_widget(Paragraph::new(key_bar(shape, mode, ground, &keys, Vec::new(), status.width)), status);
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
        // The injected rows with their outcome marks, then, in the matched
        // view, the rows the judge blocked: the order `Frame::ways` keeps.
        let mut marked = fr.clone();
        for w in &mut marked.ways {
            w.id = format!("{}{}", w.outcome.mark(), w.id);
        }
        let mut rows = table::rows(&marked, r.window_k, inner_w);
        rows.extend(shown.ways.iter().filter(|w| w.outcome == Outcome::Blocked).map(|w| blocked_row(w, shown.epoch)));
        let t = Table::new(rows, table::WIDTHS)
            .header(table::header())
            .column_spacing(2)
            .block(pane(title))
            .row_highlight_style(theme::selected())
            .highlight_symbol(Line::styled(theme::SELECTED_MARK, theme::accent()))
            .highlight_spacing(HighlightSpacing::Always);
        // From the top each frame: the table scrolls only as far as the
        // selection needs, so a frame with fewer ways never hides its first.
        *r.table.offset_mut() = 0;
        r.table.select(Some(r.sel));
        f.render_stateful_widget(t, ways, &mut r.table);
    }
    if ctx_h > 0 {
        let lines: Vec<Line> = ctx.into_iter().map(|(_, l)| l).collect();
        f.render_widget(Paragraph::new(lines).block(pane(" context ")), context);
    }
}

/// A candidate the judge blocked, in the table's columns: judged in this
/// frame, its P(yes) where the trigger goes, and nothing to re-disclose,
/// for it injected nothing. A way blocked with its ancestor names it.
fn blocked_row(w: &ActiveWay, epoch: u64) -> Row<'static> {
    let right = |t: String| Cell::from(Line::from(t).alignment(Alignment::Right));
    let with = if w.ancestor.is_empty() { String::new() } else { format!(" (with {})", w.ancestor) };
    Row::new(vec![
        Cell::from(format!("{}{}{with}", w.outcome.mark(), w.id)),
        right(w.epoch_fired.to_string()),
        right(epoch.saturating_sub(w.epoch_fired).to_string()),
        Cell::from(format!("{} {}", w.trigger, w.p_yes)),
        Cell::from(" "),
        Cell::from("not injected"),
    ])
    .style(theme::muted())
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
            let id = format!("{}{}", w.outcome.mark(), w.id);
            ListItem::new(Line::from(vec![
                Span::raw(format!("{bullet} ")),
                Span::styled(format!("e{:>ew$} ", w.epoch_fired), theme::muted()),
                if w.outcome == Outcome::Blocked { Span::styled(id, theme::muted()) } else { Span::raw(id) },
            ]))
        })
        .collect();
    let list = List::new(items)
        .block(pane(" ways · epoch "))
        .highlight_style(theme::selected())
        .highlight_symbol(Line::styled(theme::SELECTED_MARK, theme::accent()))
        .highlight_spacing(HighlightSpacing::Always);
    *r.list.offset_mut() = 0;
    r.list.select(if fr.ways.is_empty() { None } else { Some(r.sel) });

    let text_w = right.width.saturating_sub(2);
    let lines: Vec<Line> = match fr.ways.get(r.sel) {
        None => vec![Line::styled("no ways fired in this frame", theme::muted())],
        Some(w) if r.why.is_none() => vec![Line::styled(w.id.clone(), Style::new().add_modifier(Modifier::BOLD)), Line::styled("no introspection model for this session", theme::muted())],
        Some(w) => {
            let entry = facet(&w.id, &w.trigger);
            let path = entry.and_then(|e| e.way_path.clone());
            let body = path.and_then(|p| r.bodies.entry(p.clone()).or_insert_with(|| why::read_way_body(&p)).clone());
            why::detail_lines(&w.id, entry, body.as_deref(), text_w).iter().flat_map(|l| agent_tui::wrap::wrap_line(l, text_w as usize)).collect()
        }
    };
    let inner_h = right.height.saturating_sub(2) as usize;
    r.page = inner_h.saturating_sub(1).max(1);
    r.scroll = r.scroll.min(lines.len().saturating_sub(inner_h));
    let mut title = vec![Span::raw(" why it fired ")];
    if lines.len() > inner_h {
        title.push(Span::styled(format!("{}–{}/{} ", r.scroll + 1, (r.scroll + inner_h).min(lines.len()), lines.len()), theme::muted()));
    }
    f.render_stateful_widget(list, left, &mut r.list);
    f.render_widget(Paragraph::new(lines).scroll((r.scroll as u16, 0)).block(pane(Line::from(title))), right);
}
