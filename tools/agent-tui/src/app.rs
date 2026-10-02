//! The app shell over a tree of settings (ADR-504 §3): one tab per root and
//! a theme tab after them, then browse, filter, edit, run actions, and review
//! and apply as a read-only mode of the browser. Key handling and state live
//! here; drawing is in `render`, `review` and `themeview`. An [`Adapter`]
//! does the writes and commands an apply asks for.

mod apply;
pub mod flow;
mod render;
mod review;
pub mod theme;
pub mod themestate;
mod themetab;
mod themeview;

use std::collections::{BTreeSet, HashMap};
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

use crate::adapter::{Adapter, Unwired};
use crate::tree::{self, Arg, Kind, Node, Queue, Queued, Row, SecretBuf, Setting};
use apply::{Failure, Outcome, Run};
use flow::{Flow, FlowEvent};
pub use themestate::Themes;
use themestate::NameOp;

pub(crate) enum Mode {
    Browse,
    Edit(String),
    Filter,
    /// The keys and the tab's help text, scrolled this many lines.
    Help { scroll: u16 },
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
    /// The theme tab's action menu.
    ThemeMenu { sel: usize },
    /// A theme name typed for a new theme, a copy or a rename.
    ThemeName { op: NameOp, buf: String },
    /// y/n before a user theme's file is deleted.
    ThemeDelete { name: String },
    /// Esc in the editor with unsaved edits: save, discard, or back.
    ThemeUnsaved,
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
    /// The theme editor's save, and the unsaved prompt's save and discard.
    Save,
    Drop,
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
            Btn::Save => KeyCode::Char('s'),
            Btn::Drop => KeyCode::Char('d'),
        }
    }
}

/// What a reload keeps of each node, by dotted key: whether it was open,
/// and a pending value as (loaded, value).
type Kept = HashMap<String, (bool, Option<(String, String)>)>;

/// How long a step stays in each state while an apply runs.
const TICK: Duration = Duration::from_millis(150);

/// How often an idle screen looks for a settings file changed on disk.
const WATCH: Duration = Duration::from_millis(1000);

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
    /// The theme editor's sliders, each with its channel, and its hex field.
    sliders: Vec<(Rect, usize)>,
    hex: Rect,
}

/// What a session leaves behind: the edited tree and the queued actions.
pub struct Session {
    pub roots: Vec<Node>,
    pub queue: Queue,
}

impl Session {
    /// What is left pending, for the terminal after the screen closes.
    pub fn summary(&self) -> String {
        summary(&self.roots, &self.queue)
    }
}

/// What remains pending: value changes by file, then the queued commands in
/// order. A secret shows only as `<stdin>`.
pub fn summary(roots: &[Node], queue: &Queue) -> String {
    let changes = tree::changes(roots);
    if changes.is_empty() && queue.is_empty() {
        return "nothing pending\n".into();
    }
    let mut out = String::new();
    if !changes.is_empty() {
        let mut by_file: std::collections::BTreeMap<String, Vec<String>> = Default::default();
        for (key, store, from, to) in &changes {
            let (file, key) = match store {
                Some(s) => (s.file.display().to_string(), s.key.clone()),
                None => ("(no store)".into(), key.clone()),
            };
            by_file.entry(file).or_default().push(format!("    {key}: {from} → {to}"));
        }
        out += &format!("{} change(s) not applied:\n", changes.len());
        for (file, lines) in by_file {
            out += &format!("  {file}\n{}\n", lines.join("\n"));
        }
    }
    if !queue.is_empty() {
        out += &format!("{} command(s) not run:\n", queue.len());
        for (i, q) in queue.items().iter().enumerate() {
            out += &format!("  {}. {}\n", i + 1, q.command);
        }
    }
    out
}

pub struct App {
    pub roots: Vec<Node>,
    pub(crate) queue: Queue,
    title: String,
    /// The tab shown: an index into `roots`.
    tab: usize,
    /// The cursor in the rows on screen: the tab's, or the filter's.
    cursor: usize,
    /// Each tab's cursor while another tab, or a filter, is shown.
    saved: Vec<usize>,
    pub(crate) mode: Mode,
    filter: String,
    pub(crate) msg: String,
    show_changes: bool,
    list: ListState,
    /// Each tab's cursor in review, among the rows it shows.
    rcursor: Vec<usize>,
    /// The review groups the operator closed, by path; the `queued` group is the tab's root path.
    closed: BTreeSet<Vec<usize>>,
    /// The step the last apply stopped at, marked until review is left or applied again.
    failure: Option<Failure>,
    /// Whether the terminal should report the mouse; `m` turns it off so the
    /// terminal's own text selection works.
    mouse: bool,
    shape: theme::Shape,
    hits: Hits,
    /// Does the writes and commands an apply asks for, and supplies flows,
    /// help and the theme choice.
    adapter: Box<dyn Adapter>,
    /// The adapter's file stamp when the tree was last read.
    stamp: Option<u64>,
    /// The theme tab: themes on offer, the active one, the editor.
    pub themes: Themes,
    /// The slider channel a mouse drag holds.
    drag: Option<usize>,
}

