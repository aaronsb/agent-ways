//! The cleanup items (ADR-404 D3e, ADR-405 amendment of 2026-10-13), on
//! the menus of `#open`, a channel, a DM view and a claude's row:
//!
//! - Hide messages here (and `/clear` on the tab shown), asked first: a
//!   watermark on the tab, local to this chat and kept across restarts
//!   ([`crate::hidden`]). Nothing on the bus changes. On an agent's row it
//!   acts on the operator's DMs with the agent. Show hidden messages, on
//!   the menu while the tab has a watermark, takes it away.
//! - Delete read messages for everyone…: Delete older than ▸ Current
//!   sessions, Today (local midnight), All or Custom… (a duration or a
//!   date, in the entry dialog). The pick runs `attend message delete`
//!   with `--dry-run` for the tab's scope (`--open`, `--channel <c>`, or
//!   `--dm <session>` for a DM view or an agent's row), and the `y`
//!   question carries what it reports; `y` runs it. The status line
//!   names the batch, and the scope's menu then offers Undo delete, which
//!   runs `attend message delete --undo <batch>`.
//! - Tombstone…, on a claude whose session is not live, by its row or its
//!   own DM view: asked, then `attend message tombstone <session-id>`.
//!   attend's refusal is shown as it words it.
//!
//! The verbs run through the attend CLI ([`crate::attend_cli`]) on a
//! worker thread: the status says `asking attend…`, the screen stays live,
//! a second request waits for the first, and an attend that gives no
//! answer in [`attend_cli::WAIT`] is stopped. An attend without the verbs
//! says it needs updating.

use agent_tui::{Anchor, Item, Open};

use super::menu::Step;
use super::ChatPane;
use crate::attend_cli::{self, Failure, Report, Scope};
use crate::hidden::{self, Hidden};
use crate::signal::{Channel, Signal};
use crate::tabs::{self, Tab};

pub(super) const HIDE_HERE: &str = "Hide messages here";
pub(super) const SHOW_HIDDEN: &str = "Show hidden messages";
pub(super) const DELETE_READ: &str = "Delete read messages for everyone…";
pub(super) const UNDO_DELETE: &str = "Undo delete";
pub(super) const TOMBSTONE: &str = "Tombstone…";
pub(super) const CUSTOM: &str = "Custom…";

/// The menu ids of Delete older than and of its Custom… dialog, before the
/// scope's key ([`Scope::key`]).
const OLDER: &str = "older:";
const OLDER_CUSTOM: &str = "older-custom:";

/// What the status line says while attend works on a request.
pub(super) const ASKING: &str = "asking attend…";

/// What the chat does with attend's answer to a request.
pub(super) enum Job {
    /// A delete's dry run, to ask with.
    DryRun { scope: Scope, older: String },
    /// The delete asked about.
    Delete(Asked),
    /// An undo of the last delete.
    Undo(LastDelete),
    /// A tombstone of the session `member`, named `name`.
    Tombstone { member: String, name: String },
}

/// The last delete this chat made: its scope, its batch and how many it
/// marked, for Undo delete on that scope's menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LastDelete {
    scope: Scope,
    batch: String,
    marked: usize,
}

/// A delete asked about: its scope, its `--older-than` and what the dry run
/// reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Asked {
    pub scope: Scope,
    pub older: String,
    pub label: String,
    pub report: Report,
}

impl Asked {
    /// The question: what goes, and what stays unread.
    pub(super) fn question(&self) -> String {
        let n = self.report.marked;
        format!("delete {n} read message{} in {}{}?{}", if n == 1 { "" } else { "s" }, self.label, attend_cli::older_words(&self.older), self.report.kept_words())
    }
}

