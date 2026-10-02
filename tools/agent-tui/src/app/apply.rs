//! What review mode shows and applies: a tab's pending items as tree rows,
//! and the run over them. The run decides the order, value writes first and
//! one step per file, then the queued commands in order, and stops at the
//! first failure; the adapter does each step's work.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::adapter::{Adapter, Job, Write};
use crate::tree::{self, Node, Queue};

/// One pending value change: where it would be written, and what changes.
#[derive(Clone)]
pub struct Change {
    pub file: String,
    pub key: String,
    /// The layer the file belongs to.
    pub layer: String,
    pub from: String,
    pub to: String,
}

#[derive(Clone)]
pub enum RKind {
    /// A node that has changes: its own, below it, or both.
    Node {
        own: Option<Change>,
        /// Changes in the nodes below.
        below: usize,
        /// Every file the changes at and below the node write, sorted.
        files: Vec<String>,
    },
    /// The `queued` group that holds the tab's commands.
    Queued { count: usize },
    /// A queued command; `n` is its place in run order, from 1.
    Action { n: usize, command: String, confirm: bool, key: String, label: String },
}

/// One row of review mode's tree. Rows come in tree order with the `queued`
/// group last, so hiding what follows a closed row needs only the depth.
#[derive(Clone)]
pub struct RRow {
    pub depth: usize,
    /// The node a `Node` row stands for; the tab's root path for the `queued` group.
    pub path: Vec<usize>,
    pub name: String,
    pub kind: RKind,
}

impl RRow {
    /// Whether Enter and a click on its marker open and close it.
    pub fn toggles(&self) -> bool {
        match &self.kind {
            RKind::Node { below, .. } => *below > 0,
            RKind::Queued { .. } => true,
            RKind::Action { .. } => false,
        }
    }

    /// Whether it is an item an apply would write or run, not a group.
    pub fn is_item(&self) -> bool {
        matches!(&self.kind, RKind::Node { own: Some(_), .. } | RKind::Action { .. })
    }
}

/// One tab's pending items as rows: changed settings under the groups that
/// hold them, then the queued commands, which run after the writes, in the
/// order they were queued.
pub fn review_rows(roots: &[Node], queue: &Queue, tab: usize) -> Vec<RRow> {
    let mut out = Vec::new();
    for (i, c) in roots[tab].children.iter().enumerate() {
        node_rows(roots, c, vec![tab, i], &mut out);
    }
    let queued: Vec<_> = queue.items().iter().filter(|q| tree::is_under(&q.key, &roots[tab].name)).collect();
    if !queued.is_empty() {
        out.push(RRow { depth: 0, path: vec![tab], name: "queued".into(), kind: RKind::Queued { count: queued.len() } });
        for (i, q) in queued.iter().enumerate() {
            let kind = RKind::Action { n: i + 1, command: q.command.clone(), confirm: q.confirm, key: q.key.clone(), label: q.label.clone() };
            out.push(RRow { depth: 1, path: vec![tab], name: q.label.clone(), kind });
        }
    }
    out
}

fn node_rows(roots: &[Node], n: &Node, path: Vec<usize>, out: &mut Vec<RRow>) {
    if n.changes() == 0 {
        return;
    }
    let own = n.setting.as_ref().filter(|s| s.changed()).map(|s| {
        let (file, key, layer) = match &s.store {
            Some(st) => (st.shown.clone(), st.key.clone(), st.layer.clone()),
            None => ("(no store)".into(), tree::key(roots, &path), String::new()),
        };
        Change { file, key, layer, from: s.loaded.clone(), to: s.value.clone() }
    });
    let mut files = BTreeSet::new();
    files_under(n, &mut files);
    let below = n.children.iter().map(Node::changes).sum();
    out.push(RRow { depth: path.len() - 2, path: path.clone(), name: n.name.clone(), kind: RKind::Node { own, below, files: files.into_iter().collect() } });
    for (i, c) in n.children.iter().enumerate() {
        let mut p = path.clone();
        p.push(i);
        node_rows(roots, c, p, out);
    }
}

fn files_under(n: &Node, out: &mut BTreeSet<String>) {
    if let Some(s) = n.setting.as_ref().filter(|s| s.changed()) {
        out.insert(s.store.as_ref().map_or("(no store)".into(), |st| st.shown.clone()));
    }
    n.children.iter().for_each(|c| files_under(c, out));
}

/// The rows a cursor can reach: those not under a row in `closed`.
pub fn visible(rows: &[RRow], closed: &BTreeSet<Vec<usize>>) -> Vec<RRow> {
    let mut out = Vec::new();
    let mut hidden: Option<usize> = None;
    for r in rows {
        if hidden.is_some_and(|d| r.depth > d) {
            continue;
        }
        hidden = None;
        if r.toggles() && closed.contains(&r.path) {
            hidden = Some(r.depth);
        }
        out.push(r.clone());
    }
    out
}

