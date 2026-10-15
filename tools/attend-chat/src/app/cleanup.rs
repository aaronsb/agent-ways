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
//!   date, in the entry dialog). The pick opens its question at once,
//!   saying it is counting, and runs `attend message delete --dry-run`
//!   for the tab's scope (`--open`, `--channel <c>`, or `--dm <session>`
//!   for a DM view or an agent's row). Its yes waits for the count; the
//!   count updates the same question in place, which then asks to delete
//!   that many for everyone and expires a minute later as a no
//!   ([`agent_tui::Expiry`]). A count of nothing, or attend's refusal,
//!   leaves only no. No, at any time, stops a count still running; `y`
//!   runs the delete. The status line names the batch, and the scope's
//!   menu then offers Undo delete, which runs `attend message delete
//!   --undo <batch>`.
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
//! with a note. An attend without the verbs says it needs updating.
//!
//! Every question the chat asks answers a pick the operator just made, and
//! a count only updates the question its pick opened: it can never answer
//! or replace another (ADR-405, amendment of 2026-10-14).

use std::time::{Duration, Instant};

use agent_tui::{Anchor, Expiry, Item, Open, Question};

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

/// How long a question that removes something for everyone stays open,
/// before it closes as a no: a delete's once its count has landed, and
/// Delete channel's, Tombstone…'s and Remove agent's from when they open.
pub(super) const DELETE_QUESTION_LIFE: Duration = Duration::from_secs(60);

/// The menu ids of Delete older than and of its Custom… dialog, before the
/// scope's key ([`Scope::key`]).
const OLDER: &str = "older:";
const OLDER_CUSTOM: &str = "older-custom:";

/// What the chat does with attend's answer to a request.
pub(super) enum Job {
    /// A delete's dry run, for the question `ask` its pick opened.
    DryRun { ask: String },
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

/// A delete counted and confirmed: its scope, its `--older-than`, how the
/// question named the scope, and what the dry run reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Asked {
    pub scope: Scope,
    pub older: String,
    pub label: String,
    pub report: Report,
}

/// A delete's question, from the pick of its age: its scope, its age, how
/// it names the scope, and where its count is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DeleteAsk {
    pub scope: Scope,
    pub older: String,
    pub label: String,
    pub count: Count,
}

/// Where a delete question's count is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Count {
    /// The dry run is running: yes waits.
    Counting,
    /// What the dry run found to delete: yes deletes it.
    Found(Report),
    /// Nothing to delete: only no.
    Nothing(String),
    /// Why attend did not count: only no, and dismissed, it stays on the
    /// status line as the error it is.
    Refused(String),
}

impl DeleteAsk {
    /// The question under `id`, as its count stands, expiring `life` after
    /// the count is shown.
    pub(super) fn question(&self, id: String, life: Duration) -> Question {
        let older = attend_cli::older_words(&self.older);
        match &self.count {
            Count::Counting => Question::new(id, format!("counting read messages{older} in {}…", self.label), "delete", "keep").pending(),
            Count::Found(r) => {
                let n = r.marked;
                let text = format!("delete {n} read message{}{older} in {} for everyone?{}", if n == 1 { "" } else { "s" }, self.label, r.kept_words());
                Question::new(id, text, "delete", "keep").expires(Expiry::new(life))
            }
            Count::Nothing(why) | Count::Refused(why) => Question::new(id, why.clone(), "delete", "close").dismiss_only(),
        }
    }

