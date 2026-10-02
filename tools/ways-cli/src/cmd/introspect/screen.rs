//! The introspect screens on agent-tui (ADR-504 §1, §9): the session
//! picker, and a session's timeline, replayed or live, with its why-fired
//! view. One [`Introspect`] holds them all, so the process runs one screen
//! session: Enter in the picker opens a session, Esc goes back to it.

use std::collections::HashMap;
use std::time::Duration;

use agent_tui::markdown;
use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use agent_tui::ratatui::layout::{Alignment, Constraint, Layout, Rect};
use agent_tui::ratatui::style::{Modifier, Style};
use agent_tui::ratatui::text::{Line, Span};
use agent_tui::ratatui::widgets::{Block, Borders, Cell, HighlightSpacing, List, ListItem, ListState, Paragraph, Row, Table, TableState};
use agent_tui::ratatui::Frame as Draw;
use agent_tui::screen::Screen;
use agent_tui::theme::{self, Ground, Palette, Seg, Shape};
use agent_tui::timeline::{key_bar, Playback, Scrubber};
use ways_core::introspection::SessionIntrospection;

use super::model::Frame;
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
        Replay {
            session_id,
            project,
            window_k,
            frames,
            play,
            view: View::Timeline,
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
        }
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
        let short = &session_id[..session_id.len().min(12)];
        if events.is_empty() {
            return Err(format!("no events for session {short}"));
        }
        let window = crate::session::detect_context_window_for(&project, session_id);
        let frames = super::frames::reconstruct_frames(&events, &project, session_id, window);
        if frames.is_empty() {
            return Err(format!("no frames to replay in session {short}"));
        }
        let play = if live { Playback::live(frames.len()) } else { Playback::replay(frames.len()) };
        let mut r = Replay::new(session_id.to_string(), project, window / 1000, frames, play);
        r.from_log = true;
        if live {
            r.sig = events_signature();
        }
        Ok(r)
    }

    fn frame(&self) -> &Frame {
        &self.frames[self.play.pos()]
    }

    /// Frame indexes where a compaction window starts.
    fn window_starts(&self) -> Vec<usize> {
        self.frames.windows(2).enumerate().filter(|(_, p)| p[0].window != p[1].window).map(|(i, _)| i + 1).collect()
    }

    fn ways_len(&self) -> usize {
        self.frame().ways.len()
    }

    /// The selected way's id and the epoch it fired at, carried across a
    /// frame change so the cursor stays on the same way.
    fn anchor(&self) -> Option<(String, u64)> {
        self.frame().ways.get(self.sel.min(self.ways_len().saturating_sub(1))).map(|w| (w.id.clone(), w.epoch_fired))
    }

    /// Move along the timeline with `go`, keeping the selection on the
    /// same way, or the nearest that fired at or before it.
    fn travel(&mut self, go: impl FnOnce(&mut Playback)) {
        let anchor = self.anchor();
        go(&mut self.play);
        self.sel = match anchor {
            Some((id, epoch)) => reselect_by_anchor(self.frame(), &id, epoch),
            None => 0,
        };
        self.scroll = 0;
    }

    fn open_why(&mut self) {
        self.view = View::Why;
        self.play.pause();
        self.scroll = 0;
        if self.why.is_none() && self.from_log {
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
            }
            (View::Timeline, KeyCode::Tab) => self.open_why(),
            (View::Timeline, KeyCode::Enter) if n > 0 => self.open_why(),
            (View::Timeline, KeyCode::Up | KeyCode::Char('k')) | (View::Why, KeyCode::Up) => {
                self.sel = self.sel.saturating_sub(1);
                self.scroll = 0;
            }
            (View::Timeline, KeyCode::Down | KeyCode::Char('j')) | (View::Why, KeyCode::Down) => {
                self.sel = (self.sel + 1).min(n.saturating_sub(1));
                self.scroll = 0;
            }
            (View::Timeline, KeyCode::PageUp) => self.sel = self.sel.saturating_sub(10),
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
        let frames = super::frames::reconstruct_frames(&events, &self.project, &self.session_id, self.window_k * 1000);
        if frames.is_empty() {
            return;
        }
        let anchor = self.anchor();
        self.frames = frames;
        self.play.resize(self.frames.len());
        self.sel = anchor.map_or(0, |(id, epoch)| reselect_by_anchor(self.frame(), &id, epoch));
        if self.why.is_some() {
            self.why = None;
            if self.view == View::Why {
                self.open_why();
            }
        }
    }
}