/// One pending item of a tab, for planning a run.
enum Entry {
    Value { path: Vec<usize>, file: PathBuf, shown: String },
    Action { command: String },
}

/// One tab's pending items: value changes grouped by file (files sorted,
/// keys in tree order), then its queued commands in run order.
fn entries(roots: &[Node], queue: &Queue, tab: usize) -> Vec<Entry> {
    let mut by_file: BTreeMap<PathBuf, Vec<Entry>> = BTreeMap::new();
    let mut walk = Vec::new();
    collect(&roots[tab], &mut vec![tab], &mut walk);
    for path in walk {
        let s = tree::get(roots, &path).setting.as_ref().expect("collected nodes have settings");
        let (file, shown) = s.store.as_ref().map_or_else(|| (PathBuf::from("(no store)"), "(no store)".to_string()), |st| (st.file.clone(), st.shown.clone()));
        by_file.entry(file.clone()).or_default().push(Entry::Value { path, file, shown });
    }
    let root = roots[tab].name.as_str();
    let actions = queue.items().iter().filter(|q| tree::is_under(&q.key, root)).map(|q| Entry::Action { command: q.command.clone() });
    by_file.into_values().flatten().chain(actions).collect()
}

fn collect(n: &Node, path: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
    if n.setting.as_ref().is_some_and(|s| s.changed()) {
        out.push(path.clone());
    }
    for (i, c) in n.children.iter().enumerate() {
        path.push(i);
        collect(c, path, out);
        path.pop();
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum St {
    Pending,
    Running,
    Done,
    Failed,
}

enum Work {
    /// Value writes to one file: the file and the settings it takes.
    Write(PathBuf, Vec<Vec<usize>>),
    /// The tab's first queued command.
    Run,
}

pub struct Step {
    pub text: String,
    pub state: St,
    /// Why the step failed, once it has.
    pub error: Option<String>,
    /// The file a write step writes, as the screens show it.
    pub shown: Option<String>,
    work: Work,
}

impl Step {
    /// Whether the step runs a command rather than writing a file.
    pub fn is_command(&self) -> bool {
        matches!(self.work, Work::Run)
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Outcome {
    Running,
    Done,
    /// Stopped at this step.
    Stopped(usize),
}

/// An apply of one tab: value writes first, one step per file, then the
/// queued commands in order. A step advances one state per tick: it shows as
/// running for one tick, then does its work on the next.
pub struct Run {
    /// The tab applied: an index into the roots.
    pub tab: usize,
    pub steps: Vec<Step>,
    pub outcome: Outcome,
    /// Items applied so far: keys written and commands run.
    pub applied: usize,
    /// The review's rows as the run began, so a finished write keeps its row
    /// on screen with its ✓ until the run ends.
    pub rows: Vec<RRow>,
    /// The command in flight, while a run step waits on it.
    job: Option<Box<dyn Job>>,
}

impl Run {
    pub fn plan(roots: &[Node], queue: &Queue, tab: usize) -> Run {
        let mut files: Vec<(PathBuf, String, Vec<Vec<usize>>)> = Vec::new();
        let mut commands = Vec::new();
        for e in entries(roots, queue, tab) {
            match e {
                Entry::Value { path, file, shown } => match files.last_mut() {
                    Some((f, _, paths)) if *f == file => paths.push(path),
                    _ => files.push((file, shown, vec![path])),
                },
                Entry::Action { command } => commands.push(command),
            }
        }
        let writes = files.into_iter().map(|(file, shown, paths)| Step {
            text: format!("write {shown} ({} key{})", paths.len(), if paths.len() == 1 { "" } else { "s" }),
            state: St::Pending,
            error: None,
            shown: Some(shown),
            work: Work::Write(file, paths),
        });
        let runs = commands.into_iter().map(|c| Step { text: format!("run {c}"), state: St::Pending, error: None, shown: None, work: Work::Run });
        Run { tab, steps: writes.chain(runs).collect(), outcome: Outcome::Running, applied: 0, rows: review_rows(roots, queue, tab), job: None }
    }

    pub fn finished(&self) -> bool {
        self.outcome != Outcome::Running
    }

    /// The step that writes or runs what `row` shows. A group has none.
    pub fn step_of(&self, row: &RRow) -> Option<usize> {
        match &row.kind {
            RKind::Node { own: Some(_), .. } => self.steps.iter().position(|s| matches!(&s.work, Work::Write(_, p) if p.contains(&row.path))),
            RKind::Action { n, .. } => self.steps.iter().enumerate().filter(|(_, s)| matches!(s.work, Work::Run)).nth(n - 1).map(|(i, _)| i),
            _ => None,
        }
    }

    /// Why step `i` failed, or nothing when it has not.
    pub fn error(&self, i: usize) -> String {
        self.steps.get(i).and_then(|s| s.error.clone()).unwrap_or_default()
    }

    /// What a stopped run leaves for the review to mark: the failed step.
    pub fn failure(&self) -> Option<Failure> {
        let Outcome::Stopped(i) = self.outcome else { return None };
        let paths = match &self.steps[i].work {
            Work::Write(_, p) => p.clone(),
            Work::Run => Vec::new(),
        };
        Some(Failure { tab: self.tab, paths, text: self.steps[i].text.clone(), error: self.error(i) })
    }

    /// Whether a command is in flight.
    pub fn waiting(&self) -> bool {
        self.job.is_some()
    }

    /// Stop the run where it is: end the command in flight and mark its
    /// step failed with `why`. Steps before it stay done.
    pub fn stop(&mut self, why: &str) {
        if self.finished() {
            return;
        }
        if let Some(mut job) = self.job.take() {
            job.stop();
            let _ = job.poll();
        }
        let i = self.steps.iter().position(|s| s.state == St::Running).or_else(|| self.steps.iter().position(|s| s.state == St::Pending));
        if let Some(i) = i {
            self.steps[i].state = St::Failed;
            self.steps[i].error = Some(why.to_string());
            self.outcome = Outcome::Stopped(i);
        } else {
            self.outcome = Outcome::Done;
        }
    }

    /// The files the run has written, as the screens show them.
    pub fn written(&self) -> Vec<String> {
        self.steps.iter().filter(|s| s.state == St::Done).filter_map(|s| s.shown.clone()).collect()
    }

    /// Advance one state: start the next step, or do the running one's work.
    /// A write the adapter makes moves its settings' loaded values to their
    /// values; a command it runs leaves the queue. A step that fails changes
    /// nothing and stops the run.
    pub fn tick(&mut self, roots: &mut [Node], queue: &mut Queue, adapter: &mut dyn Adapter) {
        if self.finished() {
            return;
        }
        if let Some(i) = self.steps.iter().position(|s| s.state == St::Running) {
            let done = match &self.steps[i].work {
                Work::Write(file, paths) => {
                    let values: Vec<Write> = paths
                        .iter()
                        .filter_map(|p| {
                            let s = tree::get(roots, p).setting.as_ref()?;
                            Some(Write { store: s.store.as_ref()?, value: &s.value, loaded: &s.loaded })
                        })
                        .collect();
                    let r = if values.len() == paths.len() { adapter.write(file, &values) } else { Err(format!("{}: a setting here has no store", file.display())) };
                    r.map(|()| {
                        for p in paths {
                            let s = tree::get_mut(roots, p).setting.as_mut().expect("planned nodes have settings");
                            s.loaded = s.value.clone();
                        }
                        paths.len()
                    })
                }
                Work::Run => match queue.first_under(&roots[self.tab].name) {
                    Some(q) => {
                        let job = self.job.get_or_insert_with(|| adapter.start(&queue.items()[q]));
                        match job.poll() {
                            // Still running: the step stays as it is.
                            None => return,
                            Some(r) => {
                                self.job = None;
                                r.map(|()| {
                                    queue.remove(q);
                                    1
                                })
                            }
                        }
                    }
                    None => Err("the queued command is gone".into()),
                },
            };
            match done {
                Ok(n) => self.applied += n,
                Err(e) => {
                    self.steps[i].state = St::Failed;
                    self.steps[i].error = Some(e);
                    self.outcome = Outcome::Stopped(i);
                    return;
                }
            }
            self.steps[i].state = St::Done;
            if i + 1 == self.steps.len() {
                self.outcome = Outcome::Done;
            }
        } else if let Some(s) = self.steps.iter_mut().find(|s| s.state == St::Pending) {
            s.state = St::Running;
        } else {
            self.outcome = Outcome::Done;
        }
    }
}

/// The step a stopped run failed at, kept so the review marks it while its
/// rows are live again. No paths means the failed step ran a command, which
/// is then the tab's first queued one.
pub struct Failure {
    pub tab: usize,
    pub paths: Vec<Vec<usize>>,
    pub text: String,
    pub error: String,
}

impl Failure {
    pub fn marks(&self, row: &RRow) -> bool {
        match &row.kind {
            RKind::Node { own: Some(_), .. } => self.paths.contains(&row.path),
            RKind::Action { n, .. } => self.paths.is_empty() && *n == 1,
            _ => false,
        }
    }
}
