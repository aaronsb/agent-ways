//! The generic TUI over a settings tree: one tab per root, then browse,
//! filter, edit, run actions, and review and apply as a read-only mode of the
//! browser. Key handling and state live here; drawing is in `render` and `review`.

mod apply;
pub mod flow;
mod render;
mod review;
pub mod theme;

use std::collections::BTreeSet;
use std::io;
use std::time::Duration;

use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Position, Rect};
use ratatui::widgets::ListState;
use ratatui::DefaultTerminal;

use crate::tree::{self, Arg, Kind, Node, Queue, Queued, Row, SecretBuf};
use apply::{Failure, Outcome, Run};
use flow::{Flow, FlowEvent};

enum Mode {
    Browse,
    Edit(String),
    Filter,
    Help,
    /// Choosing among the actions of the node at `path`.
    Menu { path: Vec<usize>, sel: usize },
    /// A visible argument, such as a path, for action `action` of the node.
    Arg { path: Vec<usize>, action: usize, buf: String },
    /// A masked argument for action `action` of the node.
    Secret { path: Vec<usize>, action: usize, buf: SecretBuf },
    /// y/n before the action is queued.
    Confirm { queued: Queued },
    /// The browser's layout over one tab's pending items, read-only. `run` is
    /// the simulated apply in flight, a step per tick; `discard` asks y/n
    /// before the tab's items are dropped.
    Review { tab: usize, run: Option<Run>, discard: bool },
    /// y/n before one tab's pending items are dropped.
    DiscardTab { tab: usize },
    /// Quit was asked with items pending in any tab: go back, review, or
    /// quit and discard them all, which `confirm` asks a second time.
    Guard { confirm: bool },
    /// A guided flow; finishing it queues its commands on the tab that launched it.
    Flow(Box<Flow>),
}

/// The click targets of review's bar and the quit prompt, and a flow's buttons.
#[derive(Clone, Copy, PartialEq)]
enum Btn {
    Apply,
    Discard,
    Back,
    /// The quit prompt's jump to the first tab with pending items.
    Review,
    /// The quit prompt's quit-and-discard.
    Quit,
    /// A flow's buttons: on to the next step, finish on the last, or drop it.
    Next,
    Finish,
    Cancel,
}

impl Btn {
    /// The key a click or Enter on the button stands for.
    fn key(self) -> KeyCode {
        match self {
            Btn::Apply => KeyCode::Char('a'),
            Btn::Discard => KeyCode::Char('X'),
            Btn::Quit => KeyCode::Char('D'),
            Btn::Back => KeyCode::Esc,
            Btn::Review => KeyCode::Char('r'),
            Btn::Next | Btn::Finish => KeyCode::Right,
            Btn::Cancel => KeyCode::Char('q'),
        }
    }
}

/// How long a step stays in each state while an apply runs.
const TICK: Duration = Duration::from_millis(350);

/// What a key that would edit says in review.
const READ_ONLY: &str = "read-only in review; Esc to edit";

/// Where the last frame drew what a click can hit.
#[derive(Default)]
struct Hits {
    /// Each tab's lozenge and its root.
    tabs: Vec<(Rect, usize)>,
    /// The tree list inside its border; row `i` is at `y + i - offset`.
    list: Rect,
    /// The open menu and one rect per item.
    menu: Option<(Rect, Vec<Rect>)>,
    /// A confirm's answers: yes or no.
    answers: Vec<(Rect, bool)>,
    /// Review's bar targets and the quit prompt's buttons.
    buttons: Vec<(Rect, Btn)>,
    /// The call to action on the bottom bar.
    cta: Rect,
    /// Each tab badge's discard mark, and its tab.
    discard_tabs: Vec<(Rect, usize)>,
}

/// What a session leaves behind: the edited tree and the queued actions.
pub struct Session {
    pub roots: Vec<Node>,
    pub queue: Queue,
}

pub struct App {
    pub roots: Vec<Node>,
    queue: Queue,
    title: String,
    /// The tab shown: an index into `roots`.
    tab: usize,
    /// The cursor in the rows on screen: the tab's, or the filter's.
    cursor: usize,
    /// Each tab's cursor while another tab, or a filter, is shown.
    saved: Vec<usize>,
    mode: Mode,
    filter: String,
    msg: String,
    show_changes: bool,
    list: ListState,
    /// Each tab's cursor in review, among the rows it shows.
    rcursor: Vec<usize>,
    /// The review groups the operator closed, by path; the `queued` group is the tab's root path.
    closed: BTreeSet<Vec<usize>>,
    /// The step the last apply stopped at, marked until review is left or applied again.
    failure: Option<Failure>,
    /// The 1-based step an apply fails at, to see the failure path.
    fail_step: Option<usize>,
    /// Whether the terminal should report the mouse; `m` turns it off so the
    /// terminal's own text selection works.
    mouse: bool,
    shape: theme::Shape,
    hits: Hits,
    /// Builds the guided flow an action names; the adapter supplies it.
    helpers: Option<Helpers>,
}

/// Turns the name an `Arg::Flow` carries into the flow.
pub type Helpers = Box<dyn Fn(&str) -> Option<Flow>>;

impl App {
    pub fn new(title: impl Into<String>, roots: Vec<Node>) -> Self {
        let roots_len = roots.len();
        App {
            saved: vec![0; roots_len],
            roots,
            queue: Queue::default(),
            title: title.into(),
            tab: 0,
            cursor: 0,
            mode: Mode::Browse,
            filter: String::new(),
            msg: "? keys · Tab 1-9 tabs · / filters all tabs".into(),
            show_changes: false,
            list: ListState::default(),
            rcursor: vec![0; roots_len],
            closed: BTreeSet::new(),
            failure: None,
            fail_step: None,
            mouse: true,
            shape: theme::Shape::ROUND,
            hits: Hits::default(),
            helpers: None,
        }
    }

    pub fn helpers(mut self, h: impl Fn(&str) -> Option<Flow> + 'static) -> Self {
        self.helpers = Some(Box::new(h));
        self
    }

    pub fn shape(mut self, shape: theme::Shape) -> Self {
        self.shape = shape;
        self
    }

    /// Make the apply step numbered `n` (from 1) fail.
    pub fn fail_step(mut self, n: Option<usize>) -> Self {
        self.fail_step = n;
        self
    }