impl ChatPane {
    /// The messages `tab` draws: those it shows, past its watermark.
    pub(super) fn in_view<'a>(&'a self, tab: &'a Tab) -> impl Iterator<Item = &'a Signal> + 'a {
        self.signals.iter().filter(move |s| tabs::visible(s, tab, &self.hidden))
    }

    /// The menu items of the delete pair for `scope`, with Undo delete
    /// while the last delete was there.
    pub(super) fn delete_items(&self, scope: &Scope) -> Vec<Item> {
        let mut out = vec![Item::new(DELETE_READ)];
        if self.undo.as_ref().is_some_and(|u| u.scope == *scope) {
            out.push(Item::new(UNDO_DELETE));
        }
        out
    }

    /// How a question names `scope`.
    fn scope_label(&self, scope: &Scope) -> String {
        match scope {
            Scope::Open => "#open".into(),
            Scope::Channel(c) => format!("#{c}"),
            Scope::Dm(m) => format!("your DMs with @{}", self.dm_name(m)),
        }
    }

    /// How a question names `tab`.
    pub(super) fn tab_label(&self, tab: &Tab) -> String {
        match tab {
            Tab::Merged => super::view::VIEW_LABEL.to_string(),
            Tab::Channel(c) => format!("#{c}"),
            Tab::Dm(m) => format!("@{} DMs", self.dm_name(m)),
            Tab::Review(m) => format!("{} DMs", self.dm_name(m)),
        }
    }

    /// Hide messages here, asked first.
    pub(super) fn ask_hide(&mut self, tab: Tab) {
        let label = self.tab_label(&tab);
        self.ask(Step::HideHere(tab, label));
    }

    /// Hide in `tab` every message it holds now: its watermark moves to the
    /// newest, and is kept for the next start.
    pub(super) fn hide_here(&mut self, tab: &Tab) {
        if self.dry_run {
            return self.say("dry run: nothing hidden", false);
        }
        // Bus messages only: a notice of the chat's, timed by the clock now,
        // would carry the mark past a message still on its way.
        let shown: Vec<hidden::Order> = self.in_view(tab).filter(|s| s.channel != Channel::Info).map(hidden::order).collect();
        let Some(newest) = shown.iter().max().copied() else {
            return self.say("nothing to hide here", false);
        };
        let n = shown.len();
        let what = format!("hid {n} message{} in {}; the bus keeps them", if n == 1 { "" } else { "s" }, self.tab_label(tab));
        match Hidden::hide_in(&self.hidden_path, tab, newest) {
            Ok(now) => {
                self.hidden = now;
                self.say(what, false);
            }
            Err(e) => {
                self.hidden.hide(tab, newest);
                self.say(format!("{what}, for this session only: {e}"), true);
            }
        }
        self.invalidate();
    }

    /// Show hidden messages: `tab`'s watermark taken away, every message it
    /// holds shown again.
    pub(super) fn show_hidden(&mut self, tab: &Tab) {
        if self.dry_run {
            return self.say("dry run: nothing shown", false);
        }
        let label = self.tab_label(tab);
        match Hidden::show_in(&self.hidden_path, tab) {
            Ok(now) => {
                self.hidden = now;
                self.say(format!("{label} shows every message again"), false);
            }
            Err(e) => self.say(format!("could not show {label}'s messages again: {e}"), true),
        }
        self.invalidate();
    }

    /// The hide items of `tab`'s menu: Hide messages here, and Show hidden
    /// messages while the tab has a watermark.
    pub(super) fn hide_items(&self, tab: &Tab) -> Vec<Item> {
        let mut out = vec![Item::new(HIDE_HERE)];
        if self.hidden.has(tab) {
            out.push(Item::new(SHOW_HIDDEN));
        }
        out
    }

    /// Delete read messages for everyone…: Delete older than, docked beside
    /// the item.
    pub(super) fn delete_menu(&mut self, scope: &Scope) {
        let mut items: Vec<Item> = attend_cli::OLDER_THAN.iter().map(|(label, _)| Item::new(*label)).collect();
        items.push(Item::new(CUSTOM));
        self.to_open = Some(Open::Pick { id: format!("{OLDER}{}", scope.key()), title: "delete older than".into(), items, multi: false, anchor: Anchor::Parent });
    }

    /// Custom…: the entry dialog for a duration or a date, opened on
    /// `value` with what was wrong with it, if anything.
    fn ask_custom(&mut self, scope: &Scope, value: String, error: Option<String>) {
        self.to_open = Some(Open::Entry { id: format!("{OLDER_CUSTOM}{}", scope.key()), title: "older than: 2h, 3d or YYYY-MM-DD".into(), value, error, anchor: Anchor::Parent });
    }

    /// A pick of Delete older than, or a Custom… entry; false when `id`
    /// names neither.
    pub(super) fn older_item(&mut self, id: &str, value: &str) -> bool {
        if let Some(scope) = id.strip_prefix(OLDER).and_then(Scope::from_key) {
            if value == CUSTOM {
                self.ask_custom(&scope, String::new(), None);
            } else if let Some((_, older)) = attend_cli::OLDER_THAN.iter().find(|(label, _)| *label == value) {
                self.delete_read(scope, older);
            }
            return true;
        }
        if let Some(scope) = id.strip_prefix(OLDER_CUSTOM).and_then(Scope::from_key) {
            match attend_cli::custom(value) {
                Ok(older) => self.delete_read(scope, &older),
                Err(why) => self.ask_custom(&scope, value.to_string(), Some(why)),
            }
            return true;
        }
        false
    }

    /// Ask before a delete of `scope` older than `older`, with what its dry
    /// run reports; with nothing to delete, say so and ask nothing. A dry
    /// run of the chat (`--snap`) runs no attend at all, so its frames do
    /// not depend on the attend installed.
    pub(super) fn delete_read(&mut self, scope: Scope, older: &str) {
        if self.dry_run {
            return self.say("dry run: attend message delete not run", false);
        }
        let args = attend_cli::delete_argv(&scope, older, true);
        self.ask_attend(args, Job::DryRun { scope, older: older.to_string() });
    }

    /// The delete asked about, answered yes: run it.
    pub(super) fn run_delete(&mut self, asked: Asked) {
        if self.dry_run {
            return self.say("dry run: attend message delete not run", false);
        }
        let args = attend_cli::delete_argv(&asked.scope, &asked.older, false);
        self.ask_attend(args, Job::Delete(asked));
    }

    /// Undo delete: the last delete's marks removed, its messages delivered
    /// again.
    pub(super) fn undo_delete(&mut self) {
        let Some(last) = self.undo.clone() else { return };
        if self.dry_run {
            return self.say("dry run: attend message delete --undo not run", false);
        }
        self.ask_attend(attend_cli::undo_argv(&last.batch), Job::Undo(last));
    }

    /// Whether `member` is a claude that has gone, by the predicate attend's
    /// tombstone accepts (`attend_presence::gone`): one Tombstone… is
    /// offered for. A quiet session that has not gone gets no offer attend
    /// would refuse.
    pub(super) fn is_dead_claude(&self, member: &str) -> bool {
        self.world.as_ref().is_some_and(|w| w.seen.iter().any(|s| s.member == member && s.identity.is_claude && !s.live)) && attend_presence::gone(member)
    }

    /// Tombstone…, asked first.
    pub(super) fn ask_tombstone(&mut self, member: &str) {
        let name = self.dm_name(member);
        self.ask(Step::Tombstone(member.to_string(), name));
    }

    /// The tombstone asked about, answered yes.
    pub(super) fn tombstone(&mut self, member: &str, name: &str) {
        if self.dry_run {
            return self.say("dry run: attend message tombstone not run", false);
        }
        self.ask_attend(attend_cli::tombstone_argv(member), Job::Tombstone { member: member.to_string(), name: name.to_string() });
    }

    /// Run attend with `args` on a worker thread for `job`; the screen stays
    /// live and the answer is taken on a tick ([`ChatPane::attend_answered`]).
    /// While one is out, another is refused.
    fn ask_attend(&mut self, args: Vec<String>, job: Job) {
        if self.asking.is_some() {
            return self.say("attend is still answering the last request", false);
        }
        self.asking = Some((attend_cli::start(self.attend.clone(), args, self.attend_wait), job));
        self.say(ASKING, false);
    }

    /// Take attend's answer once it has come, and act on it.
    pub(super) fn attend_answered(&mut self) {
        let Some(answer) = self.asking.as_ref().and_then(|(running, _)| running.poll()) else { return };
        let Some((_, job)) = self.asking.take() else { return };
        // The answer replaces `asking attend…`, whatever it leads to.
        self.say("", false);
        match job {
            Job::DryRun { scope, older } => self.dry_ran(scope, older, answer),
            Job::Delete(asked) => self.deleted(asked, answer),
            Job::Undo(last) => self.undid(last, answer),
            Job::Tombstone { member, name } => match answer {
                Ok(_) => self.say(format!("tombstoned @{name}: its messages leave every tab · undo: attend message tombstone --undo {member}"), false),
                Err(e) => self.say(e.to_string(), true),
            },
        }
        self.stale();
    }

    /// A dry run's report: ask with its counts, or say there is nothing.
    fn dry_ran(&mut self, scope: Scope, older: String, answer: Result<serde_json::Value, Failure>) {
        let report = match answer.and_then(|doc| attend_cli::report(&doc)) {
            Ok(r) => r,
            Err(e) => return self.say(e.to_string(), true),
        };
        let label = self.scope_label(&scope);
        if report.marked == 0 {
            return self.say(format!("nothing read to delete in {label}{}{}", attend_cli::older_words(&older), report.kept_words()), false);
        }
        self.ask(Step::DeleteRead(Asked { scope, older, label, report }));
    }

    /// A delete's report: said, and its batch kept for Undo delete.
    fn deleted(&mut self, asked: Asked, answer: Result<serde_json::Value, Failure>) {
        match answer.and_then(|doc| attend_cli::report(&doc)) {
            Ok(r) => {
                let n = r.marked;
                let mut said = format!("deleted {n} message{} in {}{}", if n == 1 { "" } else { "s" }, asked.label, r.kept_words());
                if let Some(batch) = r.batch {
                    said += &format!(" · Undo delete on its menu · batch {batch}");
                    self.undo = Some(LastDelete { scope: asked.scope, batch, marked: n });
                }
                self.say(said, false);
            }
            Err(e) => self.say(e.to_string(), true),
        }
    }

    /// An undo's report.
    fn undid(&mut self, last: LastDelete, answer: Result<serde_json::Value, Failure>) {
        match answer.and_then(|doc| attend_cli::undone(&doc)) {
            Ok(u) => {
                self.undo = None;
                let gone = match u.already_removed {
                    0 => String::new(),
                    n => format!("; cleanup had already removed {n}"),
                };
                self.say(format!("restored {} of {} message(s) from batch {}{gone}", u.unmarked, last.marked, last.batch), false);
            }
            Err(e) => self.say(e.to_string(), true),
        }
    }
}