    /// The delete to run, once the count has found something.
    pub(super) fn counted(self) -> Option<Asked> {
        let Count::Found(report) = self.count else { return None };
        Some(Asked { scope: self.scope, older: self.older, label: self.label, report })
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
        let what = format!("hid {n} message{} in {}; nothing was deleted", if n == 1 { "" } else { "s" }, self.tab_label(tab));
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
        if self.refused_while_removing() {
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

    /// The pick of a delete of `scope` older than `older`: its question
    /// opens at once, an answer to this pick, saying it is counting, and
    /// the dry run starts; its count updates the question in place
    /// ([`ChatPane::attend_answered`]). A dry run of the chat (`--snap`)
    /// runs no attend at all, so its frames do not depend on the attend
    /// installed.
    pub(super) fn delete_read(&mut self, scope: Scope, older: &str) {
        if self.dry_run {
            return self.say("dry run: attend message delete not run", false);
        }
        if self.refused_while_removing() {
            return;
        }
        let args = attend_cli::delete_argv(&scope, older, true);
        let label = self.scope_label(&scope);
        let ask = self.ask(Step::DeleteRead(DeleteAsk { scope, older: older.to_string(), label, count: Count::Counting }));
        self.ask_attend(args, Job::DryRun { ask });
    }

    /// Stop the count running for the question `ask`, if one is: its
    /// question was answered no or let go.
    pub(super) fn stop_count(&mut self, ask: &str) {
        if self.removing.as_ref().is_some_and(|r| matches!(&r.job, Job::DryRun { ask: running } if running == ask)) {
            if let Some(r) = self.removing.take() {
                r.running.stop();
            }
        }
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
            Job::DryRun { .. } => "a count still running",
            Job::Tombstone { .. } => "a tombstone still running",
            Job::Delete(_) | Job::Undo(_) => "a delete still running",
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
            Job::Delete(asked) => format!("stopped deleting in {}: the ones it already marked come back with `attend message delete --undo`, which takes back the newest delete", asked.label),
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
            Job::DryRun { ask } => self.dry_ran(&ask, answer),
            Job::Delete(asked) => self.deleted(asked, answer),
            Job::Undo(last) => self.undid(last, answer),
            Job::Tombstone { member, name } => match answer {
                Ok(_) => self.say(format!("tombstoned @{name}: its messages leave every tab · undo: attend message tombstone --undo {member}"), false),
                Err(e) => self.say(e.to_string(), true),
            },
        }
        self.stale();
    }

    /// A dry run's report, into the question `ask` that its pick opened,
    /// in place: what it found to delete, that there is nothing, or why
    /// attend did not count. Only that question: a count never touches
    /// another, and one whose question is gone is let go.
    fn dry_ran(&mut self, ask: &str, answer: Result<serde_json::Value, Failure>) {
        let Some((id, Step::DeleteRead(d))) = &mut self.asked else { return };
        if id != ask {
            return;
        }
        d.count = match answer.and_then(|doc| attend_cli::report(&doc)) {
            Ok(r) if r.marked == 0 => Count::Nothing(format!("no read messages{} in {}{}", attend_cli::older_words(&d.older), d.label, r.kept_words())),
            Ok(r) => Count::Found(r),
            Err(e) => Count::Refused(e.to_string()),
        };
        self.to_open = Some(Open::Ask(d.question(id.clone(), self.delete_question_life)));
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

    fn count(marked: u64) -> Result<serde_json::Value, Failure> {
        Ok(serde_json::json!({"scope": "#slow", "dry_run": true, "marked": marked, "batch": null, "ids": [], "held": []}))
    }

    /// A pane with a delete question open on `#slow`, counting, as a pick
    /// opens it.
    fn counting() -> (ChatPane, String) {
        let mut c = ChatPane::new(None, Default::default());
        let id = c.ask(Step::DeleteRead(DeleteAsk { scope: Scope::Channel("slow".into()), older: "all".into(), label: "#slow".into(), count: Count::Counting }));
        (c, id)
    }

    fn opened(c: &mut ChatPane) -> Question {
        match c.to_open.take() {
            Some(Open::Ask(q)) => q,
            other => panic!("no question: {other:?}"),
        }
    }

    /// The pick's question opens at once, counting, its yes waiting; the
    /// count updates the same question in place, ready and expiring.
    #[test]
    fn a_count_updates_its_own_question_in_place() {
        let (mut c, id) = counting();
        let q = opened(&mut c);
        assert_eq!((q.id.as_str(), q.question.as_str(), q.pending), (id.as_str(), "counting read messages in #slow…", true));
        c.dry_ran(&id, count(3));
        let q = opened(&mut c);
        assert_eq!(q.id, id, "the same question");
        assert_eq!(q.question, "delete 3 read messages in #slow for everyone?");
        assert!(!q.pending && q.expiry.as_ref().is_some_and(|e| e.after == DELETE_QUESTION_LIFE && !e.answers_yes()), "{q:?}");
    }

    /// A count of nothing, or attend's refusal, leaves only no.
    #[test]
    fn a_count_of_nothing_or_a_refusal_offers_only_no() {
        let (mut c, id) = counting();
        c.to_open = None;
        c.dry_ran(&id, count(0));
        let q = opened(&mut c);
        assert_eq!((q.question.as_str(), q.yes.clone()), ("no read messages in #slow", None));
        let (mut c, id) = counting();
        c.to_open = None;
        c.dry_ran(&id, Err(Failure::TooOld { verb: "message delete".into() }));
        let q = opened(&mut c);
        assert_eq!((q.question.as_str(), q.yes.clone()), ("`attend message delete` is missing: needs a newer attend", None));
    }

    /// No to a question that says attend refused leaves the refusal on the
    /// status line, as an error; no to one that found nothing leaves what it
    /// found.
    #[test]
    fn a_dismissed_refusal_stays_on_the_status_line_as_an_error() {
        let (mut c, id) = counting();
        c.dry_ran(&id, Err(Failure::TooOld { verb: "message delete".into() }));
        c.answered(&id, false);
        assert_eq!((c.status.as_str(), c.status_is_error), ("`attend message delete` is missing: needs a newer attend", true));
        let (mut c, id) = counting();
        c.dry_ran(&id, count(0));
        c.answered(&id, false);
        assert_eq!((c.status.as_str(), c.status_is_error), ("no read messages in #slow", false));
    }

    /// A count never touches a question but its own: not another delete's
    /// question, counting too, which only the id tells apart, nor a
    /// question of another kind.
    #[test]
    fn a_count_never_touches_another_question() {
        let (mut c, id) = counting();
        let other = c.ask(Step::DeleteRead(DeleteAsk { scope: Scope::Channel("deploy".into()), older: "all".into(), label: "#deploy".into(), count: Count::Counting }));
        assert_ne!(other, id);
        let pending = c.asked.clone();
        c.to_open = None;
        c.dry_ran(&id, count(3));
        assert_eq!(c.asked, pending, "the other delete's question still counts");
        assert!(c.to_open.is_none(), "nothing opened or updated");
        let (mut c, id) = counting();
        c.ask(Step::Leave("deploy".into()));
        let pending = c.asked.clone();
        c.to_open = None;
        c.dry_ran(&id, count(3));
        assert_eq!(c.asked, pending, "the leave keeps its question");
        assert!(c.to_open.is_none(), "nothing opened or updated");
    }

    /// An expired delete question is let go, and says so.
    #[test]
    fn an_expired_delete_question_is_let_go() {
        let (mut c, id) = counting();
        c.dry_ran(&id, count(3));
        c.answered_expired(&id);
        assert!(c.asked.is_none());
        assert_eq!(c.status, "delete cancelled: no answer in 60 s");
    }

    /// A question the shell drops leaves nothing waiting on its answer.
    #[test]
    fn a_dropped_question_is_let_go() {
        let (mut c, _) = counting();
        let open = c.to_open.take().expect("it asked");
        c.question_dropped(open);
        assert!(c.asked.is_none(), "nothing waits on an answer that will not come");
    }
}