    /// Run until quit. Mouse capture follows `self.mouse`; the caller turns
    /// it off on every exit path, since `ratatui::restore` does not.
    pub fn run(mut self, term: &mut DefaultTerminal) -> io::Result<Session> {
        let mut captured = false;
        loop {
            if self.mouse != captured {
                if self.mouse {
                    execute!(io::stdout(), EnableMouseCapture)?;
                } else {
                    execute!(io::stdout(), DisableMouseCapture)?;
                }
                captured = self.mouse;
            }
            term.draw(|f| self.draw(f))?;
            if matches!(self.mode, Mode::Review { run: Some(_), .. }) && !event::poll(TICK)? {
                self.tick();
                continue;
            }
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press && !self.key(k) => {
                    return Ok(Session { roots: self.roots, queue: self.queue });
                }
                Event::Mouse(m) => self.mouse(m),
                _ => {}
            }
        }
    }

    /// Value changes plus queued actions, in every tab.
    fn pending(&self) -> usize {
        (0..self.roots.len()).map(|t| self.pending_in(t)).sum()
    }

    fn pending_in(&self, tab: usize) -> usize {
        tree::pending(&self.roots[tab], &self.queue)
    }

    /// One tick of a running apply. A finished run is closed out on the tick
    /// after its last state, so the last glyph is seen.
    fn tick(&mut self) {
        let Mode::Review { run: Some(run), .. } = &mut self.mode else { return };
        if run.finished() {
            return self.finish_run();
        }
        run.tick(&mut self.roots, &mut self.queue);
        self.follow_run();
    }

    /// Put the cursor on the row of the step that is running, or that failed.
    fn follow_run(&mut self) {
        let Mode::Review { tab, run: Some(run), .. } = &self.mode else { return };
        let at = run.steps.iter().position(|s| matches!(s.state, apply::St::Running | apply::St::Failed));
        let rows = apply::visible(&run.rows, &self.closed);
        if let Some(i) = at.and_then(|at| rows.iter().position(|r| run.step_of(r) == Some(at))) {
            self.rcursor[*tab] = i;
        }
    }

    /// A run ended: success clears the tab and goes on; a stop keeps the
    /// failed step and what follows, marked, with the review on the tab.
    fn finish_run(&mut self) {
        let Mode::Review { run: Some(run), .. } = std::mem::replace(&mut self.mode, Mode::Browse) else { return };
        let tab = run.tab;
        let name = self.roots[tab].name.clone();
        match run.outcome {
            Outcome::Stopped(i) => {
                self.msg = format!("stopped at step {} of {}; {} still pending in {name}", i + 1, run.steps.len(), self.pending_in(tab));
                self.failure = run.failure();
                self.mode = Mode::Review { tab, run: None, discard: false };
                self.focus_failure(tab);
            }
            _ => {
                self.msg = format!("applied {} in {name}", run.applied);
                self.advance(tab);
            }
        }
    }

    /// Put the cursor on the first row the failure marks.
    fn focus_failure(&mut self, tab: usize) {
        let rows = self.review_view(tab);
        if let Some(i) = self.failure.as_ref().and_then(|f| rows.iter().position(|r| f.marks(r))) {
            self.rcursor[tab] = i;
        }
    }

    /// The next tab after `tab` with anything pending, wrapping round.
    fn next_pending(&self, tab: usize) -> Option<usize> {
        let n = self.roots.len();
        (1..=n).map(|d| (tab + d) % n).find(|t| self.pending_in(*t) > 0)
    }

    /// `tab` has been applied or discarded: review moves to the next tab with
    /// pending items, or ends when there is none.
    fn advance(&mut self, tab: usize) {
        self.failure = None;
        self.mode = match self.next_pending(tab) {
            Some(next) => Mode::Review { tab: next, run: None, discard: false },
            None => Mode::Browse,
        };
    }

    /// Quit, or ask first when anything is pending in any tab.
    fn quit(&mut self) -> bool {
        if self.pending() == 0 {
            return false;
        }
        self.mode = Mode::Guard { confirm: false };
        true
    }

    /// Enter review on `tab`, or on the next tab with pending items when it
    /// has none. Leaving and entering again starts every group open.
    fn open_review(&mut self, tab: usize) {
        let Some(at) = Some(tab).filter(|t| self.pending_in(*t) > 0).or_else(|| self.next_pending(tab)) else {
            self.msg = format!("nothing pending in {}", self.roots[tab].name);
            return;
        };
        self.closed.clear();
        self.failure = None;
        self.msg = READ_ONLY.into();
        self.mode = Mode::Review { tab: at, run: None, discard: false };
    }

    fn begin_apply(&mut self, tab: usize) {
        let run = Run::plan(&self.roots, &self.queue, tab, self.fail_step);
        if run.steps.is_empty() {
            self.msg = format!("nothing pending in {}", self.roots[tab].name);
            self.mode = Mode::Review { tab, run: None, discard: false };
        } else {
            self.failure = None;
            self.msg.clear();
            self.mode = Mode::Review { tab, run: Some(run), discard: false };
            self.follow_run();
        }
    }

    /// Revert one tab's values and unqueue its actions.
    fn discard_tab(&mut self, tab: usize) {
        self.msg = format!("discarded {} in {}", self.pending_in(tab), self.roots[tab].name);
        tree::revert_all(&mut self.roots[tab..=tab]);
        self.queue.remove_under(&self.roots[tab].name);
    }

    fn discard_all(&mut self) {
        tree::revert_all(&mut self.roots);
        self.queue.clear();
    }

    /// The tab's rows, or with a filter the matches of every tab.
    fn rows(&self) -> Vec<Row> {
        if self.filter.is_empty() {
            tree::tab_rows(&self.roots, self.tab)
        } else {
            tree::rows(&self.roots, &self.filter)
        }
    }

    fn switch_tab(&mut self, to: usize) {
        if !self.filter.is_empty() {
            self.msg = "Esc clears the filter before switching tabs".into();
            return;
        }
        self.saved[self.tab] = self.cursor;
        self.tab = to;
        self.cursor = self.saved[to];
    }

    fn clear_filter(&mut self) {
        if !self.filter.is_empty() {
            self.filter.clear();
            self.cursor = self.saved[self.tab];
        }
    }

    /// Show the node at `path` in its own tab, opening what hides it.
    fn jump(&mut self, path: &[usize]) {
        self.filter.clear();
        for i in 1..path.len() {
            tree::get_mut(&mut self.roots, &path[..i]).open = true;
        }
        self.tab = path[0];
        self.cursor = self.rows().iter().position(|r| r.path == path).unwrap_or(0);
        self.saved[self.tab] = self.cursor;
        self.msg = tree::key(&self.roots, path);
    }

    /// A flow ended: queue what it finished with, on the tab that launched it,
    /// or drop it. Always true: a flow never ends the session.
    fn flow_event(&mut self, ev: FlowEvent) -> bool {
        match ev {
            FlowEvent::Stay => {}
            FlowEvent::Cancel => {
                self.mode = Mode::Browse;
                self.msg = "flow cancelled; nothing queued".into();
            }
            FlowEvent::Finish { key, outs } => {
                self.mode = Mode::Browse;
                self.msg = if outs.is_empty() { "flow finished; nothing to queue".into() } else { format!("queued {} from the flow", outs.len()) };
                for o in outs {
                    self.queue.push(Queued { key: key.clone(), label: o.label, command: o.command, confirm: o.confirm });
                }
            }
        }
        true
    }

    /// Open the flow an action names, launched from the node at `path`.
    fn start_flow(&mut self, path: &[usize], name: &str) {
        match self.helpers.as_ref().and_then(|h| h(name)) {
            Some(mut flow) => {
                flow.key = tree::key(&self.roots, path);
                self.mode = Mode::Flow(Box::new(flow));
            }
            None => self.msg = format!("no guided flow named {name}"),
        }
    }

    /// Handle one key. False ends the session.
    fn key(&mut self, k: KeyEvent) -> bool {
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            // A running apply is not interrupted; anything pending asks first.
            if matches!(self.mode, Mode::Review { run: Some(_), .. } | Mode::Guard { .. }) {
                return true;
            }
            if matches!(self.mode, Mode::Flow(_)) {
                return self.flow_event(FlowEvent::Cancel);
            }
            return self.quit();
        }
        match std::mem::replace(&mut self.mode, Mode::Browse) {
            Mode::Flow(mut flow) => {
                let ev = flow.key(k);
                if ev == FlowEvent::Stay {
                    self.mode = Mode::Flow(flow);
                } else {
                    return self.flow_event(ev);
                }
            }
            Mode::Help => {}
            Mode::Filter => match k.code {
                KeyCode::Esc => self.clear_filter(),
                KeyCode::Enter => {}
                KeyCode::Backspace => {
                    self.filter.pop();
                    if self.filter.is_empty() {
                        self.cursor = self.saved[self.tab];
                    }
                    self.mode = Mode::Filter;
                }
                KeyCode::Char(c) => {
                    if self.filter.is_empty() {
                        self.saved[self.tab] = self.cursor;
                    }
                    self.filter.push(c);
                    self.cursor = 0;
                    self.mode = Mode::Filter;
                }
                _ => self.mode = Mode::Filter,
            },
            Mode::Edit(mut buf) => match k.code {
                KeyCode::Esc => self.msg = "edit cancelled".into(),
                KeyCode::Enter => self.commit(&buf),
                KeyCode::Backspace => {
                    buf.pop();
                    self.mode = Mode::Edit(buf);
                }
                KeyCode::Char(c) => {
                    buf.push(c);
                    self.mode = Mode::Edit(buf);
                }
                _ => self.mode = Mode::Edit(buf),
            },
            Mode::Menu { path, sel } => {
                let len = tree::get(&self.roots, &path).actions.len();
                match k.code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('a') => {}
                    KeyCode::Enter => self.pick(path, sel),
                    KeyCode::Up | KeyCode::Char('k') => self.mode = Mode::Menu { path, sel: sel.saturating_sub(1) },
                    KeyCode::Down | KeyCode::Char('j') => self.mode = Mode::Menu { path, sel: (sel + 1).min(len - 1) },
                    _ => self.mode = Mode::Menu { path, sel },
                }
            }
            Mode::Arg { path, action, mut buf } => match k.code {
                KeyCode::Esc => self.msg = "cancelled".into(),
                KeyCode::Enter if buf.trim().is_empty() => self.mode = Mode::Arg { path, action, buf },
                KeyCode::Enter => self.stage(&path, action, buf.trim()),
                KeyCode::Backspace => {
                    buf.pop();
                    self.mode = Mode::Arg { path, action, buf };
                }
                KeyCode::Char(c) => {
                    buf.push(c);
                    self.mode = Mode::Arg { path, action, buf };
                }
                _ => self.mode = Mode::Arg { path, action, buf },
            },
            Mode::Secret { path, action, mut buf } => match k.code {
                KeyCode::Esc => self.msg = "cancelled; nothing entered was kept".into(),
                KeyCode::Enter if buf.is_empty() => self.mode = Mode::Secret { path, action, buf },
                // The queued command shows `<stdin>`; `buf` drops, zeroed, here.
                KeyCode::Enter => self.stage(&path, action, ""),
                KeyCode::Backspace => {
                    buf.pop();
                    self.mode = Mode::Secret { path, action, buf };
                }
                KeyCode::Char(c) => {
                    buf.push(c);
                    self.mode = Mode::Secret { path, action, buf };
                }
                _ => self.mode = Mode::Secret { path, action, buf },
            },
            Mode::Confirm { queued } => match k.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => self.enqueue(queued),
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => self.msg = "not queued".into(),
                _ => self.mode = Mode::Confirm { queued },
            },
            Mode::Review { tab, run, discard } => self.review_key(k, tab, run, discard),
            Mode::DiscardTab { tab } => match k.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => self.discard_tab(tab),
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {}
                _ => self.mode = Mode::DiscardTab { tab },
            },
            Mode::Guard { confirm: false } => match k.code {
                KeyCode::Char('r') => {
                    let first = (0..self.roots.len()).find(|t| self.pending_in(*t) > 0).unwrap_or(self.tab);
                    self.clear_filter();
                    self.switch_tab(first);
                    self.open_review(first);
                }
                KeyCode::Char('D') => self.mode = Mode::Guard { confirm: true },
                KeyCode::Esc | KeyCode::Char('b') | KeyCode::Enter => {}
                _ => self.mode = Mode::Guard { confirm: false },
            },
            Mode::Guard { confirm: true } => match k.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.discard_all();
                    return false;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => self.mode = Mode::Guard { confirm: false },
                _ => self.mode = Mode::Guard { confirm: true },
            },
            Mode::Browse => return self.browse(k),
        }
        true
    }

    /// Review's keys. Moving and opening groups work; nothing edits or queues.
    /// A run in flight takes no keys.
    fn review_key(&mut self, k: KeyEvent, tab: usize, run: Option<Run>, mut discard: bool) {
        self.mode = Mode::Review { tab, run, discard };
        if matches!(self.mode, Mode::Review { run: Some(_), .. }) {
            return;
        }
        if discard {
            match k.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.discard_tab(tab);
                    return self.advance(tab);
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => discard = false,
                _ => {}
            }
            self.mode = Mode::Review { tab, run: None, discard };
            return;
        }
        let rows = self.review_view(tab);
        let last = rows.len().saturating_sub(1);
        let cur = self.rcursor[tab].min(last);
        let open = |app: &mut App, i: usize, on: bool| {
            if let Some(r) = rows.get(i).filter(|r| r.toggles()) {
                if on { app.closed.remove(&r.path) } else { app.closed.insert(r.path.clone()) };
            }
        };
        match k.code {
            KeyCode::Esc => {
                self.failure = None;
                self.msg.clear();
                self.mode = Mode::Browse;
            }
            KeyCode::Char('a') => self.begin_apply(tab),
            KeyCode::Char('X') => {
                if self.pending_in(tab) == 0 {
                    self.msg = format!("nothing pending in {}", self.roots[tab].name);
                } else {
                    self.mode = Mode::Review { tab, run: None, discard: true };
                }
            }
            KeyCode::Char('q') => {
                self.mode = Mode::Browse;
                self.quit();
            }
            KeyCode::Tab => self.review_to(self.next_pending(tab)),
            KeyCode::BackTab => {
                let n = self.roots.len();
                self.review_to((1..=n).map(|d| (tab + n - d) % n).find(|t| self.pending_in(*t) > 0));
            }
            KeyCode::Char(c @ '1'..='9') if (c as usize - '1' as usize) < self.roots.len() => {
                let to = c as usize - '1' as usize;
                if self.pending_in(to) > 0 {
                    self.review_to(Some(to));
                } else {
                    self.msg = format!("nothing pending in {}", self.roots[to].name);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => self.rcursor[tab] = cur.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.rcursor[tab] = (cur + 1).min(last),
            KeyCode::PageUp => self.rcursor[tab] = cur.saturating_sub(10),
            KeyCode::PageDown => self.rcursor[tab] = (cur + 10).min(last),
            KeyCode::Home | KeyCode::Char('g') => self.rcursor[tab] = 0,
            KeyCode::End | KeyCode::Char('G') => self.rcursor[tab] = last,
            KeyCode::Right | KeyCode::Char('l') => match rows.get(cur) {
                Some(r) if r.toggles() && self.closed.contains(&r.path) => open(self, cur, true),
                Some(r) if r.toggles() => self.rcursor[tab] = (cur + 1).min(last),
                _ => {}
            },
            KeyCode::Left | KeyCode::Char('h') => match rows.get(cur) {
                Some(r) if r.toggles() && !self.closed.contains(&r.path) => open(self, cur, false),
                Some(r) => {
                    if let Some(parent) = rows[..cur].iter().rposition(|p| p.depth < r.depth) {
                        self.rcursor[tab] = parent;
                    }
                }
                None => {}
            },
            KeyCode::Enter | KeyCode::Char(' ') => match rows.get(cur) {
                Some(r) if r.toggles() => open(self, cur, self.closed.contains(&r.path)),
                _ => self.msg = READ_ONLY.into(),
            },
            KeyCode::Char('e' | 'd' | 'u' | 'x' | 'w' | 'c') => self.msg = READ_ONLY.into(),
            KeyCode::Char('m') => {
                self.mouse = !self.mouse;
                self.msg = if self.mouse { "mouse on" } else { "mouse off: the terminal selects text" }.into();
            }
            _ => {}
        }
    }

    /// Show another tab's review; none when there is nowhere to go.
    fn review_to(&mut self, to: Option<usize>) {
        if let (Some(to), Mode::Review { tab, .. }) = (to, &mut self.mode) {
            *tab = to;
            self.failure = None;
        }
    }

    fn browse(&mut self, k: KeyEvent) -> bool {
        let rows = self.rows();
        if rows.is_empty() {
            match k.code {
                KeyCode::Char('q') => return self.quit(),
                KeyCode::Esc => self.clear_filter(),
                KeyCode::Char('w') => self.open_review(self.tab),
                KeyCode::Char('/') => self.mode = Mode::Filter,
                _ => {}
            }
            return true;
        }
        self.cursor = self.cursor.min(rows.len() - 1);
        let path = rows[self.cursor].path.clone();
        let last = rows.len() - 1;
        match k.code {
            KeyCode::Char('q') => return self.quit(),
            KeyCode::Esc if !self.filter.is_empty() => self.clear_filter(),
            KeyCode::Esc => return self.quit(),
            KeyCode::Char('w') => self.open_review(self.tab),
            KeyCode::Char('s') if k.modifiers.contains(KeyModifiers::CONTROL) => self.open_review(self.tab),
            KeyCode::Char('X') => self.ask_discard(self.tab),
            KeyCode::Tab => self.switch_tab((self.tab + 1) % self.roots.len()),
            KeyCode::BackTab => self.switch_tab((self.tab + self.roots.len() - 1) % self.roots.len()),
            KeyCode::Char(c @ '1'..='9') => {
                let i = c as usize - '1' as usize;
                if i < self.roots.len() {
                    self.switch_tab(i);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.cursor = (self.cursor + 1).min(last),
            KeyCode::PageUp => self.cursor = self.cursor.saturating_sub(10),
            KeyCode::PageDown => self.cursor = (self.cursor + 10).min(last),
            KeyCode::Home | KeyCode::Char('g') => self.cursor = 0,
            KeyCode::End | KeyCode::Char('G') => self.cursor = last,
            KeyCode::Right | KeyCode::Char('l') => {
                let n = tree::get_mut(&mut self.roots, &path);
                if !n.children.is_empty() {
                    if n.open {
                        self.cursor = (self.cursor + 1).min(last);
                    } else {
                        n.open = true;
                    }
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                let n = tree::get_mut(&mut self.roots, &path);
                if n.open && !n.children.is_empty() && self.filter.is_empty() {
                    n.open = false;
                } else if path.len() > 1 {
                    let parent = &path[..path.len() - 1];
                    if let Some(i) = rows.iter().position(|r| r.path == parent) {
                        self.cursor = i;
                    }
                }
            }
            KeyCode::Enter if !self.filter.is_empty() => self.jump(&path),
            KeyCode::Enter | KeyCode::Char(' ') => self.activate(&path),
            KeyCode::Char('e') => self.begin_edit(&path),
            KeyCode::Char('d') => self.reset(&path, true),
            KeyCode::Char('u') => self.reset(&path, false),
            KeyCode::Char('a') => self.open_menu(&path),
            KeyCode::Char('x') => match self.queue.undo_last() {
                Some(q) => self.msg = format!("unqueued: {}", q.command),
                None => self.msg = "no queued action".into(),
            },
            KeyCode::Char('c') => self.show_changes = !self.show_changes,
            KeyCode::Char('m') => {
                self.mouse = !self.mouse;
                self.msg = if self.mouse { "mouse on" } else { "mouse off: the terminal selects text" }.into();
            }
            KeyCode::Char('/') => self.mode = Mode::Filter,
            KeyCode::Char('?') => self.mode = Mode::Help,
            _ => {}
        }
        true
    }

    /// Handle one mouse event against the last frame's hits. A click reaches
    /// the same paths a key does, so the confirm and secret rules hold.
    /// During text, secret or filter entry and a confirm, only the confirm's
    /// answers respond.
    fn mouse(&mut self, m: MouseEvent) {
        let at = Position::new(m.column, m.row);
        let wheel = match m.kind {
            MouseEventKind::ScrollUp => Some(KeyCode::Up),
            MouseEventKind::ScrollDown => Some(KeyCode::Down),
            _ => None,
        };
        let click = m.kind == MouseEventKind::Down(MouseButton::Left);
        let press = |c| KeyEvent::new(c, KeyModifiers::NONE);
        if let Mode::Flow(flow) = &mut self.mode {
            let ev = match (wheel, click) {
                (Some(k), _) => flow.key(press(k)),
                (None, true) => flow.click(at),
                _ => FlowEvent::Stay,
            };
            self.flow_event(ev);
            return;
        }
        match &mut self.mode {
            Mode::Browse => {
                if let Some(k) = wheel {
                    self.browse(press(k));
                } else if click {
                    self.click_browse(at);
                }
            }
            Mode::Menu { sel, .. } => {
                if let Some(k) = wheel {
                    self.key(press(k));
                } else if click {
                    let Some((menu, items)) = &self.hits.menu else { return };
                    if let Some(i) = items.iter().position(|r| r.contains(at)) {
                        *sel = i;
                        self.key(press(KeyCode::Enter));
                    } else if !menu.contains(at) {
                        self.key(press(KeyCode::Esc));
                    }
                }
            }
            Mode::Review { run: Some(_), .. } => {}
            Mode::Confirm { .. } | Mode::DiscardTab { .. } | Mode::Guard { confirm: true } | Mode::Review { discard: true, .. } if click => {
                if let Some(&(_, yes)) = self.hits.answers.iter().find(|(r, _)| r.contains(at)) {
                    self.key(press(KeyCode::Char(if yes { 'y' } else { 'n' })));
                }
            }
            Mode::Review { discard: false, .. } => {
                if let Some(k) = wheel {
                    self.key(press(k));
                } else if click {
                    self.click_review(at);
                }
            }
            Mode::Guard { confirm: false } if click => self.click_button(at),
            Mode::Help if click => self.mode = Mode::Browse,
            _ => {}
        }
    }

    /// A click on a button is the key it stands for.
    fn ask_discard(&mut self, tab: usize) {
        if self.pending_in(tab) == 0 {
            self.msg = format!("nothing pending in {}", self.roots[tab].name);
        } else {
            self.mode = Mode::DiscardTab { tab };
        }
    }

    fn click_button(&mut self, at: Position) {
        if let Some(&(_, b)) = self.hits.buttons.iter().find(|(r, _)| r.contains(at)) {
            self.key(KeyEvent::new(b.key(), KeyModifiers::NONE));
        }
    }

    /// A click in review: a bar target, a tab, or a row, which takes the
    /// cursor; a click on a group's marker, or on the selected group, opens or
    /// closes it. Nothing a click does edits or queues.
    fn click_review(&mut self, at: Position) {
        let Mode::Review { tab, .. } = self.mode else { return };
        if let Some(&(_, t)) = self.hits.discard_tabs.iter().find(|(r, _)| r.contains(at)) {
            self.review_to(Some(t));
            self.key(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE));
            return;
        }
        if let Some(&(_, t)) = self.hits.tabs.iter().find(|(r, _)| r.contains(at)) {
            return self.review_to(Some(t));
        }
        if self.hits.buttons.iter().any(|(r, _)| r.contains(at)) {
            return self.click_button(at);
        }
        let list = self.hits.list;
        if !list.contains(at) {
            return;
        }
        let rows = self.review_view(tab);
        let i = self.list.offset() + (at.y - list.y) as usize;
        let Some(row) = rows.get(i) else { return };
        let marker = list.x + 1 + review::GUTTER + 2 * row.depth as u16;
        if row.toggles() && (marker..marker + 2).contains(&at.x) {
            if !self.closed.remove(&row.path) {
                self.closed.insert(row.path.clone());
            }
            self.rcursor[tab] = i;
        } else if i == self.rcursor[tab] {
            self.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        } else {
            self.rcursor[tab] = i;
        }
    }

    /// A click while browsing: the call to action opens the review; a tab
    /// switches to it; a row selects it, and a second click on the selected
    /// row acts as Enter; a group's marker opens or closes it.
    fn click_browse(&mut self, at: Position) {
        if self.hits.cta.contains(at) {
            return self.open_review(self.tab);
        }
        if let Some(&(_, tab)) = self.hits.discard_tabs.iter().find(|(r, _)| r.contains(at)) {
            return self.ask_discard(tab);
        }
        if let Some(&(_, tab)) = self.hits.tabs.iter().find(|(r, _)| r.contains(at)) {
            return self.switch_tab(tab);
        }
        let list = self.hits.list;
        if !list.contains(at) {
            return;
        }
        let i = self.list.offset() + (at.y - list.y) as usize;
        let rows = self.rows();
        let Some(row) = rows.get(i) else { return };
        // The selection mark takes one column, then two per level of depth.
        let marker = list.x + 1 + 2 * row.depth as u16;
        let n = tree::get_mut(&mut self.roots, &row.path);
        if self.filter.is_empty() && !n.children.is_empty() && (marker..marker + 2).contains(&at.x) {
            n.open = !n.open;
            self.cursor = i;
        } else if i == self.cursor {
            self.browse(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        } else {
            self.cursor = i;
        }
    }

    /// Enter: toggle a bool, cycle a choice, edit text and numbers, enter a
    /// secret masked, open the action menu of a node that only has actions, or
    /// open and close a group.
    fn activate(&mut self, path: &[usize]) {
        let n = tree::get_mut(&mut self.roots, path);
        if !n.actions.is_empty() && n.setting.as_ref().is_some_and(|s| matches!(s.kind, Kind::ReadOnly)) {
            return self.open_menu(path);
        }
        match n.setting.as_mut() {
            Some(s) => match &s.kind {
                Kind::Bool => {
                    s.value = if s.value == "true" { "false" } else { "true" }.into();
                    self.msg = format!("{} = {}", n.name, s.value);
                }
                Kind::Choice(opts) => {
                    let i = opts.iter().position(|o| *o == s.value).map_or(0, |i| (i + 1) % opts.len());
                    s.value = opts[i].clone();
                    self.msg = format!("{} = {}", n.name, s.value);
                }
                Kind::ReadOnly => self.msg = "read-only here; the detail pane names the command".into(),
                Kind::Secret => match n.actions.iter().position(|a| matches!(a.arg, Arg::Secret)) {
                    Some(action) => self.mode = Mode::Secret { path: path.to_vec(), action, buf: SecretBuf::default() },
                    None => self.msg = "no way to set this here".into(),
                },
                _ => self.begin_edit(path),
            },
            None => n.open = !n.open,
        }
    }

    fn begin_edit(&mut self, path: &[usize]) {
        let n = tree::get(&self.roots, path);
        match &n.setting {
            Some(s) if s.kind.editable() => self.mode = Mode::Edit(s.value.clone()),
            _ => self.msg = "nothing to edit".into(),
        }
    }

    /// The row's actions, or the tab's when the row has none.
    fn open_menu(&mut self, path: &[usize]) {
        let own = !tree::get(&self.roots, path).actions.is_empty();
        let path = if own { path } else { &path[..1] };
        if tree::get(&self.roots, path).actions.is_empty() {
            self.msg = "no actions here".into();
        } else {
            self.mode = Mode::Menu { path: path.to_vec(), sel: 0 };
        }
    }

    /// An action was chosen: ask for its argument, or stage it at once.
    fn pick(&mut self, path: Vec<usize>, action: usize) {
        match tree::get(&self.roots, &path).actions[action].arg.clone() {
            Arg::None => self.stage(&path, action, ""),
            Arg::Text(_) => self.mode = Mode::Arg { path, action, buf: String::new() },
            Arg::Secret => self.mode = Mode::Secret { path, action, buf: SecretBuf::default() },
            Arg::Flow(name) => self.start_flow(&path, &name),
        }
    }

    /// Build the queued command; ask first when the action needs confirming.
    fn stage(&mut self, path: &[usize], action: usize, text: &str) {
        let a = &tree::get(&self.roots, path).actions[action];
        let queued = Queued { key: tree::key(&self.roots, path), label: a.label.clone(), command: a.render(text), confirm: a.confirm };
        if a.confirm {
            self.mode = Mode::Confirm { queued };
        } else {
            self.enqueue(queued);
        }
    }

    fn enqueue(&mut self, q: Queued) {
        self.msg = format!("queued: {}", q.command);
        self.queue.push(q);
    }

    fn commit(&mut self, buf: &str) {
        let rows = self.rows();
        let path = rows[self.cursor.min(rows.len() - 1)].path.clone();
        let n = tree::get_mut(&mut self.roots, &path);
        let s = n.setting.as_mut().expect("edit mode only on settings");
        match s.validate(buf) {
            Ok(v) => {
                s.value = v;
                self.msg = format!("{} = {}", n.name, s.value);
            }
            Err(e) => {
                self.msg = format!("rejected: {e}");
                self.mode = Mode::Edit(buf.to_string());
            }
        }
    }

    fn reset(&mut self, path: &[usize], to_default: bool) {
        let n = tree::get_mut(&mut self.roots, path);
        let Some(s) = n.setting.as_mut().filter(|s| s.kind.editable()) else {
            self.msg = "nothing to reset".into();
            return;
        };
        if to_default {
            match &s.default {
                Some(d) => {
                    s.value = d.clone();
                    self.msg = format!("{} = {} (default)", n.name, d);
                }
                None => self.msg = "no default".into(),
            }
        } else {
            s.value = s.loaded.clone();
            self.msg = format!("{} reverted", n.name);
        }
    }
}

#[cfg(test)]
mod flow_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{Action, Setting};
    use ratatui::backend::TestBackend;
    use ratatui::style::Modifier;
    use ratatui::Terminal;

    fn press(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    fn keys(app: &mut App, ks: &[KeyCode]) {
        for k in ks {
            assert!(app.key(press(*k)));
        }
    }

    fn type_str(app: &mut App, s: &str) {
        for c in s.chars() {
            assert!(app.key(press(KeyCode::Char(c))));
        }
    }

    fn screen(app: &mut App) -> String {
        let mut term = Terminal::new(TestBackend::new(110, 24)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        term.backend().buffer().content().iter().map(|c| c.symbol()).collect()
    }

    /// A key node with a secret and a destructive action, and a plain target.
    fn action_tree() -> Vec<Node> {
        let key = Node::leaf("anthropic", "", Setting::new(Kind::Secret, "absent", "keys/")).with_actions(vec![
            Action::new("set", "ways agent key add --provider anthropic").arg(Arg::Secret),
            Action::new("remove", "ways agent key remove --provider anthropic").confirm(),
            Action::new("check", "ways agent key check --provider anthropic"),
        ]);
        vec![Node::group("keys", "", vec![key]).opened()]
    }

    #[test]
    fn renders_and_edits_through_keys() {
        let roots = vec![Node::group(
            "matching",
            "",
            vec![Node::leaf("tau_s", "doc", Setting::new(Kind::Float { min: 0.0, max: 1.0 }, "0.5", "default").default("0.5"))],
        )];
        let mut app = App::new("t", roots);
        keys(&mut app, &[KeyCode::Right, KeyCode::Down, KeyCode::Enter, KeyCode::Backspace, KeyCode::Char('4'), KeyCode::Enter]);
        assert_eq!(tree::changes(&app.roots)[0].3, "0.4");
        assert!(screen(&mut app).contains("tau_s"));
    }

    #[test]
    fn typed_secret_never_reaches_the_screen_the_pane_or_the_summary() {
        const SECRET: &str = "Q#Z9@X!";
        let mut app = App::new("t", action_tree());
        keys(&mut app, &[KeyCode::Down, KeyCode::Enter]);
        type_str(&mut app, SECRET);
        let masked = screen(&mut app);
        assert!(masked.contains('•'));
        assert!(!masked.contains(SECRET) && !masked.chars().any(|c| SECRET.contains(c)), "typed characters reached the frame");

        keys(&mut app, &[KeyCode::Enter]);
        assert_eq!(app.queue.len(), 1);
        keys(&mut app, &[KeyCode::Char('c')]);
        let pane = screen(&mut app);
        assert!(pane.contains("<stdin>"));
        assert!(!pane.chars().any(|c| SECRET.contains(c)), "typed characters reached the pending pane");

        let out = crate::summary(&app.roots, &app.queue);
        assert!(out.contains("ways agent key add --provider anthropic < <stdin>"));
        assert!(!out.chars().any(|c| SECRET.contains(c)), "typed characters reached the summary");
        assert!(!format!("{:?}", app.queue).contains(SECRET));
    }

    #[test]
    fn destructive_action_is_queued_only_on_yes() {
        let mut app = App::new("t", action_tree());
        let to_remove = [KeyCode::Down, KeyCode::Char('a'), KeyCode::Down, KeyCode::Enter];
        keys(&mut app, &to_remove);
        keys(&mut app, &[KeyCode::Char('n')]);
        assert!(app.queue.is_empty());
        keys(&mut app, &to_remove);
        keys(&mut app, &[KeyCode::Char('y')]);
        assert_eq!(app.queue.items()[0].command, "ways agent key remove --provider anthropic");
    }

    #[test]
    fn queue_keeps_order_and_x_undoes_the_last() {
        let mut app = App::new("t", action_tree());
        // check, then remove (confirmed); undo drops remove, leaving check.
        keys(&mut app, &[KeyCode::Down, KeyCode::Char('a'), KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
        keys(&mut app, &[KeyCode::Char('a'), KeyCode::Down, KeyCode::Enter, KeyCode::Char('y')]);
        let cmds: Vec<_> = app.queue.items().iter().map(|q| q.label.as_str()).collect();
        assert_eq!(cmds, ["check", "remove"]);
        keys(&mut app, &[KeyCode::Char('x')]);
        assert_eq!(app.queue.items().len(), 1);
        assert_eq!(app.queue.items()[0].label, "check");
    }

    #[test]
    fn action_only_node_opens_the_menu_on_enter() {
        let target = Node::leaf("/home/me/.claude", "", Setting::new(Kind::ReadOnly, "enabled", "user"))
            .with_actions(vec![Action::new("plan", "ways config target plan {}").arg(Arg::Text("dir".into()))]);
        let mut app = App::new("t", vec![Node::group("install", "", vec![target]).opened()]);
        keys(&mut app, &[KeyCode::Enter, KeyCode::Enter]);
        type_str(&mut app, "/tmp/a b");
        keys(&mut app, &[KeyCode::Enter]);
        assert_eq!(app.queue.items()[0].command, "ways config target plan '/tmp/a b'");
    }

    fn flag(name: &str) -> Node {
        Node::leaf(name, "", Setting::new(Kind::Bool, "true", "default").default("true"))
    }

    /// Three tabs: nested ways, a number, and a node with an action.
    fn tabbed() -> Vec<Node> {
        let ways = Node::group("ways", "", vec![flag("alpha"), flag("beta"), Node::group("deep", "", vec![flag("gamma")])]);
        let tau = |name| Node::leaf(name, "", Setting::new(Kind::Float { min: 0.0, max: 1.0 }, "0.5", "default").default("0.5"));
        let matching = Node::group("matching", "", vec![tau("tau_s"), tau("tau_k")]);
        let gate = Node::group("gate", "", vec![flag("language")]).with_actions(vec![Action::new("check", "ways agent status")]);
        vec![ways.opened(), matching.opened(), gate.opened()]
    }

    fn frame(app: &mut App, w: u16, h: u16) -> Vec<String> {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let buf = term.backend().buffer();
        buf.content().chunks(w as usize).map(|r| r.iter().map(|c| c.symbol()).collect()).collect()
    }

    #[test]
    fn tab_bar_names_every_root_and_tab_keys_switch_the_tree() {
        let mut app = App::new("t", tabbed());
        let bar = frame(&mut app, 100, 24)[0].clone();
        assert!(["1 ways", "2 matching", "3 gate"].iter().all(|t| bar.contains(t)), "{bar}");
        assert!(screen(&mut app).contains("alpha") && !screen(&mut app).contains("tau_s"));
        keys(&mut app, &[KeyCode::Tab]);
        assert!(screen(&mut app).contains("tau_s") && !screen(&mut app).contains("alpha"));
        keys(&mut app, &[KeyCode::Char('3')]);
        assert!(screen(&mut app).contains("language"));
        keys(&mut app, &[KeyCode::BackTab, KeyCode::BackTab]);
        assert!(screen(&mut app).contains("alpha"));
        keys(&mut app, &[KeyCode::Char('9')]);
        assert!(screen(&mut app).contains("alpha"), "a digit past the last tab is ignored");
    }

    #[test]
    fn each_tab_keeps_its_cursor_and_open_state() {
        let mut app = App::new("t", tabbed());
        // On ways: move the cursor to `deep` at row 2 and open it.
        keys(&mut app, &[KeyCode::Down, KeyCode::Down, KeyCode::Right]);
        assert_eq!(app.cursor, 2);
        assert!(screen(&mut app).contains("gamma"));
        keys(&mut app, &[KeyCode::Char('2'), KeyCode::Down]);
        assert_eq!(app.cursor, 1);
        keys(&mut app, &[KeyCode::Char('1')]);
        assert_eq!(app.cursor, 2);
        assert!(screen(&mut app).contains("gamma"), "open state kept");
        keys(&mut app, &[KeyCode::Char('2')]);
        assert_eq!(app.cursor, 1);
    }

    #[test]
    fn filter_hit_in_another_tab_jumps_there_on_enter() {
        let mut app = App::new("t", tabbed());
        keys(&mut app, &[KeyCode::Char('/')]);
        type_str(&mut app, "language");
        let hits = screen(&mut app);
        assert!(hits.contains("gate") && hits.contains("language") && !hits.contains("alpha"));
        keys(&mut app, &[KeyCode::Enter]);
        assert_eq!(app.filter, "language");
        keys(&mut app, &[KeyCode::Down, KeyCode::Enter]);
        assert!(app.filter.is_empty());
        assert_eq!(app.tab, 2);
        assert_eq!(tree::key(&app.roots, &app.rows()[app.cursor].path), "gate.language");
        // Esc from a filter returns to the tab's own cursor.
        keys(&mut app, &[KeyCode::Char('1'), KeyCode::Down, KeyCode::Char('/')]);
        type_str(&mut app, "tau");
        keys(&mut app, &[KeyCode::Esc]);
        assert_eq!((app.tab, app.cursor), (0, 1));
    }

    #[test]
    fn pending_count_shows_on_the_edited_tab_only() {
        // The plain shape: the badge segment abuts the label, no glyphs between.
        let mut app = App::new("t", tabbed()).shape(theme::Shape::PLAIN);
        keys(&mut app, &[KeyCode::Down, KeyCode::Enter]);
        let bar = frame(&mut app, 100, 24)[0].clone();
        assert!(bar.contains("1 ways  ●1") && !bar.contains("matching  ●") && !bar.contains("gate  ●"), "{bar}");
        keys(&mut app, &[KeyCode::Char('3'), KeyCode::Char('a'), KeyCode::Enter]);
        let bar = frame(&mut app, 100, 24)[0].clone();
        assert!(bar.contains("3 gate  ●1"), "{bar}");
        keys(&mut app, &[KeyCode::Char('c')]);
        assert!(screen(&mut app).contains("pending (1 changes, 1 actions)"));
    }

    #[test]
    fn minimum_size_fits_the_tab_bar_and_the_selected_row() {
        let mut app = App::new("t", tabbed());
        keys(&mut app, &[KeyCode::Down, KeyCode::Down]);
        let f = frame(&mut app, 80, 25);
        assert!(["1 ways", "2 matching", "3 gate"].iter().all(|t| f[0].contains(t)));
        assert!(f.iter().any(|l| l.contains("beta")), "{f:#?}");
        keys(&mut app, &[KeyCode::Char('?')]);
        frame(&mut app, 80, 25);
    }

    fn mouse_at(app: &mut App, kind: MouseEventKind, (x, y): (u16, u16)) {
        // Draw first, as the run loop does, so the hits are the frame's.
        frame(app, 110, 24);
        app.mouse(MouseEvent { kind, column: x, row: y, modifiers: KeyModifiers::NONE });
    }

    fn click(app: &mut App, at: (u16, u16)) {
        mouse_at(app, MouseEventKind::Down(MouseButton::Left), at);
    }

    /// Where `text` first appears on screen, as (column, row).
    fn find(app: &mut App, text: &str) -> (u16, u16) {
        let f = frame(app, 110, 24);
        for (y, line) in f.iter().enumerate() {
            if let Some(b) = line.find(text) {
                return (line[..b].chars().count() as u16, y as u16);
            }
        }
        panic!("{text} not on screen:\n{}", f.join("\n"));
    }

    #[test]
    fn mouse_clicks_a_tab_and_selects_then_activates_a_row() {
        let mut app = App::new("t", tabbed());
        let at = find(&mut app, "2 matching");
        click(&mut app, at);
        assert_eq!(app.tab, 1);
        let at = find(&mut app, "1 ways");
        click(&mut app, at);
        assert_eq!(app.tab, 0);

        let beta = find(&mut app, "beta");
        click(&mut app, beta);
        assert_eq!(app.cursor, 1);
        assert!(tree::changes(&app.roots).is_empty(), "the first click only selects");
        click(&mut app, beta);
        assert_eq!(tree::changes(&app.roots)[0].0, "ways.beta", "a second click acts as Enter");
    }

    #[test]
    fn mouse_marker_toggles_a_group_and_the_wheel_moves_the_cursor() {
        let mut app = App::new("t", tabbed());
        let (x, y) = find(&mut app, "▸ deep");
        click(&mut app, (x, y));
        assert!(screen(&mut app).contains("gamma"), "a click on ▸ opens the group");
        assert_eq!(app.cursor, 2);
        click(&mut app, (x + 1, y));
        assert!(!screen(&mut app).contains("gamma"), "the marker's second column closes it again");

        mouse_at(&mut app, MouseEventKind::ScrollUp, (0, 0));
        mouse_at(&mut app, MouseEventKind::ScrollUp, (0, 0));
        assert_eq!(app.cursor, 0);
        mouse_at(&mut app, MouseEventKind::ScrollDown, (0, 0));
        assert_eq!(app.cursor, 1);
    }

    #[test]
    fn mouse_picks_a_menu_item_and_a_click_outside_closes_it() {
        let mut app = App::new("t", action_tree());
        keys(&mut app, &[KeyCode::Down, KeyCode::Char('a')]);
        click(&mut app, (0, 23));
        assert!(matches!(app.mode, Mode::Browse) && app.queue.is_empty());
        keys(&mut app, &[KeyCode::Char('a')]);
        let at = find(&mut app, " check");
        click(&mut app, at);
        assert_eq!(app.queue.items()[0].label, "check");
    }

    #[test]
    fn mouse_answers_a_confirm_on_its_targets_only() {
        let mut app = App::new("t", action_tree());
        keys(&mut app, &[KeyCode::Down, KeyCode::Char('a')]);
        let at = find(&mut app, " remove");
        click(&mut app, at);
        assert!(matches!(app.mode, Mode::Confirm { .. }));
        click(&mut app, (1, 5));
        assert!(matches!(app.mode, Mode::Confirm { .. }), "a click off the answers does nothing");
        let at = find(&mut app, "n cancel");
        click(&mut app, at);
        assert!(app.queue.is_empty() && matches!(app.mode, Mode::Browse));

        keys(&mut app, &[KeyCode::Char('a'), KeyCode::Down, KeyCode::Enter]);
        let at = find(&mut app, "y queue");
        click(&mut app, at);
        assert_eq!(app.queue.items()[0].command, "ways agent key remove --provider anthropic");
    }

    #[test]
    fn mouse_during_secret_entry_neither_reveals_nor_queues() {
        const SECRET: &str = "Q#Z9@X!";
        let mut app = App::new("t", action_tree());
        keys(&mut app, &[KeyCode::Down, KeyCode::Enter]);
        type_str(&mut app, SECRET);
        // Every cell, clicked twice, and the wheel.
        for y in 0..24 {
            for x in 0..110 {
                click(&mut app, (x, y));
                click(&mut app, (x, y));
            }
        }
        mouse_at(&mut app, MouseEventKind::ScrollDown, (5, 5));
        let Mode::Secret { buf, .. } = &app.mode else { panic!("left secret entry") };
        assert_eq!(buf.len(), SECRET.chars().count());
        assert!(app.queue.is_empty());
        assert!(!screen(&mut app).chars().any(|c| SECRET.contains(c)), "typed characters reached the frame");
    }

    #[test]
    fn the_look_follows_the_statusline_palette() {
        let mut app = App::new("t", tabbed());
        keys(&mut app, &[KeyCode::Down, KeyCode::Enter]);
        let mut term = Terminal::new(TestBackend::new(80, 25)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let buf = term.backend().buffer();
        let row = |y: u16| (0..80).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>();
        let at = |text: &str, y: u16| {
            let r = row(y);
            &buf[(r[..r.find(text).unwrap_or_else(|| panic!("{text} not in {r}"))].chars().count() as u16, y)]
        };

        // The tab bar: a round cap, the shown tab on the accent, its count on a warn badge.
        assert_eq!(buf[(0, 0)].symbol(), theme::Shape::ROUND.cap);
        assert_eq!((at("1 ways", 0).fg, at("1 ways", 0).bg), (theme::INK, theme::ACCENT));
        assert_eq!(at("●1", 0).bg, theme::WARN);
        assert_eq!(at("2 matching", 0).bg, theme::ACCENT_DIM);
        // The selected row keeps the changed value's colour over the accent shade.
        let y = (0..25).find(|&y| row(y).contains(theme::SELECTED_MARK)).unwrap();
        let v = at("false", y);
        assert_eq!((v.fg, v.bg), (theme::WARN, theme::ACCENT_SHADE));
        assert!(v.modifier.contains(Modifier::BOLD));
        // The status line: a mode lozenge, then the count in the changed style.
        assert_eq!(at("browse", 24).bg, theme::ACCENT);
        assert_eq!(at("●1 changed", 24).fg, theme::WARN);
    }

    #[test]
    fn a_tab_starts_at_its_roots_children() {
        let mut app = App::new("t", tabbed());
        let tree = frame(&mut app, 100, 24);
        assert!(tree.iter().all(|l| !l.contains("▾ ways")), "the root has no row");
        assert_eq!(app.rows().len(), 3);
        assert_eq!(tree::key(&app.roots, &app.rows()[0].path), "ways.alpha");
    }

    #[test]
    fn group_and_tab_badges_both_count_values_and_queued_actions() {
        let mut app = App::new("t", tabbed());
        // ways.deep.gamma changed, and a command queued under ways.deep.
        keys(&mut app, &[KeyCode::Down, KeyCode::Down, KeyCode::Right, KeyCode::Down, KeyCode::Enter]);
        app.queue.push(Queued { key: "ways.deep".into(), label: "x".into(), command: "ways x".into(), confirm: false });
        let f = frame(&mut app, 100, 24);
        let deep = f.iter().find(|l| l.contains("▾ deep")).unwrap();
        assert!(deep.contains("●2"), "{deep}");
        assert!(f[0].contains("●2"), "{}", f[0]);
    }

    /// Three tabs. `ways` has three bools in two files and a key node with
    /// actions; `other` has one bool in a third file; `clean` has nothing to change.
    fn pending_tree() -> Vec<Node> {
        let s = |file: &str, key: &str| Setting::new(Kind::Bool, "true", "user").default("true").store("user", file.into(), key);
        let key = action_tree().remove(0).children.remove(0);
        let ways = Node::group(
            "ways",
            "",
            vec![
                Node::leaf("alpha", "", s("/c/a.yaml", "alpha")),
                Node::leaf("beta", "", s("/c/a.yaml", "beta")),
                Node::leaf("gamma", "", s("/c/b.yaml", "gamma")),
                key,
            ],
        );
        let other = Node::group("other", "", vec![Node::leaf("delta", "", s("/c/c.yaml", "delta"))]);
        vec![ways.opened(), other.opened(), Node::group("clean", "", vec![flag("zeta")]).opened()]
    }

    /// In `ways`: alpha, beta and gamma toggled; `check` queued, then `remove`
    /// (confirmed). In `other`: delta toggled. Ends on `ways`: 5 and 1 pending.
    fn pending_app() -> App {
        let mut app = App::new("t", pending_tree());
        keys(&mut app, &[KeyCode::Enter, KeyCode::Down, KeyCode::Enter, KeyCode::Down, KeyCode::Enter, KeyCode::Down]);
        keys(&mut app, &[KeyCode::Char('a'), KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
        keys(&mut app, &[KeyCode::Char('a'), KeyCode::Down, KeyCode::Enter, KeyCode::Char('y')]);
        keys(&mut app, &[KeyCode::Char('2'), KeyCode::Enter, KeyCode::Char('1')]);
        app
    }

    /// Tick until the run has ended and review has closed it out.
    fn tick_out(app: &mut App) {
        for _ in 0..60 {
            if !matches!(app.mode, Mode::Review { run: Some(_), .. }) {
                return;
            }
            app.tick();
        }
        panic!("the run never ended");
    }

    /// Tick until the run has ended, but not past it: the stopped or finished
    /// frame is still on screen.
    fn tick_to_end(app: &mut App) {
        for _ in 0..60 {
            if matches!(&app.mode, Mode::Review { run: Some(r), .. } if r.finished()) {
                return;
            }
            app.tick();
        }
        panic!("the run never ended");
    }

    fn bottom(app: &mut App, w: u16, h: u16) -> String {
        frame(app, w, h)[h as usize - 1].clone()
    }

    fn delta_changed(app: &App) -> bool {
        app.roots[1].children[0].setting.as_ref().unwrap().changed()
    }

    fn review_tab(app: &App) -> Option<usize> {
        match &app.mode {
            Mode::Review { tab, .. } => Some(*tab),
            _ => None,
        }
    }

    /// The tree rows of the screen's left pane and its detail pane, as text.
    fn panes(app: &mut App) -> (String, String) {
        let f = frame(app, 110, 24);
        let split = 61;
        let cut = |from: usize, to: usize| f[1..23].iter().map(|l| l.chars().skip(from).take(to - from).collect::<String>()).collect::<Vec<_>>().join("\n");
        (cut(0, split), cut(split, 110))
    }

    #[test]
    fn the_call_to_action_shows_while_the_current_tab_has_pending_items() {
        let mut app = App::new("t", pending_tree());
        assert!(!bottom(&mut app, 100, 24).contains("unsaved"));
        keys(&mut app, &[KeyCode::Enter]);
        assert!(bottom(&mut app, 100, 24).contains("● 1 unsaved in ways · w review & apply"));
        keys(&mut app, &[KeyCode::Enter]);
        assert!(!bottom(&mut app, 100, 24).contains("unsaved"), "reverting the value clears it");
        app.queue.push(Queued { key: "ways.anthropic".into(), label: "x".into(), command: "ways x".into(), confirm: false });
        assert!(bottom(&mut app, 100, 24).contains("● 1 unsaved in ways"), "a queued action alone counts");
        app.queue.clear();
        assert!(!bottom(&mut app, 100, 24).contains("unsaved"));
        app.queue.push(Queued { key: "other".into(), label: "x".into(), command: "ways x".into(), confirm: false });
        assert!(!bottom(&mut app, 100, 24).contains("unsaved"), "pending in another tab is shown on its badge only");
        assert!(frame(&mut app, 100, 24)[0].contains("●1 ↺"));
        keys(&mut app, &[KeyCode::Char('2')]);
        assert!(bottom(&mut app, 100, 24).contains("● 1 unsaved in other"));
    }

    #[test]
    fn the_call_to_action_is_bold_hot_and_clicking_it_enters_review() {
        let mut app = pending_app();
        let (x, y) = find(&mut app, "● 5 unsaved in ways");
        let mut term = Terminal::new(TestBackend::new(110, 24)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let c = &term.backend().buffer()[(x + 2, y)];
        assert_eq!(c.bg, theme::HOT);
        assert!(c.modifier.contains(Modifier::BOLD));
        click(&mut app, (x + 3, y));
        assert_eq!(review_tab(&app), Some(0));
        let mut app = pending_app();
        keys(&mut app, &[KeyCode::Char('w')]);
        assert_eq!(review_tab(&app), Some(0));
        let mut app = pending_app();
        assert!(app.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)));
        assert!(matches!(app.mode, Mode::Review { .. }));
        let mut app = App::new("t", pending_tree());
        keys(&mut app, &[KeyCode::Char('w')]);
        assert!(matches!(app.mode, Mode::Browse), "nothing to review");
    }

    #[test]
    fn review_keeps_the_browsers_layout_and_shows_only_what_is_pending() {
        let mut app = pending_app();
        keys(&mut app, &[KeyCode::Char('w')]);
        let f = frame(&mut app, 100, 30);
        assert!(["1 ways", "2 other", "3 clean"].iter().all(|t| f[0].contains(t)), "the tab bar stays: {}", f[0]);
        assert!(f[1].contains("review"), "the tree pane is titled review: {}", f[1]);
        assert!(f.iter().any(|l| l.contains("detail")), "the detail pane stays");
        let bar = &f[29];
        assert!(bar.contains("review") && !bar.contains("browse"), "{bar}");
        assert!(bar.contains("a apply ways · X discard ways · Esc back") || ["a apply ways", "X discard ways", "Esc back"].iter().all(|t| bar.contains(t)), "{bar}");
        let (tree, _) = panes(&mut app);
        let at = |t: &str| tree.find(t).unwrap_or_else(|| panic!("{t} missing:\n{tree}"));
        assert!(at("alpha") < at("beta") && at("beta") < at("gamma") && at("gamma") < at("queued"));
        assert!(at("queued") < at("1. $ ways agent key check") && at("1. $ ways agent key check") < at("2. $ ways agent key remove ") + 3);
        let remove = tree.lines().find(|l| l.contains("key remove")).unwrap();
        assert!(remove.contains('▲') && !tree.lines().find(|l| l.contains("key check")).unwrap().contains('▲'));
        assert!(!tree.contains("delta") && !tree.contains("a.yaml") && tree.lines().filter(|l| l.contains("anthropic")).all(|l| l.contains('$')), "unchanged and other tabs' rows stay out:\n{tree}");
        assert!(tree.contains("true → false"));
    }

    #[test]
    fn a_group_over_a_change_is_listed_with_its_ancestors_open_and_toggles_on_enter() {
        let mut app = App::new("t", tabbed());
        keys(&mut app, &[KeyCode::Down, KeyCode::Down, KeyCode::Right, KeyCode::Down, KeyCode::Enter]);
        keys(&mut app, &[KeyCode::Char('w')]);
        let (tree, detail) = panes(&mut app);
        assert!(tree.contains("▾ deep") && tree.contains("gamma") && !tree.contains("alpha"), "{tree}");
        keys(&mut app, &[KeyCode::Enter]);
        assert!(!panes(&mut app).0.contains("gamma"), "Enter on a group closes it");
        assert!(tree.contains("●1") && detail.contains("1 change under this group"), "{detail}");
        keys(&mut app, &[KeyCode::Right, KeyCode::Right]);
        assert!(panes(&mut app).0.contains("gamma"), "→ opens it, and a second → steps in");
        assert_eq!(app.rcursor[0], 1);
        assert_eq!(tree::changes(&app.roots).len(), 1, "toggling a group changes no value");
    }

    #[test]
    fn tab_and_digits_skip_clean_tabs_which_are_dimmed_but_clickable() {
        let mut app = pending_app();
        keys(&mut app, &[KeyCode::Char('w')]);
        let mut term = Terminal::new(TestBackend::new(110, 24)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let buf = term.backend().buffer();
        let bar: String = (0..110).map(|x| buf[(x, 0)].symbol().to_string()).collect();
        let at = |t: &str| &buf[(bar[..bar.find(t).unwrap()].chars().count() as u16, 0)];
        assert_eq!((at("3 clean").fg, at("3 clean").bg), (theme::MUTED, theme::DIM_TAB), "a clean tab is dimmed");
        assert_eq!(at("2 other").bg, theme::ACCENT_DIM);
        assert_eq!(at("1 ways").bg, theme::ACCENT);

        keys(&mut app, &[KeyCode::Tab]);
        assert_eq!(review_tab(&app), Some(1));
        keys(&mut app, &[KeyCode::Tab]);
        assert_eq!(review_tab(&app), Some(0), "Tab skips the clean tab");
        keys(&mut app, &[KeyCode::BackTab]);
        assert_eq!(review_tab(&app), Some(1), "so does Shift-Tab");
        keys(&mut app, &[KeyCode::Char('3')]);
        assert_eq!(review_tab(&app), Some(1), "a digit onto a clean tab does nothing");
        assert!(bottom(&mut app, 110, 24).contains("nothing pending in clean"));
        keys(&mut app, &[KeyCode::Char('1')]);
        assert_eq!(review_tab(&app), Some(0));

        let at = find(&mut app, "3 clean");
        click(&mut app, at);
        assert_eq!(review_tab(&app), Some(2));
        let (tree, detail) = panes(&mut app);
        assert!(tree.contains("nothing pending in clean") && detail.contains("nothing pending in clean"), "{tree}");
        assert!(matches!(app.mode, Mode::Review { .. }) && app.pending_in(0) == 5);
    }

    #[test]
    fn each_tab_keeps_its_own_review_cursor() {
        let mut app = pending_app();
        keys(&mut app, &[KeyCode::Char('w'), KeyCode::Down, KeyCode::Down]);
        keys(&mut app, &[KeyCode::Tab, KeyCode::Tab]);
        assert_eq!((review_tab(&app), app.rcursor[0]), (Some(0), 2));
        keys(&mut app, &[KeyCode::Char('G')]);
        assert_eq!(app.rcursor[0], 5);
        keys(&mut app, &[KeyCode::Tab, KeyCode::Tab]);
        assert_eq!(app.rcursor[0], 5);
    }

    #[test]
    fn editing_keys_do_nothing_in_review_and_say_so() {
        let mut app = pending_app();
        keys(&mut app, &[KeyCode::Char('w')]);
        let values = |app: &App| tree::changes(&app.roots).iter().map(|c| format!("{}={}", c.0, c.3)).collect::<Vec<_>>();
        let (before, queued) = (values(&app), app.queue.items().iter().map(|q| q.command.clone()).collect::<Vec<_>>());
        // Every row in turn: a value, then the group, then the commands.
        for row in 0..6 {
            app.rcursor[0] = row;
            for c in ['e', 'd', 'u', 'x', 'c', 'w', 'z', '5'] {
                if c == '5' {
                    keys(&mut app, &[KeyCode::Backspace, KeyCode::Char('5')]);
                } else {
                    keys(&mut app, &[KeyCode::Char(c)]);
                }
            }
            if row != 3 {
                keys(&mut app, &[KeyCode::Enter, KeyCode::Char(' ')]);
                assert!(bottom(&mut app, 110, 24).contains("read-only in review; Esc to edit"), "row {row}");
            }
            assert!(matches!(app.mode, Mode::Review { run: None, discard: false, .. }), "row {row}");
            assert_eq!(values(&app), before, "row {row}");
            assert_eq!(app.queue.items().iter().map(|q| q.command.clone()).collect::<Vec<_>>(), queued, "row {row}");
        }
        // The group is the one thing Enter does: it hides its commands.
        app.rcursor[0] = 3;
        keys(&mut app, &[KeyCode::Enter]);
        assert!(!panes(&mut app).0.contains("key check"));
        assert_eq!((values(&app), app.queue.len()), (before, 2));
    }

    #[test]
    fn detail_for_a_setting_shows_was_will_be_layers_file_and_key() {
        let mut app = pending_app();
        keys(&mut app, &[KeyCode::Char('w'), KeyCode::Down]);
        let (_, detail) = panes(&mut app);
        for t in ["ways.beta", "change", "true → false", "from      user layer", "to        user layer", "file      /c/a.yaml", "key       beta"] {
            assert!(detail.contains(t), "{t} missing:\n{detail}");
        }
        let (x, y) = find(&mut app, "change    true");
        let mut term = Terminal::new(TestBackend::new(110, 24)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let buf = term.backend().buffer();
        let (was, will) = (&buf[(x + 10, y)], &buf[(x + 10 + 7, y)]);
        assert!(was.modifier.contains(Modifier::CROSSED_OUT) && was.fg == theme::MUTED, "the old value is struck and muted");
        assert!(will.modifier.contains(Modifier::BOLD) && will.fg == theme::OK, "the new value is green and bold");
    }

    #[test]
    fn detail_for_a_group_counts_its_changes_and_names_the_files() {
        let mut app = App::new("t", pending_tree());
        // ways.deep holds a change in another file.
        app.roots[0].children.push(Node::group("deep", "", vec![Node::leaf("eps", "", Setting::new(Kind::Bool, "false", "user").store("project", "/p/x.yaml".into(), "eps"))]));
        app.roots[0].children[4].children[0].setting.as_mut().unwrap().value = "true".into();
        app.roots[0].children[4].open = true;
        keys(&mut app, &[KeyCode::Char('w'), KeyCode::Char('G')]);
        app.rcursor[0] = app.review_view(0).iter().position(|r| r.name == "deep").unwrap();
        let (_, detail) = panes(&mut app);
        assert!(detail.contains("1 change under this group") && detail.contains("file it writes") && detail.contains("/p/x.yaml"), "{detail}");
        assert!(!detail.contains("/c/a.yaml"), "only the files under it:\n{detail}");
    }

    #[test]
    fn detail_for_an_action_shows_the_command_with_stdin_for_a_secret_and_never_the_typed_text() {
        const SECRET: &str = "ß#¤@%^";
        let mut app = App::new("t", action_tree());
        keys(&mut app, &[KeyCode::Enter]);
        type_str(&mut app, SECRET);
        keys(&mut app, &[KeyCode::Enter]);
        // Queue a confirmed action too.
        keys(&mut app, &[KeyCode::Char('a'), KeyCode::Down, KeyCode::Enter, KeyCode::Char('y')]);
        keys(&mut app, &[KeyCode::Char('w'), KeyCode::Down]);
        let (_, detail) = panes(&mut app);
        for t in ["$ ways agent key add --provider anthropic", "<stdin>", "queued by", "keys.anthropic", "asks      no"] {
            assert!(detail.contains(t), "{t} missing:\n{detail}");
        }
        keys(&mut app, &[KeyCode::Down]);
        let (_, detail) = panes(&mut app);
        assert!(detail.contains("$ ways agent key remove") && detail.contains("asks      yes: it is destructive or reconciles"), "{detail}");
        let all = frame(&mut app, 110, 24).join("\n");
        assert!(!all.chars().any(|c| SECRET.contains(c)), "typed characters reached the review");
        assert!(!format!("{:?}", app.queue).contains(SECRET));
    }

    #[test]
    fn an_action_detail_carries_its_doc_and_the_files_it_touches() {
        let a = Action::new("remove", "ways x remove").confirm().doc("Deletes the thing.").touches("/keys/x");
        let node = Node::leaf("x", "", Setting::new(Kind::ReadOnly, "on", "user")).with_actions(vec![a.clone()]);
        let mut app = App::new("t", vec![Node::group("g", "", vec![node]).opened()]);
        app.queue.push(Queued { key: "g.x".into(), label: a.label.clone(), command: a.render(""), confirm: true });
        keys(&mut app, &[KeyCode::Char('w'), KeyCode::Down]);
        let (_, detail) = panes(&mut app);
        assert!(detail.contains("Deletes the thing.") && detail.contains("touches   /keys/x"), "{detail}");
    }

    #[test]
    fn discard_in_review_asks_first_and_takes_only_its_tab_then_review_moves_on() {
        let mut app = pending_app();
        keys(&mut app, &[KeyCode::Char('w'), KeyCode::Char('X')]);
        assert!(bottom(&mut app, 100, 24).contains("discard 5 pending in ways?"));
        keys(&mut app, &[KeyCode::Char('x'), KeyCode::Enter]);
        assert_eq!(app.pending_in(0), 5, "other keys do not answer");
        keys(&mut app, &[KeyCode::Char('n')]);
        assert!(matches!(app.mode, Mode::Review { discard: false, .. }) && app.pending_in(0) == 5);
        keys(&mut app, &[KeyCode::Char('X'), KeyCode::Char('y')]);
        assert_eq!((app.pending_in(0), app.pending_in(1)), (0, 1));
        assert_eq!(review_tab(&app), Some(1), "the next tab with pending");
        assert!(delta_changed(&app));
        keys(&mut app, &[KeyCode::Char('X'), KeyCode::Char('y')]);
        assert!(matches!(app.mode, Mode::Browse) && app.pending() == 0, "nothing left ends review");
    }

    #[test]
    fn the_bar_targets_are_clickable() {
        let mut app = pending_app();
        keys(&mut app, &[KeyCode::Char('w')]);
        let at = find(&mut app, "X discard ways");
        click(&mut app, at);
        assert!(matches!(app.mode, Mode::Review { discard: true, .. }));
        let at = find(&mut app, "n keep");
        click(&mut app, at);
        assert!(matches!(app.mode, Mode::Review { discard: false, .. }) && app.pending_in(0) == 5);
        let at = find(&mut app, "a apply ways");
        click(&mut app, at);
        assert!(matches!(app.mode, Mode::Review { run: Some(_), .. }));
        tick_out(&mut app);
        assert_eq!(app.pending_in(0), 0);

        let mut app = pending_app();
        keys(&mut app, &[KeyCode::Char('w')]);
        let at = find(&mut app, "X discard ways");
        click(&mut app, at);
        let at = find(&mut app, "y discard");
        click(&mut app, at);
        assert_eq!((app.pending_in(0), review_tab(&app)), (0, Some(1)));

        let mut app = pending_app();
        keys(&mut app, &[KeyCode::Char('w')]);
        let at = find(&mut app, "Esc back");
        click(&mut app, at);
        assert!(matches!(app.mode, Mode::Browse));
    }

    #[test]
    fn clicks_and_the_wheel_move_the_review_cursor_without_editing() {
        let mut app = pending_app();
        keys(&mut app, &[KeyCode::Char('w')]);
        let before = tree::changes(&app.roots).len();
        let at = find(&mut app, "gamma");
        click(&mut app, at);
        assert_eq!(app.rcursor[0], 2, "a click on a row moves the cursor");
        click(&mut app, at);
        assert_eq!(tree::changes(&app.roots).len(), before, "a second click on a value edits nothing");
        assert!(bottom(&mut app, 110, 24).contains("read-only in review"));
        let at = find(&mut app, "1. $ ways agent key check");
        click(&mut app, at);
        assert_eq!(app.rcursor[0], 4);
        assert_eq!(app.queue.len(), 2);
        // The marker of the `queued` group closes it.
        let (x, y) = find(&mut app, "▾ queued");
        click(&mut app, (x, y));
        assert!(!panes(&mut app).0.contains("key check"));
        click(&mut app, (x + 1, y));
        assert!(panes(&mut app).0.contains("key check"));
        mouse_at(&mut app, MouseEventKind::ScrollUp, (0, 0));
        mouse_at(&mut app, MouseEventKind::ScrollUp, (0, 0));
        assert_eq!(app.rcursor[0], 1, "the marker click put the cursor on the group, row 3");
        mouse_at(&mut app, MouseEventKind::ScrollDown, (0, 0));
        assert_eq!(app.rcursor[0], 2);
        assert_eq!((tree::changes(&app.roots).len(), app.queue.len()), (before, 2));
    }

    #[test]
    fn esc_returns_to_browse_with_the_browse_cursor_where_it_was() {
        let mut app = pending_app();
        keys(&mut app, &[KeyCode::Down, KeyCode::Down]);
        let at = (app.tab, app.cursor);
        keys(&mut app, &[KeyCode::Char('w'), KeyCode::Down, KeyCode::Down, KeyCode::Down, KeyCode::Tab]);
        keys(&mut app, &[KeyCode::Esc]);
        assert!(matches!(app.mode, Mode::Browse));
        assert_eq!((app.tab, app.cursor), at);
        assert!(bottom(&mut app, 100, 24).contains("browse"));
        assert_eq!(app.pending(), 6, "nothing was applied or dropped");
    }

    #[test]
    fn apply_ticks_gutter_glyphs_in_place_and_clears_only_the_current_tab() {
        let mut app = pending_app();
        keys(&mut app, &[KeyCode::Char('w'), KeyCode::Char('a')]);
        let tree = |app: &mut App| panes(app).0;
        let t = tree(&mut app);
        assert!(t.contains("·   alpha") && t.contains("·   gamma") && t.contains("·     1. $ ways agent key check"), "{t}");
        app.tick();
        let (t, detail) = panes(&mut app);
        assert!(t.contains("◐   alpha") && t.contains("◐   beta") && t.contains("·   gamma"), "{t}");
        assert!(detail.contains("step      would write /c/a.yaml (2 keys)") && detail.contains("ways.alpha"), "the detail follows the running row:\n{detail}");
        app.tick();
        let t = tree(&mut app);
        assert!(t.contains("✓   alpha") && t.contains("✓   beta") && t.contains("·   gamma"), "a finished row stays, marked:\n{t}");
        assert_eq!(app.pending_in(0), 3, "a finished write is applied at once");
        assert!(bottom(&mut app, 100, 24).contains("applying"));
        tick_to_end(&mut app);
        let t = tree(&mut app);
        assert!(["✓   alpha", "✓   gamma", "✓     1. $ ways agent key check", "✓     2. $ ways agent key remove"].iter().all(|g| t.contains(g)), "{t}");
        app.tick();
        assert_eq!((app.pending_in(0), app.pending_in(1)), (0, 1));
        assert!(delta_changed(&app));
        assert_eq!(review_tab(&app), Some(1), "review moves to the next tab with pending");
        let (t, _) = panes(&mut app);
        assert!(t.contains("delta") && !t.contains("alpha"), "{t}");
        assert!(bottom(&mut app, 100, 24).contains("applied 5 in ways"));

        keys(&mut app, &[KeyCode::Char('a')]);
        tick_out(&mut app);
        assert!(matches!(app.mode, Mode::Browse) && app.pending() == 0, "nothing else pending ends review");
        assert!(bottom(&mut app, 100, 24).contains("applied 1 in other"));
    }

    #[test]
    fn apply_writes_files_first_then_commands_in_the_steps() {
        let mut app = pending_app();
        keys(&mut app, &[KeyCode::Char('w'), KeyCode::Char('a')]);
        let Mode::Review { run: Some(r), .. } = &app.mode else { panic!("not applying") };
        assert_eq!(
            r.steps.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(),
            [
                "would write /c/a.yaml (2 keys)",
                "would write /c/b.yaml (1 key)",
                "would run ways agent key check --provider anthropic",
                "would run ways agent key remove --provider anthropic",
            ]
        );
        keys(&mut app, &[KeyCode::Char('x'), KeyCode::Char('X'), KeyCode::Esc]);
        assert!(matches!(app.mode, Mode::Review { run: Some(_), discard: false, .. }), "a run in flight takes no keys");
    }

    #[test]
    fn a_failing_write_step_keeps_it_and_everything_after_with_the_failed_row_marked() {
        let mut app = pending_app().fail_step(Some(2));
        keys(&mut app, &[KeyCode::Char('w'), KeyCode::Char('a')]);
        tick_to_end(&mut app);
        let (t, detail) = panes(&mut app);
        assert!(t.contains("✓   alpha") && t.contains("✗   gamma") && t.contains("·     1. $ ways agent key check"), "{t}");
        assert!(detail.contains("error") && detail.contains("SPIKE_FAIL_STEP=2") && detail.contains("would write /c/b.yaml (1 key)"), "{detail}");
        assert!(bottom(&mut app, 100, 24).contains("stopped"));
        app.tick();
        assert_eq!(review_tab(&app), Some(0), "review stays on the tab");
        let (t, detail) = panes(&mut app);
        assert!(t.contains("✗   gamma") && t.contains("·     1. $ ways agent key check") && t.contains("·     2. $ ways agent key remove"), "{t}");
        assert!(!t.contains("alpha") && !t.contains("beta"), "applied rows are cleared:\n{t}");
        assert!(detail.contains("SPIKE_FAIL_STEP=2"), "the cursor sits on the failed row:\n{detail}");
        assert!(bottom(&mut app, 100, 24).contains("stopped at step 2 of 4; 3 still pending in ways"));
        assert_eq!((app.pending_in(0), app.queue.len()), (3, 2));
        assert!(app.roots[0].children[2].setting.as_ref().unwrap().changed() && !app.roots[0].children[0].setting.as_ref().unwrap().changed());
        keys(&mut app, &[KeyCode::Esc]);
        assert!(matches!(app.mode, Mode::Browse) && app.failure.is_none());
    }

    #[test]
    fn a_failing_command_step_keeps_it_and_the_commands_after_it() {
        let mut app = pending_app().fail_step(Some(3));
        keys(&mut app, &[KeyCode::Char('w'), KeyCode::Char('a')]);
        tick_out(&mut app);
        assert_eq!(app.roots[0].changes(), 0, "both writes landed");
        assert_eq!(app.queue.len(), 2, "the failed command and the one after stay queued");
        let (t, _) = panes(&mut app);
        assert!(t.contains("✗     1. $ ways agent key check") && t.contains("·     2. $ ways agent key remove") && !t.contains("alpha"), "{t}");
    }

    #[test]
    fn quit_with_pending_in_any_tab_lists_each_and_discarding_needs_a_second_confirm() {
        let mut app = App::new("t", pending_tree());
        assert!(!app.key(press(KeyCode::Char('q'))), "nothing pending quits");
        let mut app = pending_app();
        keys(&mut app, &[KeyCode::Char('2')]);
        assert!(app.key(press(KeyCode::Char('q'))));
        let f = frame(&mut app, 100, 24).join("\n");
        for t in ["6 unsaved in 2 tabs", "ways", "●5", "other", "●1", "Back (Esc)", "Review ways (r)", "Quit and discard all (D)"] {
            assert!(f.contains(t), "{t} missing:\n{f}");
        }
        keys(&mut app, &[KeyCode::Esc]);
        assert!(matches!(app.mode, Mode::Browse) && app.pending() == 6);
        assert!(app.key(press(KeyCode::Esc)) && matches!(app.mode, Mode::Guard { .. }), "Esc asks too");
        let at = find(&mut app, "Back (Esc)");
        click(&mut app, at);
        assert!(matches!(app.mode, Mode::Browse));

        keys(&mut app, &[KeyCode::Char('q'), KeyCode::Char('r')]);
        assert!(review_tab(&app) == Some(0) && app.tab == 0, "Review enters review mode on the first tab with pending");
        assert!(bottom(&mut app, 100, 24).contains("review"));
        let at = find(&mut app, "Esc back");
        click(&mut app, at);
        keys(&mut app, &[KeyCode::Char('q')]);
        let at = find(&mut app, "Review ways (r)");
        click(&mut app, at);
        assert_eq!(review_tab(&app), Some(0), "a click on Review does the same");

        let mut app = pending_app();
        keys(&mut app, &[KeyCode::Char('q'), KeyCode::Char('D')]);
        assert!(matches!(app.mode, Mode::Guard { confirm: true }) && app.pending() == 6, "one D discards nothing");
        assert!(bottom(&mut app, 100, 24).contains("quit and discard all 6 pending?"));
        keys(&mut app, &[KeyCode::Esc]);
        assert!(matches!(app.mode, Mode::Guard { confirm: false }));
        keys(&mut app, &[KeyCode::Char('D')]);
        assert!(!app.key(press(KeyCode::Char('y'))), "the second confirm quits");
        assert_eq!(app.pending(), 0);
    }

    #[test]
    fn ctrl_c_meets_the_guard_when_anything_is_pending() {
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        let mut app = App::new("t", pending_tree());
        assert!(!app.key(ctrl_c), "nothing pending quits");
        let mut app = pending_app();
        keys(&mut app, &[KeyCode::Char('2')]);
        assert!(app.key(ctrl_c) && matches!(app.mode, Mode::Guard { confirm: false }));
        assert!(app.key(ctrl_c), "a second ^C does not quit past the prompt");
        let at = find(&mut app, "Quit and discard all (D)");
        click(&mut app, at);
        let at = find(&mut app, "y quit");
        assert_eq!(app.pending(), 6);
        click(&mut app, at);
        assert_eq!(app.pending(), 0);

        let mut app = pending_app();
        keys(&mut app, &[KeyCode::Char('w'), KeyCode::Char('a')]);
        assert!(app.key(ctrl_c) && matches!(app.mode, Mode::Review { run: Some(_), .. }), "a running apply is not interrupted");
    }

    #[test]
    fn a_typed_secret_never_reaches_review_or_its_progress() {
        const SECRET: &str = "ß#¤@%^";
        let mut app = App::new("t", action_tree());
        keys(&mut app, &[KeyCode::Enter]);
        type_str(&mut app, SECRET);
        keys(&mut app, &[KeyCode::Enter, KeyCode::Char('w')]);
        let review = frame(&mut app, 100, 30).join("\n");
        assert!(review.contains("< <stdin>") && !review.chars().any(|c| SECRET.contains(c)), "{review}");
        keys(&mut app, &[KeyCode::Char('a')]);
        for _ in 0..4 {
            let f = frame(&mut app, 100, 30).join("\n");
            assert!(f.contains("<stdin>") && !f.chars().any(|c| SECRET.contains(c)), "{f}");
            app.tick();
        }
        assert!(!format!("{:?}", app.queue).contains(SECRET));
    }

    #[test]
    fn a_modal_as_tall_as_the_pane_leaves_nothing_of_the_trees_title_beside_its_corner() {
        for (w, h) in [(80, 25), (80, 24), (100, 30), (110, 24)] {
            for open in [vec![KeyCode::Char('?')], vec![KeyCode::Char('q')], vec![KeyCode::Down, KeyCode::Char('a')]] {
                let mut app = pending_app();
                app.title = " ways settings — /home/aaron/Projects/ai/harness/agent-ways ".into();
                keys(&mut app, &open);
                let f = frame(&mut app, w, h);
                // The tree's title sits on the second row; a modal whose top edge is there starts at column 0.
                for tag in ["┌keys", "┌actions", "┌quit"] {
                    if let Some(at) = f[1].find(tag) {
                        assert_eq!(f[1][..at].chars().count(), 0, "{w}x{h}: {}", f[1]);
                    }
                }
            }
        }
    }

    #[test]
    fn m_toggles_mouse_capture_and_the_status_line_says_so() {
        let mut app = App::new("t", tabbed());
        assert!(app.mouse && screen(&mut app).contains("mouse on"));
        keys(&mut app, &[KeyCode::Char('m')]);
        assert!(!app.mouse && screen(&mut app).contains("mouse off"));
    }

    /// Writes frames of the real tree, cell by cell, for a PNG review:
    /// `SNAP_DIR=dir cargo test snap_frames -- --ignored`. Each line is the
    /// symbol, foreground, background and modifiers of one cell, row-major.
    #[test]
    #[ignore]
    fn snap_frames() {
        let out = std::path::PathBuf::from(std::env::var("SNAP_DIR").expect("SNAP_DIR"));
        let dir = std::env::current_dir().unwrap();
        let real = || App::new(" ways settings ", crate::ways::build(&crate::ways::Paths::resolve(&dir), &dir));
        let shot = |app: &mut App, name: &str, w: u16, h: u16| {
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            term.draw(|f| app.draw(f)).unwrap();
            let cells: Vec<String> = term
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|c| format!("{}\t{:?}\t{:?}\t{:?}", c.symbol(), c.fg, c.bg, c.modifier))
                .collect();
            std::fs::write(out.join(format!("{name}.cells")), format!("{w} {h}\n{}\n", cells.join("\n"))).unwrap();
        };
        // gate at 100x30: keys open, a check queued, the cursor on a provider.
        let mut app = real();
        keys(&mut app, &[KeyCode::Char('3'), KeyCode::Char('G'), KeyCode::Right, KeyCode::Down, KeyCode::Char('a'), KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
        keys(&mut app, &[KeyCode::Up, KeyCode::Up, KeyCode::Up, KeyCode::Enter, KeyCode::Down, KeyCode::Enter]);
        shot(&mut app, "gate-100x30", 100, 30);
        // The menu and the confirm of a key's remove action.
        keys(&mut app, &[KeyCode::Char('G'), KeyCode::Up, KeyCode::Char('a')]);
        shot(&mut app, "gate-menu-100x30", 100, 30);
        let Mode::Menu { path, .. } = &app.mode else { panic!("no menu") };
        let remove = tree::get(&app.roots, path).actions.iter().position(|a| a.confirm).expect("a confirmed action");
        keys(&mut app, &vec![KeyCode::Down; remove]);
        keys(&mut app, &[KeyCode::Enter]);
        shot(&mut app, "gate-confirm-100x30", 100, 30);
        // ways at 80x25: a domain switched off, so the tab and the row carry it.
        let mut app = real();
        keys(&mut app, &[KeyCode::Down, KeyCode::Down, KeyCode::Right, KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
        shot(&mut app, "ways-80x25", 80, 25);
        keys(&mut app, &[KeyCode::Char('c')]);
        shot(&mut app, "ways-pending-80x25", 80, 25);
    }

    /// Frames of the review flow on the real tree, as `snap_frames` writes them:
    /// `SNAP_DIR=dir cargo test snap_review -- --ignored`.
    #[test]
    #[ignore]
    fn snap_review() {
        let out = std::path::PathBuf::from(std::env::var("SNAP_DIR").expect("SNAP_DIR"));
        let dir = std::env::current_dir().unwrap();
        // Change up to two bools or numbers in each of three files, and queue three commands.
        let real_changes = || {
            let mut app = App::new(" ways settings ", crate::ways::build(&crate::ways::Paths::resolve(&dir), &dir));
            let mut per_file: std::collections::BTreeMap<std::path::PathBuf, usize> = Default::default();
            fn walk(n: &mut Node, per_file: &mut std::collections::BTreeMap<std::path::PathBuf, usize>) {
                if let Some(s) = n.setting.as_mut() {
                    if let Some(st) = &s.store {
                        let k = per_file.entry(st.file.clone()).or_default();
                        if *k < 2 && per_file.len() <= 3 {
                            match &s.kind {
                                Kind::Bool => s.value = if s.value == "true" { "false" } else { "true" }.into(),
                                Kind::Float { .. } => s.value = "0.6".into(),
                                _ => return,
                            }
                            *per_file.get_mut(&st.file).unwrap() += 1;
                        }
                    }
                }
                n.children.iter_mut().for_each(|c| walk(c, per_file));
            }
            app.roots.iter_mut().for_each(|r| walk(r, &mut per_file));
            let queued = [
                ("gate.keys.anthropic", "set", "ways agent key add --provider anthropic < <stdin>", false),
                ("install", "reconcile", "ways reconcile", true),
                ("install.targets", "add", "ways config target add ~/work/.claude", true),
            ];
            for (key, label, command, confirm) in queued {
                app.queue.push(Queued { key: key.into(), label: label.into(), command: command.into(), confirm });
            }
            app
        };
        let mut app = real_changes();
        let shot = |app: &mut App, name: &str, w: u16, h: u16| {
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            term.draw(|f| app.draw(f)).unwrap();
            let cells: Vec<String> =
                term.backend().buffer().content().iter().map(|c| format!("{}\t{:?}\t{:?}\t{:?}", c.symbol(), c.fg, c.bg, c.modifier)).collect();
            std::fs::write(out.join(format!("{name}.cells")), format!("{w} {h}\n{}\n", cells.join("\n"))).unwrap();
        };
        shot(&mut app, "cta-80x25", 80, 25);
        keys(&mut app, &[KeyCode::Char('q')]);
        shot(&mut app, "guard-100x30", 100, 30);
        keys(&mut app, &[KeyCode::Esc, KeyCode::Char('3'), KeyCode::Char('w')]);
        // The cursor on the first changed setting of the gate tab.
        let at = app.review_view(2).iter().position(|r| matches!(&r.kind, apply::RKind::Node { own: Some(_), .. })).expect("a changed gate setting");
        app.rcursor[2] = at;
        shot(&mut app, "reviewmode-100x30", 100, 30);
        keys(&mut app, &[KeyCode::Char('a')]);
        for _ in 0..3 {
            app.tick();
        }
        shot(&mut app, "reviewmode-apply-100x30", 100, 30);
        let mut app = real_changes();
        keys(&mut app, &[KeyCode::Char('3'), KeyCode::Char('w')]);
        let at = app.review_view(2).iter().position(|r| matches!(&r.kind, apply::RKind::Node { own: Some(_), .. })).unwrap();
        app.rcursor[2] = at;
        shot(&mut app, "reviewmode-80x25", 80, 25);
    }

    /// Prints one frame of the real tree for layout review: `cargo test frame_dump -- --nocapture --ignored`.
    #[test]
    #[ignore]
    fn frame_dump() {
        let dir = std::env::current_dir().unwrap();
        let mut app = App::new(" ways settings ", crate::ways::build(&crate::ways::Paths::resolve(&dir), &dir));
        keys(&mut app, &[KeyCode::Down, KeyCode::Down, KeyCode::Right, KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
        println!("{}", frame(&mut app, 100, 30).join("\n"));
    }
}
