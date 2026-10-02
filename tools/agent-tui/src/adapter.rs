//! What an application supplies to the shell (ADR-504 §3): the writes and
//! commands an apply performs, a fresh tree after them, guided flows, help
//! text, and where the theme choice is kept. The shell holds the tree and
//! the queue and decides the order; the adapter does the work.

use std::path::Path;

use crate::app::flow::Flow;
use crate::tree::{Node, Queued, Store};

/// One value an apply writes: the setting's store and the text the tree
/// holds for it.
#[derive(Debug, Clone, Copy)]
pub struct Write<'a> {
    pub store: &'a Store,
    pub value: &'a str,
}

pub trait Adapter {
    /// Check text typed for the setting stored at `store`, returning the
    /// value as the tree shows it. `None` leaves the check to the setting's
    /// kind.
    fn validate(&self, _store: &Store, _text: &str) -> Option<Result<String, String>> {
        None
    }

    /// Write `values` into `file`, all or nothing. An error names what went
    /// wrong; the file is then as it was.
    fn write(&mut self, file: &Path, values: &[Write]) -> Result<(), String>;

    /// Run one queued command. Its masked stdin, if any, is in `q.stdin`.
    fn run(&mut self, q: &Queued) -> Result<(), String>;

    /// The tree as the files now hold it, after an apply or a change on
    /// disk. `None` keeps the tree the shell has.
    fn reload(&mut self) -> Option<Vec<Node>> {
        None
    }

    /// A value that changes when a file the tree was read from changes, so
    /// the shell knows to reload. `None` turns the watch off.
    fn stamp(&self) -> Option<u64> {
        None
    }

    /// The guided flow an action's `Arg::Flow` names.
    fn flow(&self, _name: &str) -> Option<Flow> {
        None
    }

    /// The help text for a tab, as the application's own `--help` prints it
    /// (ADR-503 §10). The help overlay shows it below the keys.
    fn help(&self, _tab: &str) -> Option<String> {
        None
    }

    /// Keep `name` as the active theme.
    fn choose_theme(&mut self, _name: &str) -> Result<(), String> {
        Err("this application keeps no theme choice".into())
    }
}

/// The adapter of an application that wired none: nothing is written or
/// run, and every apply step says so.
pub struct Unwired;

impl Adapter for Unwired {
    fn write(&mut self, file: &Path, _: &[Write]) -> Result<(), String> {
        Err(format!("no adapter: {} was not written", file.display()))
    }

    fn run(&mut self, q: &Queued) -> Result<(), String> {
        Err(format!("no adapter: `{}` was not run", q.command))
    }
}