/// The row in `frame` that best keeps an anchor across a frame change: the
/// same way if it is still active, else the nearest active way that fired
/// at or before the anchor's epoch (the ways are in epoch order, so the
/// last such row), else the first row. Epochs restart at each compaction
/// window, so across one the id match does the work and the fallback only
/// places the cursor.
pub(super) fn reselect_by_anchor(frame: &Frame, anchor_id: &str, anchor_epoch: u64) -> usize {
    if let Some(i) = frame.ways.iter().position(|w| w.id == anchor_id) {
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
    let title = format!(" sessions in {} ({}) ", p.scope, p.sessions.len());
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
/// frame shown sits, and for a live session whether it follows.
fn header(r: &Replay) -> Vec<Line<'static>> {
    let fr = r.frame();
    let windows = r.frames.last().map_or(1, |l| l.window);
    let mut metrics = vec![Span::styled(
        format!(
            "epoch {} · {}K ctx · {} ways · window {}/{} · {}",
            fr.epoch,
            r.window_k,
            fr.ways.len(),
            fr.window,
            windows,
            friendly_ts(&fr.timestamp)
        ),
        theme::muted(),
    )];
    if r.play.is_live() {
        if r.play.following() {
            metrics.push(Span::styled("  ● LIVE", theme::ok().add_modifier(Modifier::BOLD)));
            if let Some(then) = agent_fmt::when::parse_utc_iso(&fr.timestamp) {
                metrics.push(Span::styled(format!(" · {}", agent_fmt::when::ago(r.now.saturating_sub(then))), theme::muted()));
            }
        } else {
            metrics.push(Span::styled("  ● LIVE paused", theme::warn().add_modifier(Modifier::BOLD)));
        }
    }
    vec![
        Line::from(vec![
            Span::styled("Session ", Style::new().add_modifier(Modifier::BOLD)),
            Span::raw(r.session_id.clone()),
            Span::styled(format!("  {}", r.project), theme::muted()),
        ]),
        Line::from(metrics),
    ]
}

fn draw_replay(f: &mut Draw, r: &mut Replay, shape: Shape) {
    let [bar, head, scrub, body, status] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(2), Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)]).areas(f.area());
    let n = r.ways_len();
    r.sel = r.sel.min(n.saturating_sub(1));
    f.render_widget(Paragraph::new(tabs(shape, &[("timeline", r.view == View::Timeline), ("why fired", r.view == View::Why)])), bar);
    f.render_widget(Paragraph::new(header(r)), head);
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
        keys.push(("tab", "why"));
    } else {
        keys.push(("esc", "timeline"));
    }
    keys.push(("q", "quit"));
    f.render_widget(Paragraph::new(key_bar(shape, mode, ground, &keys, Vec::new(), status.width)), status);
}

fn draw_timeline(f: &mut Draw, r: &mut Replay, area: Rect) {
    let inner_w = area.width.saturating_sub(2) as usize;
    let fr = &r.frames[r.play.pos()];
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
    if fr.ways.is_empty() {
        f.render_widget(Paragraph::new(Line::styled("no ways fired yet", theme::muted())).block(pane(title)), ways);
    } else {
        let t = Table::new(table::rows(fr, r.window_k, inner_w), table::WIDTHS)
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

fn draw_why(f: &mut Draw, r: &mut Replay, area: Rect) {
    let left_w = (area.width / 3).clamp(16, 40).min(area.width.saturating_sub(14));
    let [left, right] = Layout::horizontal([Constraint::Length(left_w), Constraint::Min(10)]).areas(area);
    let fr = &r.frames[r.play.pos()];
    let ew = fr.ways.iter().map(|w| w.epoch_fired).max().unwrap_or(0).to_string().len();
    let facet = |id: &str, trigger: &str| r.why.as_ref().and_then(|ix| ix.get(&(id.to_string(), trigger.to_string())));
    let items: Vec<ListItem> = fr
        .ways
        .iter()
        .map(|w| {
            // A filled bullet marks a way the model has a record of on this channel.
            let bullet = if facet(&w.id, &w.trigger).is_some() { "•" } else { "·" };
            ListItem::new(Line::from(vec![
                Span::raw(format!("{bullet} ")),
                Span::styled(format!("e{:>ew$} ", w.epoch_fired), theme::muted()),
                Span::raw(w.id.clone()),
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
            why::detail_lines(&w.id, entry, body.as_deref(), text_w).iter().flat_map(|l| markdown::wrap_line(l, text_w)).collect()
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
