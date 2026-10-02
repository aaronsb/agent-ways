//! The app shell over a tree of settings (ADR-504 §3): one tab per root and
//! a theme tab after them, then browse, filter, edit, run actions, and review
//! and apply as a read-only mode of the browser. Key handling and state live
//! here; drawing is in `render`, `review` and `themeview`. An [`Adapter`]
//! does the writes and commands an apply asks for.

mod apply;
pub mod flow;
mod keys;
mod render;
mod review;
pub mod theme;
pub mod themestate;
mod sync;
pub use sync::Reloaded;
pub mod term;
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
    /// The signal that ended the session, if one did. The terminal is
    /// restored by then; the caller exits with 128 plus it.
    pub signal: Option<i32>,
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
        let failed_command = matches!(run.outcome, Outcome::Stopped(i) if run.steps[i].is_command());
        self.failure = run.failure();
        // Values that were written resolve afresh, with their layer and
        // anything above them; a command that failed may have done part of
        // its work, so the tree is read again after it too.
        let reloaded = if run.applied > 0 || matches!(run.outcome, Outcome::Stopped(_)) { self.reload() } else { Reloaded::default() };
        match run.outcome {
            Outcome::Stopped(i) => {
                let wrote = run.written();
                let mut msg = String::new();
                if !wrote.is_empty() {
                    msg += &format!("wrote {}; ", wrote.join(", "));
                }
                msg += &format!("stopped at step {} of {}: {}; {} still pending in {name}", i + 1, run.steps.len(), run.error(i), self.pending_in(tab));
                if failed_command {
                    msg += ". The command may have done part of its work and the tree is read again: check the row, then apply again or x to unqueue it";
                }
                if !reloaded.is_clean() {
                    msg += &format!(" · {}", reloaded.message());
                }
                self.msg = msg;
                self.mode = Mode::Review { tab, run: None, discard: false };
                self.focus_failure(tab);
            }
            _ => {
                self.failure = None;
                self.msg = format!("applied {} in {name}", run.applied);
                if !reloaded.is_clean() {
                    self.msg += &format!(" · {}", reloaded.message());
                }
                self.advance(tab);
            }
        }
    }

    /// Stop a running apply where it is: the command in flight ends, its
    /// step is marked failed with `why`, and the run is closed out.
    pub fn stop_run(&mut self, why: &str) {
        if let Mode::Review { run: Some(run), .. } = &mut self.mode {
            run.stop(why);
            self.finish_run();
        }
    }

    /// Whether a masked entry is open: nothing typed now may be shown or
    /// come from anywhere but a keyboard.
    pub fn masked(&self) -> bool {
        matches!(self.mode, Mode::Secret { .. })
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
        // A file changed since it was read: read it again first, and when a
        // pending edit moved or went, stay in review so the change is seen
        // before anything is written. The adapter also refuses a write whose
        // read value no longer matches, under its lock.
        if self.adapter.stamp() != self.stamp {
            let r = self.reload();
            if !r.is_clean() {
                self.msg = format!("{}; a to apply what is pending now", r.message());
                self.mode = Mode::Review { tab, run: None, discard: false };
                return;
            }
        }
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

}

#[cfg(test)]
mod theme_tests;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod shell_tests;
