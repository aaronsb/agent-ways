//! What review and apply work on: the pending items as one list, and the
//! simulated run over them. Nothing here writes or runs anything; a step only
//! moves the tree's loaded values and the queue as the real step would.

use std::collections::BTreeMap;

use crate::tree::{self, Node, Queue};

/// One pending item. A value change carries its file; a queued command its
/// place in the queue and whether it asked first.
pub enum Entry {
    Value { path: Vec<usize>, file: String, key: String, from: String, to: String },
    Action { index: usize, command: String, confirm: bool },
}

/// One tab's pending items: value changes grouped by file (files sorted,
/// keys in tree order), then its queued commands in run order.
pub fn entries(roots: &[Node], queue: &Queue, tab: usize) -> Vec<Entry> {
    let mut by_file: BTreeMap<String, Vec<Entry>> = BTreeMap::new();
    let mut walk = Vec::new();
    collect(&roots[tab], &mut vec![tab], &mut walk);
    for path in walk {
        let n = tree::get(roots, &path);
        let s = n.setting.as_ref().expect("collected nodes have settings");
        let (file, key) = match &s.store {
            Some(st) => (st.file.display().to_string(), st.key.clone()),
            None => ("(no store)".into(), tree::key(roots, &path)),
        };
        by_file.entry(file.clone()).or_default().push(Entry::Value { path, file, key, from: s.loaded.clone(), to: s.value.clone() });
    }
    let root = roots[tab].name.as_str();
    let actions = queue
        .items()
        .iter()
        .enumerate()
        .filter(|(_, q)| tree::is_under(&q.key, root))
        .map(|(index, q)| Entry::Action { index, command: q.command.clone(), confirm: q.confirm });
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
    /// Value writes to one file: the settings it takes.
    Write(Vec<Vec<usize>>),
    /// The tab's first queued command.
    Run,
}

pub struct Step {
    pub text: String,
    pub state: St,
    work: Work,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Outcome {
    Running,
    Done,
    /// Stopped at this step.
    Stopped(usize),
}

/// A simulated apply of one tab: value writes first, one step per file, then the queued
/// commands in order. A step advances one state per tick.
pub struct Run {
    /// The tab applied: an index into the roots.
    pub tab: usize,
    pub steps: Vec<Step>,
    pub outcome: Outcome,
    /// Items applied so far: keys written and commands run.
    pub applied: usize,
    /// The 1-based step that fails, for exercising the failure path.
    fail_step: Option<usize>,
}

impl Run {
    pub fn plan(roots: &[Node], queue: &Queue, tab: usize, fail_step: Option<usize>) -> Run {
        let mut files: Vec<(String, Vec<Vec<usize>>)> = Vec::new();
        let mut commands = Vec::new();
        for e in entries(roots, queue, tab) {
            match e {
                Entry::Value { path, file, .. } => match files.last_mut() {
                    Some((f, paths)) if *f == file => paths.push(path),
                    _ => files.push((file, vec![path])),
                },
                Entry::Action { command, .. } => commands.push(command),
            }
        }
        let writes = files.into_iter().map(|(file, paths)| Step {
            text: format!("would write {file} ({} key{})", paths.len(), if paths.len() == 1 { "" } else { "s" }),
            state: St::Pending,
            work: Work::Write(paths),
        });
        let runs = commands.into_iter().map(|c| Step { text: format!("would run {c}"), state: St::Pending, work: Work::Run });
        Run { tab, steps: writes.chain(runs).collect(), outcome: Outcome::Running, applied: 0, fail_step }
    }

    pub fn finished(&self) -> bool {
        self.outcome != Outcome::Running
    }

    /// Advance one state: start the next step, or finish the running one.
    /// A finished write moves its settings' loaded values to their values; a
    /// finished command leaves the queue. A step that fails changes nothing
    /// and stops the run.
    pub fn tick(&mut self, roots: &mut [Node], queue: &mut Queue) {
        if self.finished() {
            return;
        }
        if let Some(i) = self.steps.iter().position(|s| s.state == St::Running) {
            if self.fail_step == Some(i + 1) {
                self.steps[i].state = St::Failed;
                self.outcome = Outcome::Stopped(i);
                return;
            }
            match &self.steps[i].work {
                Work::Write(paths) => {
                    for p in paths {
                        let s = tree::get_mut(roots, p).setting.as_mut().expect("planned nodes have settings");
                        s.loaded = s.value.clone();
                    }
                    self.applied += paths.len();
                }
                Work::Run => {
                    if let Some(q) = queue.first_under(&roots[self.tab].name) {
                        queue.remove(q);
                    }
                    self.applied += 1;
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
