//! What an application supplies to the shell (ADR-504 §3): the writes and
//! commands an apply performs, a fresh tree after them, guided flows, help
//! text, and where the theme choice is kept. The shell holds the tree and
//! the queue and decides the order; the adapter does the work.

use std::path::Path;

use ratatui::text::Line;

use crate::app::flow::Flow;
use crate::tree::{Node, Queued, Store};

/// One value an apply writes: the setting's store, the text the tree holds
/// for it, and the value the tree read before the edit. An adapter compares
/// `loaded` with what the file holds now, under its lock, and refuses the
/// write when they differ, so an outside change is never overwritten
/// unseen.
#[derive(Debug, Clone, Copy)]
pub struct Write<'a> {
    pub store: &'a Store,
    pub value: &'a str,
    pub loaded: &'a str,
}

/// A queued command in flight. The shell polls it between frames, so the
/// screen keeps drawing and Ctrl-C keeps working while it runs.
pub trait Job {
    /// The outcome once the command has ended; `None` while it runs.
    fn poll(&mut self) -> Option<Result<(), String>>;
    /// End the command now. A later `poll` reports how it ended.
    fn stop(&mut self) {}
    /// What the command printed, once `poll` has reported its end: what a
    /// response modal shows. `None` from a job that keeps no output; the
    /// modal then shows the outcome `poll` gave.
    fn printed(&mut self) -> Option<Printed> {
        None
    }
}

/// What an ended command printed and how it exited.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Printed {
    /// The exit code; `None` when a signal ended it.
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// A job that ended before it was polled: what [`Adapter::start`] gives for
/// an adapter that only runs commands to the end.
pub struct Ended(pub Option<Result<(), String>>);

impl Job for Ended {
    fn poll(&mut self) -> Option<Result<(), String>> {
        Some(self.0.take().unwrap_or(Ok(())))
    }
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

    /// Run one queued command to its end. Its masked stdin, if any, is in
    /// `q.stdin`.
    fn run(&mut self, q: &Queued) -> Result<(), String>;

    /// Start one queued command and hand back the job to poll. The default
    /// runs it to the end at once; an adapter whose commands can take a
    /// while starts them in the background so the screen stays live.
    fn start(&mut self, q: &Queued) -> Box<dyn Job> {
        Box::new(Ended(Some(self.run(q))))
    }

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

    /// Why the stamp moved, when the adapter knows it was not a file
    /// changing on disk, such as a background read landing: the bottom bar
    /// says this after the reload instead of naming a changed file. `None`
    /// keeps the shell's message. Asked before the reload.
    fn stamp_moved(&mut self) -> Option<String> {
        None
    }

    /// The guided flow an action's `Arg::Flow` names.
    fn flow(&self, _name: &str) -> Option<Flow> {
        None
    }

    /// Switch to the view an action's `Arg::View` names. `pending` holds
    /// the store of each pending value edit: an adapter refuses, saying
    /// why, a view in which one of them would no longer show, since the
    /// reload would drop it. After a switch the shell reloads the tree; the
    /// message, if any, goes to the bottom bar.
    fn view(&mut self, _name: &str, _pending: &[&Store]) -> Result<Option<String>, String> {
        Ok(None)
    }

    /// The tree pane's title on `tab`, such as one naming the view the tab
    /// shows. Asked as the pane is drawn; `None` keeps the shell's.
    fn title(&self, _tab: &str) -> Option<String> {
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

    /// Keep `name` as the lozenge shape, one of [`crate::theme::Shape::NAMES`].
    fn choose_shape(&mut self, _name: &str) -> Result<(), String> {
        Err("this application keeps no shape choice".into())
    }

    /// Keep `name` as whose waypoints the screens draw: `theme` or
    /// `default` (`agent_theme::settings::WAYPOINT_SETS`).
    fn choose_waypoints(&mut self, _name: &str) -> Result<(), String> {
        Err("this application keeps no waypoints choice".into())
    }

    /// The waypoints `theme` draws under `setting`, each in its colour, for
    /// the theme tab to show beside the choice. `None` shows none.
    fn waypoints_preview(&self, _theme: &agent_theme::Theme, _setting: &str) -> Option<Line<'static>> {
        None
    }

    /// What the theme tab's Claude Code section shows: the mod that draws
    /// the theme in Claude Code, its render settings and its words. `None`
    /// leaves the section out.
    fn claude_code(&self) -> Option<ClaudeCode> {
        None
    }

    /// Keep `value` for the Claude Code setting `key`, one of
    /// [`ClaudeCode::render`]'s.
    fn set_claude_code(&mut self, key: &str, _value: &str) -> Result<(), String> {
        Err(format!("this application keeps no {key}"))
    }
}

/// The theme tab's Claude Code section, as the application reads it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClaudeCode {
    /// The mod in each target: the target as shown, whether the mod is
    /// installed and active there, and its state in a few words.
    pub targets: Vec<(String, bool, String)>,
    /// The render settings, in the order the section lists them.
    pub render: Vec<Render>,
    /// The spinner's and the done line's words the settings set; an empty
    /// list leaves the theme's.
    pub spinner: Vec<String>,
    pub done: Vec<String>,
    /// The file a change is written to, as shown.
    pub file: String,
}

/// One render setting: changed in place by stepping through `options`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Render {
    /// The settings key a change is kept under.
    pub key: String,
    /// The row's name.
    pub label: String,
    pub value: String,
    pub default: String,
    pub options: Vec<String>,
    pub doc: String,
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
