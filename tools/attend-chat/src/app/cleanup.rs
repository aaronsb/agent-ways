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
//! The verbs run through the attend CLI ([`crate::attend_cli`]), every
//! form of `attend message delete` (the dry run, the delete, the undo) and
//! `attend message tombstone`, on a thread of their own with no time
//! limit (ADR-405, amendment of 2026-10-14): a delete runs to its end. The
//! status line says what is running and for how long (`deleting… 12 s`)
//! until it ends, a fresh result taking the line for its few seconds, and
//! the screen stays live. `≡` ▸ Stop deleting stops it, and the exit guard
//! names it. One runs at a time: another asked for meanwhile is refused
//! with a note. A count that lands while something is open over the chat,
//! or after the operator typed, asks nothing: it is held for [`HOLD`] and
//! said, and choosing the delete again asks from it. An attend without the
//! verbs says it needs updating.

use std::time::{Duration, Instant};

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

/// How long a count held for a delete stays good to ask from.
const HOLD: Duration = Duration::from_secs(60);

/// How long the operator must have been still before a count's question
/// opens on its own: a keystroke begun before it showed must not land on it.
pub(super) const IDLE_BEFORE_ASKING: Duration = Duration::from_millis(1500);

/// The menu ids of Delete older than and of its Custom… dialog, before the
/// scope's key ([`Scope::key`]).
const OLDER: &str = "older:";
const OLDER_CUSTOM: &str = "older-custom:";

/// What the chat does with attend's answer to a request.
pub(super) enum Job {
    /// A delete's dry run, to ask with, and the compose box's text when it
    /// was asked for.
    DryRun { scope: Scope, older: String, draft: String },
    /// The delete asked about.
    Delete(Asked),
    /// An undo of the last delete.
    Undo(LastDelete),
    /// A tombstone of the session `member`, named `name`.
    Tombstone { member: String, name: String },
}

/// The delete or tombstone running, with no time limit: attend working,
/// what to do with its answer, and since when.
pub(super) struct Removal {
    running: attend_cli::Running,
    job: Job,
    began: Instant,
}

/// What the `≡` menu's item that stops a running delete or tombstone
/// starts with: `Stop deleting`.
pub(super) const STOP: &str = "Stop ";