impl App {
    pub fn new(title: impl Into<String>, roots: Vec<Node>) -> Self {
        let roots_len = roots.len();
        App {
            saved: vec![0; roots_len + 1],
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
            mouse: true,
            shape: theme::Shape::ROUND,
            hits: Hits::default(),
            adapter: Box::new(Unwired),
            stamp: None,
            themes: Themes::new(None, agent_theme::ColorDepth::TrueColor, None),
            drag: None,
        }
    }

    /// What applies the tree's changes and queued commands.
    pub fn adapter(mut self, a: impl Adapter + 'static) -> Self {
        self.stamp = a.stamp();
        self.adapter = Box::new(a);
        self
    }

    /// Open on tab `i`, the theme tab being the last.
    pub fn on_tab(mut self, i: usize) -> Self {
        if i < self.tabs() {
            self.tab = i;
        }
        self
    }

    /// The names of the tabs, the theme tab last.
    pub fn tab_names(&self) -> Vec<String> {
        self.roots.iter().map(|r| r.name.clone()).chain(["theme".to_string()]).collect()
    }

    /// The themes, their directory and the colour depth to draw at.
    pub fn themes(mut self, t: Themes) -> Self {
        self.themes = t;
        self
    }

    /// The settings tabs and the theme tab.
    fn tabs(&self) -> usize {
        self.roots.len() + 1
    }

    fn toggle_mouse(&mut self) {
        self.mouse = !self.mouse;
        self.msg = if self.mouse { "mouse on" } else { "mouse off: the terminal selects text" }.into();
    }

    pub fn shape(mut self, shape: theme::Shape) -> Self {
        self.shape = shape;
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
            let running = matches!(self.mode, Mode::Review { run: Some(_), .. });
            if !event::poll(if running { TICK } else { WATCH })? {
                if running {
                    self.tick();
                } else {
                    self.watch();
                }
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

    /// Reload the tree when a file it was read from changed on disk.
    pub fn watch(&mut self) {
        let now = self.adapter.stamp();
        if now.is_some() && now != self.stamp && matches!(self.mode, Mode::Browse | Mode::Review { run: None, .. }) {
            self.reload();
            if self.msg.is_empty() || !self.msg.contains("changed on disk") {
                self.msg = "reloaded: a settings file changed on disk".into();
            }
        }
    }

    /// Take the adapter's fresh tree, keeping what is open and every value
    /// still pending. A pending value whose file changed under it keeps the
    /// edit, and the message names it.
    pub fn reload(&mut self) {
        self.stamp = self.adapter.stamp();
        let Some(mut fresh) = self.adapter.reload() else { return };
        let mut kept: Kept = HashMap::new();
        fn collect(n: &Node, key: String, out: &mut Kept) {
            let pending = n.setting.as_ref().filter(|s| s.changed()).map(|s| (s.loaded.clone(), s.value.clone()));
            for c in &n.children {
                collect(c, format!("{key}.{}", c.name), out);
            }
            out.insert(key, (n.open, pending));
        }
        for r in &self.roots {
            collect(r, r.name.clone(), &mut kept);
        }
        let mut moved = Vec::new();
        fn restore(n: &mut Node, key: String, kept: &Kept, moved: &mut Vec<String>) {
            if let Some((open, pending)) = kept.get(&key) {
                n.open = *open;
                if let (Some((loaded, value)), Some(s)) = (pending, n.setting.as_mut()) {
                    if s.loaded != *loaded {
                        moved.push(key.clone());
                    }
                    if s.editable() {
                        s.value = value.clone();
                    }
                }
            }
            for c in &mut n.children {
                let k = format!("{key}.{}", c.name);
                restore(c, k, kept, moved);
            }
        }
        for r in &mut fresh {
            let k = r.name.clone();
            restore(r, k, &kept, &mut moved);
        }
        self.roots = fresh;
        let n = self.roots.len();
        self.saved.resize(n + 1, 0);
        self.rcursor.resize(n, 0);
        self.tab = self.tab.min(n);
        if let Some(k) = moved.first() {
            self.msg = format!("{k} changed on disk under a pending edit; review shows both");
        }
    }

    /// The guided flow that is open, if one is.
    pub fn flow(&self) -> Option<&Flow> {
        match &self.mode {
            Mode::Flow(f) => Some(f),
            _ => None,
        }
    }

    /// The queued commands, in run order.
    pub fn queued(&self) -> &[Queued] {
        self.queue.items()
    }

    /// The bottom bar's message.
    pub fn message(&self) -> &str {
        &self.msg
    }

    /// Whether the exit guard is asking.
    pub fn guarding(&self) -> bool {
        matches!(self.mode, Mode::Guard { .. })
    }

    /// The tab shown, the theme tab being the last.
    pub fn tab(&self) -> usize {
        self.tab
    }

    /// What remains pending, as the terminal shows it when the screen closes.
    pub fn summary(&self) -> String {
        summary(&self.roots, &self.queue)
    }

    /// Whether an apply is running.
    pub fn applying(&self) -> bool {
        matches!(self.mode, Mode::Review { run: Some(_), .. })
    }

    /// Value changes plus queued actions, in every tab.
    pub fn pending(&self) -> usize {
        (0..self.roots.len()).map(|t| self.pending_in(t)).sum()
    }

    pub fn pending_in(&self, tab: usize) -> usize {
        tree::pending(&self.roots[tab], &self.queue)
    }

    /// One tick of a running apply. A finished run is closed out on the tick
    /// after its last state, so the last glyph is seen.
    pub fn tick(&mut self) {
        let Mode::Review { run: Some(run), .. } = &mut self.mode else { return };
        if run.finished() {
            return self.finish_run();
        }
        run.tick(&mut self.roots, &mut self.queue, self.adapter.as_mut());
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
        // Values that were written resolve afresh: their layer and anything
        // above them that overrides them.
        if run.applied > 0 {
            self.reload();
        }
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

    /// Quit, or ask first when anything is pending in any tab or the theme
    /// editor holds unsaved edits.
    fn quit(&mut self) -> bool {
        if self.pending() == 0 && !self.theme_dirty() {
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
        let run = Run::plan(&self.roots, &self.queue, tab);
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
        self.themes.editor = None;
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
                    self.queue.push(Queued::new(key.clone(), o.label, o.command, o.confirm));
                }
            }
        }
        true
    }

    /// Open the flow an action names, launched from the node at `path`.
    fn start_flow(&mut self, path: &[usize], name: &str) {
        match self.adapter.flow(name) {
            Some(mut flow) => {
                flow.key = tree::key(&self.roots, path);
                self.mode = Mode::Flow(Box::new(flow));
            }
            None => self.msg = format!("no guided flow named {name}"),
        }
    }

    /// Handle one key. False ends the session.
    pub fn key(&mut self, k: KeyEvent) -> bool {
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
            Mode::Help { scroll } => match k.code {
                KeyCode::Up | KeyCode::Char('k') => self.mode = Mode::Help { scroll: scroll.saturating_sub(1) },
                KeyCode::Down | KeyCode::Char('j') => self.mode = Mode::Help { scroll: scroll.saturating_add(1) },
                KeyCode::PageUp => self.mode = Mode::Help { scroll: scroll.saturating_sub(10) },
                KeyCode::PageDown => self.mode = Mode::Help { scroll: scroll.saturating_add(10) },
                _ => {}
            },
            Mode::ThemeMenu { sel } => {
                let acts = self.theme_acts();
                match k.code {
                    KeyCode::Enter => self.theme_act(acts[sel.min(acts.len() - 1)]),
                    KeyCode::Up | KeyCode::Char('k') => self.mode = Mode::ThemeMenu { sel: sel.saturating_sub(1) },
                    KeyCode::Down | KeyCode::Char('j') => self.mode = Mode::ThemeMenu { sel: (sel + 1).min(acts.len() - 1) },
                    KeyCode::Esc | KeyCode::Char('q' | 'a') => {}
                    _ => self.mode = Mode::ThemeMenu { sel },
                }
            }
            Mode::ThemeName { op, mut buf } => match k.code {
                KeyCode::Esc => self.msg = "cancelled".into(),
                KeyCode::Enter => self.theme_named(op, buf),
                KeyCode::Backspace => {
                    buf.pop();
                    self.mode = Mode::ThemeName { op, buf };
                }
                KeyCode::Char(c) => {
                    buf.push(c);
                    self.mode = Mode::ThemeName { op, buf };
                }
                _ => self.mode = Mode::ThemeName { op, buf },
            },
            Mode::ThemeDelete { name } => match k.code {
                KeyCode::Char('y' | 'Y') => self.theme_delete(&name),
                KeyCode::Char('n' | 'N') | KeyCode::Esc => self.msg = "kept".into(),
                _ => self.mode = Mode::ThemeDelete { name },
            },
            Mode::ThemeUnsaved => self.unsaved_key(k),
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
                KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => self.mode = Mode::Edit(String::new()),
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
                KeyCode::Enter => self.stage(&path, action, buf.trim(), None),
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
                // The queued command shows `<stdin>`; the typed text rides
                // with it, redacted, to the command's stdin.
                KeyCode::Enter => self.stage(&path, action, "", Some(buf)),
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
                    self.clear_filter();
                    match (0..self.roots.len()).find(|t| self.pending_in(*t) > 0) {
                        Some(first) => {
                            self.switch_tab(first);
                            self.open_review(first);
                        }
                        // Only theme edits are unsaved: their review is the editor.
                        None => self.switch_tab(self.theme_tab()),
                    }
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
            Mode::Browse if self.on_theme_tab() => return self.theme_key(k),
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
            KeyCode::Char('m') => self.toggle_mouse(),
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
            KeyCode::Tab => self.switch_tab((self.tab + 1) % self.tabs()),
            KeyCode::BackTab => self.switch_tab((self.tab + self.tabs() - 1) % self.tabs()),
            KeyCode::Char(c @ '1'..='9') => {
                let i = c as usize - '1' as usize;
                if i < self.tabs() {
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
            KeyCode::Char('m') => self.toggle_mouse(),
            KeyCode::Char('/') => self.mode = Mode::Filter,
            KeyCode::Char('?') => self.mode = Mode::Help { scroll: 0 },
            _ => {}
        }
        true
    }

    /// Handle one mouse event against the last frame's hits. A click reaches
    /// the same paths a key does, so the confirm and secret rules hold.
    /// During text, secret or filter entry and a confirm, only the confirm's
    /// answers respond.
    pub fn mouse(&mut self, m: MouseEvent) {
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
            Mode::Browse if self.tab == self.roots.len() => self.theme_mouse(m),
            Mode::Browse => {
                if let Some(k) = wheel {
                    self.browse(press(k));
                } else if click {
                    self.click_browse(at);
                }
            }
            Mode::Menu { sel, .. } | Mode::ThemeMenu { sel } => {
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
            Mode::Confirm { .. } | Mode::DiscardTab { .. } | Mode::ThemeDelete { .. } | Mode::Guard { confirm: true } | Mode::Review { discard: true, .. } if click => {
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
            Mode::Guard { confirm: false } | Mode::ThemeUnsaved if click => self.click_button(at),
            Mode::Help { .. } if click => self.mode = Mode::Browse,
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
            if t == self.theme_tab() {
                self.msg = "a theme saves on its own; nothing of it is reviewed".into();
                return;
            }
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
        if let Some(why) = n.setting.as_ref().and_then(|s| s.locked.clone()) {
            self.msg = format!("locked: {why}");
            return;
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
            Some(s) if s.editable() => self.mode = Mode::Edit(s.value.clone()),
            Some(Setting { locked: Some(why), .. }) => self.msg = format!("locked: {why}"),
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
            Arg::None => self.stage(&path, action, "", None),
            Arg::Text(_) => self.mode = Mode::Arg { path, action, buf: String::new() },
            Arg::Secret => self.mode = Mode::Secret { path, action, buf: SecretBuf::default() },
            Arg::Flow(name) => self.start_flow(&path, &name),
        }
    }

    /// Build the queued command; ask first when the action needs confirming.
    fn stage(&mut self, path: &[usize], action: usize, text: &str, stdin: Option<SecretBuf>) {
        let a = &tree::get(&self.roots, path).actions[action];
        let mut queued = Queued::new(tree::key(&self.roots, path), a.label.clone(), a.render(text), a.confirm);
        queued.stdin = stdin;
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
        let checked = {
            let s = tree::get(&self.roots, &path).setting.as_ref().expect("edit mode only on settings");
            match s.store.as_ref().and_then(|st| self.adapter.validate(st, buf)) {
                Some(r) => r,
                None => s.validate(buf),
            }
        };
        let n = tree::get_mut(&mut self.roots, &path);
        let s = n.setting.as_mut().expect("edit mode only on settings");
        match checked {
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
        if let Some(why) = n.setting.as_ref().and_then(|s| s.locked.clone()) {
            self.msg = format!("locked: {why}");
            return;
        }
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
mod theme_tests;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod shell_tests;