impl Job {
    /// What the chat is doing while attend works on the job.
    pub(super) fn doing(&self) -> &'static str {
        match self {
            Job::DryRun { .. } => "counting read messages",
            Job::Delete(_) => "deleting",
            Job::Undo(_) => "undoing the delete",
            Job::Tombstone { .. } => "tombstoning",
        }
    }
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
    /// the item; refused while a delete or a tombstone runs.
    pub(super) fn delete_menu(&mut self, scope: &Scope) {
        if self.refused_while_removing() || self.ask_held(|a| a.scope == *scope) {
            return;
        }
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
        if self.ask_held(|a| a.scope == scope && a.older == older) {
            return;
        }
        let args = attend_cli::delete_argv(&scope, older, true);
        let draft = self.input.text().to_string();
        self.ask_attend(args, Job::DryRun { scope, older: older.to_string(), draft });
    }

    /// Ask from the count held for a delete `matches` picks, if one landed
    /// in the last [`HOLD`]; true when it asked.
    fn ask_held(&mut self, matches: impl Fn(&Asked) -> bool) -> bool {
        let Some((asked, _)) = self.held.take_if(|(a, at)| at.elapsed() < HOLD && matches(a)) else { return false };
        self.ask(Step::DeleteRead(asked));
        true
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
        if self.refused_while_removing() {
            return;
        }
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
        self.world.as_ref().is_some_and(|w| w.seen.iter().any(|s| s.member == member && s.identity.is_claude && !s.live))
            && attend_presence::gone(member)
            && attend_groups::readers::read_as_live(member).is_none()
    }

    /// Tombstone…, asked first; refused while a delete or a tombstone runs.
    pub(super) fn ask_tombstone(&mut self, member: &str) {
        if self.refused_while_removing() {
            return;
        }
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

    /// What the delete or tombstone running now is doing: `deleting…`.
    pub(super) fn removing_now(&self) -> Option<String> {
        self.removing.as_ref().map(|r| format!("{}…", r.job.doing()))
    }

    /// The status line while a delete or tombstone runs: what it is doing,
    /// for how long, and where it is stopped.
    pub(super) fn busy_line(&self) -> Option<String> {
        self.removing.as_ref().map(|r| format!("{}… {} s · stop it from ≡", r.job.doing(), r.began.elapsed().as_secs()))
    }

    /// What is running, as the exit guard names it: `a delete still
    /// running`.
    pub(super) fn running_what(&self) -> Option<&'static str> {
        self.removing.as_ref().map(|r| match r.job {
            Job::Tombstone { .. } => "a tombstone still running",
            _ => "a delete still running",
        })
    }

    /// The `≡` menu's item that stops the delete or tombstone running.
    pub(super) fn stop_item(&self) -> Option<String> {
        self.removing.as_ref().map(|r| format!("{STOP}{}", r.job.doing()))
    }

    /// Stop the delete or tombstone running: attend is killed, the chat
    /// takes no answer from it, and the status line says what that leaves.
    pub(super) fn stop_removing(&mut self) {
        let Some(r) = self.removing.take() else { return };
        r.running.stop();
        let note = match r.job {
            Job::DryRun { .. } => "stopped counting: nothing was deleted".to_string(),
            Job::Delete(asked) => format!("stopped deleting in {}: any it marked already come back with `attend message delete --undo`, which takes back the newest delete", asked.label),
            Job::Undo(last) => format!("stopped undoing batch {}: Undo delete on its menu runs it again", last.batch),
            Job::Tombstone { member, name } => format!("stopped tombstoning @{name}: if it went through, `attend message tombstone --undo {member}` takes it back"),
        };
        self.say(note, false);
        self.stale();
    }

    /// Whether a delete or a tombstone is running; if so, the request is
    /// refused and the status line says why.
    pub(super) fn refused_while_removing(&mut self) -> bool {
        let Some(Removal { job, .. }) = &self.removing else { return false };
        let note = format!("still {}: one delete or tombstone runs at a time", job.doing());
        self.say(note, true);
        true
    }

    /// Run attend with `args` for `job` on a thread of its own, to its end
    /// however long it takes; the screen stays live and the answer is taken
    /// on a tick ([`ChatPane::attend_answered`]).
    fn ask_attend(&mut self, args: Vec<String>, job: Job) {
        if self.refused_while_removing() {
            return;
        }
        // Said set back, not fresh: the status line shows the running line,
        // with its seconds and its stop, from the start.
        self.say(format!("{}…", job.doing()), false);
        self.status_set_at = None;
        self.removing = Some(Removal { running: attend_cli::start(self.attend.clone(), args, None), job, began: Instant::now() });
    }

    /// Take attend's answer once it has come, and act on it.
    pub(super) fn attend_answered(&mut self) {
        let Some(answer) = self.removing.as_ref().and_then(|r| r.running.poll()) else { return };
        let Some(Removal { job, .. }) = self.removing.take() else { return };
        // The answer replaces `deleting…`, whatever it leads to.
        self.say("", false);
        match job {
            Job::DryRun { scope, older, draft } => self.dry_ran(scope, older, draft, answer),
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
    /// The count can land at any time, so its question is the chat's own
    /// ([`ChatPane::ask_unprompted`]), and opens only once the operator has
    /// been still for `idle_before_asking` ([`ChatPane::ask_count_due`]).
    fn dry_ran(&mut self, scope: Scope, older: String, draft: String, answer: Result<serde_json::Value, Failure>) {
        let report = match answer.and_then(|doc| attend_cli::report(&doc)) {
            Ok(r) => r,
            Err(e) => return self.say(e.to_string(), true),
        };
        let label = self.scope_label(&scope);
        if report.marked == 0 {
            return self.say(format!("nothing read to delete in {label}{}{}", attend_cli::older_words(&older), report.kept_words()), false);
        }
        self.count_due = Some((Asked { scope, older, label, report }, draft, Instant::now()));
        self.ask_count_due();
    }

    /// The count that landed, asked once the operator has been still long
    /// enough, or held when it cannot be asked: something is open over the
    /// chat or about to be, the compose box changed since the delete was
    /// chosen, or the operator gave input after the count landed (they have
    /// moved on).
    pub(super) fn ask_count_due(&mut self) {
        let Some((_, draft, landed)) = &self.count_due else { return };
        let moved_on = self.last_input.is_some_and(|t| t > *landed);
        if moved_on || !self.can_ask_unprompted(draft) {
            let (asked, ..) = self.count_due.take().expect("just seen");
            return self.hold_count(asked);
        }
        if self.last_input.is_none_or(|t| t.elapsed() >= self.idle_before_asking) {
            let (asked, ..) = self.count_due.take().expect("just seen");
            self.ask_unprompted(Step::DeleteRead(asked));
        }
    }

    /// Hold `asked`'s count, and say so: choosing the delete again soon
    /// asks from it.
    pub(super) fn hold_count(&mut self, asked: Asked) {
        let n = asked.report.marked;
        self.say(format!("counted {n} read message{} in {}{}: choose Delete read messages again to delete them", if n == 1 { "" } else { "s" }, asked.label, attend_cli::older_words(&asked.older)), false);
        self.held = Some((asked, Instant::now()));
    }

    /// Whether a question the operator did not just ask for may open now:
    /// nothing is open over the chat or about to open (the record of what
    /// is open is told at each frame, so a question still pending counts
    /// too), and the compose box holds what it held when the work began, or
    /// does not have the keyboard.
    fn can_ask_unprompted(&self, draft: &str) -> bool {
        let typing = self.ix.owns_text() && self.input.text() != draft;
        !self.ix.overlaid() && self.asked.is_none() && self.to_open.is_none() && !typing
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

#[cfg(test)]
mod tests {
    use super::*;

    fn landed(c: &mut ChatPane) {
        let doc = serde_json::json!({"scope": "#slow", "dry_run": true, "marked": 3, "batch": null, "ids": [], "held": []});
        c.dry_ran(Scope::Channel("slow".into()), "all".into(), String::new(), Ok(doc));
    }

    /// A count lands between the frame that opened a question and the next
    /// one, so the record of what is open does not have it yet: the
    /// question pending still keeps the count from asking.
    #[test]
    fn a_count_never_takes_the_place_of_a_question_pending() {
        let mut c = ChatPane::new(None, Default::default());
        c.dry_run = true;
        c.ask(Step::Leave("deploy".into()));
        let pending = c.asked.clone();
        c.to_open = None;
        landed(&mut c);
        assert_eq!(c.asked, pending, "the leave still waits for its answer");
        assert!(c.to_open.is_none(), "no second question");
        assert!(c.held.is_some() && c.status.starts_with("counted 3 read messages in #slow"), "{}", c.status);
        c.asked = None;
        landed(&mut c);
        assert!(matches!(&c.to_open, Some(Open::Ask { question, unprompted: true, .. }) if question.starts_with("delete 3 read messages in #slow")), "over nothing it asks, on its own");
    }

    /// A count asks only once the operator has been still: input a moment
    /// before it landed defers the question, and input after it landed holds
    /// the count instead.
    #[test]
    fn a_count_waits_for_the_operator_to_be_still() {
        let mut c = ChatPane::new(None, Default::default());
        c.dry_run = true;
        c.last_input = Some(Instant::now());
        landed(&mut c);
        assert!(c.to_open.is_none() && c.count_due.is_some(), "input a moment ago: not yet");
        c.ask_count_due();
        assert!(c.to_open.is_none(), "still too soon");
        c.last_input = Some(Instant::now());
        c.ask_count_due();
        assert!(c.count_due.is_none() && c.to_open.is_none(), "input after it landed: held, not asked");
        assert!(c.held.is_some() && c.status.starts_with("counted 3 read messages in #slow"), "{}", c.status);
        c.held = None;
        c.idle_before_asking = Duration::from_millis(50);
        c.last_input = Some(Instant::now());
        landed(&mut c);
        std::thread::sleep(Duration::from_millis(80));
        c.ask_count_due();
        assert!(matches!(&c.to_open, Some(Open::Ask { unprompted: true, .. })), "still long enough: it asks");
    }
}
