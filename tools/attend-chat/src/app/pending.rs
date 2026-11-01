//! Messages held after Enter before they are sent (`attend.chat.send_delay`).
//!
//! Enter on a message holds it here, shown in the feed with a countdown, and
//! it is sent when the countdown ends, through the path Enter sends by. While
//! it waits, Esc in the compose box recalls the newest one into the box and a
//! click recalls the one clicked; nothing held is sent then. Enter on another
//! message to the same route, while one waits, joins it on a new line and
//! starts its countdown again, so a burst goes out as one message. A
//! different route waits on its own.
//!
//! When a countdown ends the route is resolved again. A message whose route
//! has changed, such as one held for a channel dissolved meanwhile, is not
//! sent elsewhere: it stays in the feed marked not sent, as does one the send
//! refuses then. Either comes back only when Esc or a click recalls it, into
//! the view it was written in, so the draft being typed meanwhile is left as
//! it is and routes as it would have.
//!
//! A recall that shows another view parks the draft in the box as a draft
//! entry of its own view, and the draft comes back into the box when that
//! view is shown again with the box empty. Text the earlier chats kept
//! ([`super::unsent`]) comes back the same way: each text as an entry of its
//! own, recalled into its own view, and the newest draft whose view is still
//! there in the box of that view. Nothing sends a draft or a restored entry
//! until the operator recalls it and presses Enter.
//!
//! The queue is pure state over the chat's monotonic clock
//! ([`ChatPane::mono`]): every method takes the time it acts at, so the
//! tests step the clock rather than sleep, and a pinned clock holds a
//! frame's countdown still.

use std::time::{Duration, UNIX_EPOCH};

use agent_tui::feed::Entry;
use agent_tui::ratatui::text::Line;
use agent_tui::theme::{self, Ground, Seg};

use super::ix::Ev;
use super::{unsent, ChatPane, Clock};
use crate::legend::{parse_addressed, Addressed};
use crate::tabs::{self, Tab};

/// One message held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Held {
    /// Its own number, for a click on its entry.
    pub(super) id: u64,
    /// What is sent: each message joined, a line apart.
    pub(super) text: String,
    /// The view it was written in, which routes it as Enter routed it there.
    pub(super) tab: Tab,
    /// Its route as the send resolves it ([`ChatPane::destination`]): what
    /// joins two messages, and what the send checks again at the end of the
    /// countdown.
    pub(super) dest: String,
    /// Where it goes, as the destination flag named it.
    pub(super) label: String,
    /// When it is sent, on the chat's monotonic clock.
    pub(super) due: Duration,
    /// Set when the send at the end of the countdown did not go: the
    /// message stays, recallable, and nothing sends it by itself.
    pub(super) refused: Option<Refused>,
    /// When it was last written to: the newest is recalled first.
    touched: u64,
    /// How it came to be in the feed.
    pub(super) source: Source,
}

/// How an entry came to be in the feed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Source {
    /// Enter held it: it counts down, or a send refused it. Send and quit
    /// and the unasked end send it.
    Enter,
    /// The draft of its view, parked when a recall showed another view.
    /// `origins` are the kept files whose text it carries.
    Parked { origins: Vec<std::path::PathBuf> },
    /// Restored from the kept file `path`, which stays claimed until the
    /// text is sent, held or kept again. `draft` when it was kept as a
    /// draft; `view_known` false for a file of an earlier build;
    /// `partial` when the file was cut short; `maybe_sent` when it was
    /// being sent as a chat ended ([`unsent::Kind::Sending`]).
    Kept { path: std::path::PathBuf, draft: bool, view_known: bool, partial: bool, maybe_sent: bool },
}

impl Held {
    /// Text the operator never pressed Enter on.
    pub(super) fn is_draft(&self) -> bool {
        matches!(self.source, Source::Parked { .. } | Source::Kept { draft: true, .. })
    }

    /// Whether Send and quit and the unasked end send it: only what Enter
    /// held in this chat.
    fn sendable(&self) -> bool {
        self.source == Source::Enter
    }
}

/// A send that did not go at the end of a countdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Refused {
    /// The agent it addresses was quiet. Recalled unchanged into its own
    /// view, Enter sends it anyway, as Enter again does after a refusal at
    /// Enter.
    pub(super) quiet: bool,
}

/// What [`Pending::hold`] did with a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Hold {
    /// No delay and nothing waiting for its route: send it now, as Enter
    /// always did.
    Now,
    /// Held as the entry with this id, new or joined.
    Held(u64),
}

/// The messages held, in the order they were first held.
#[derive(Debug, Clone, Default)]
pub(super) struct Pending {
    held: Vec<Held>,
    next_id: u64,
    writes: u64,
}

impl Pending {
    pub(super) fn is_empty(&self) -> bool {
        self.held.is_empty()
    }

    /// The messages held, oldest first.
    pub(super) fn iter(&self) -> impl Iterator<Item = &Held> {
        self.held.iter()
    }

    /// Hold `text`, written in `tab`, for the route `dest`, at `now`, for
    /// `delay`. A message still counting down for `dest` takes it on a new
    /// line and waits `delay` again from `now`; one a send refused takes
    /// nothing. With no delay, a message nothing waits for is sent at once
    /// ([`Hold::Now`]); one that joins a waiting message is due at once, so
    /// the two go out together.
    pub(super) fn hold(&mut self, text: &str, tab: Tab, dest: String, label: String, now: Duration, delay: Duration) -> Hold {
        self.writes += 1;
        let due = now + delay;
        if let Some(h) = self.held.iter_mut().find(|h| h.refused.is_none() && h.dest == dest) {
            h.text = format!("{}\n{text}", h.text);
            h.due = due;
            h.touched = self.writes;
            return Hold::Held(h.id);
        }
        if delay.is_zero() {
            return Hold::Now;
        }
        self.next_id += 1;
        self.held.push(Held { id: self.next_id, text: text.to_string(), tab, dest, label, due, refused: None, touched: self.writes, source: Source::Enter });
        Hold::Held(self.next_id)
    }

    /// Put `h` back as refused, in its place among the others, written to
    /// last so Esc recalls it first.
    pub(super) fn refuse(&mut self, mut h: Held, refused: Refused) {
        self.writes += 1;
        h.refused = Some(refused);
        h.touched = self.writes;
        let at = self.held.iter().position(|o| o.id > h.id).unwrap_or(self.held.len());
        self.held.insert(at, h);
    }

    /// Take the message written to last, which Esc recalls.
    pub(super) fn recall_newest(&mut self) -> Option<Held> {
        let at = self.held.iter().enumerate().max_by_key(|(_, h)| h.touched).map(|(i, _)| i)?;
        Some(self.held.remove(at))
    }

    /// Take the message `id`, which a click on its entry recalls.
    pub(super) fn recall(&mut self, id: u64) -> Option<Held> {
        let at = self.held.iter().position(|h| h.id == id)?;
        Some(self.held.remove(at))
    }

    /// Take every message counting down that is due by `now`, in the order
    /// they were held. The chat sends them one at a time
    /// ([`ChatPane::send_due`]); the tests take them together.
    #[cfg(test)]
    pub(super) fn due(&mut self, now: Duration) -> Vec<Held> {
        let (due, kept) = std::mem::take(&mut self.held).into_iter().partition(|h| h.refused.is_none() && h.due <= now);
        self.held = kept;
        due
    }

    /// Take every message, due, refused or not: sent now or discarded as
    /// the operator quits.
    pub(super) fn take_all(&mut self) -> Vec<Held> {
        std::mem::take(&mut self.held)
    }

    /// Take every entry `keep` picks, leaving the rest in place.
    fn take_where(&mut self, keep: impl Fn(&Held) -> bool) -> Vec<Held> {
        let (taken, left) = std::mem::take(&mut self.held).into_iter().partition(keep);
        self.held = left;
        taken
    }

    /// Add an entry that never counts down: a parked draft, or one
    /// restored. Its text, view and route are `h`'s; it is numbered here,
    /// and written to after everything before it unless `last` is false.
    fn add(&mut self, mut h: Held, last: bool) -> u64 {
        self.next_id += 1;
        self.writes += 1;
        h.id = self.next_id;
        h.touched = if last { self.writes } else { 0 };
        h.refused = Some(Refused { quiet: false });
        self.held.push(h);
        self.next_id
    }

    /// How many entries Send and quit would send.
    fn sendable(&self) -> usize {
        self.held.iter().filter(|h| h.sendable()).count()
    }

    /// The whole seconds left on each message's countdown at `now`, rounded
    /// up, oldest first, `None` for one refused: what the feed shows, so it
    /// is drawn again when one of them changes.
    pub(super) fn countdown(&self, now: Duration) -> Vec<Option<u64>> {
        self.held.iter().map(|h| h.refused.is_none().then(|| left(h, now))).collect()
    }
}

/// The whole seconds left before `h` is sent at `now`, rounded up: `5s` the
/// moment it is held for five seconds, `1s` in its last second.
pub(super) fn left(h: &Held, now: Duration) -> u64 {
    let d = h.due.saturating_sub(now);
    d.as_secs() + u64::from(d.subsec_nanos() > 0)
}

/// `recalled` put back into a compose box that holds `draft`: the recalled
/// text first, then a line, then the draft, so nothing typed is lost; the
/// recalled text alone into an empty box.
pub(super) fn with_draft(recalled: &str, draft: &str) -> String {
    if draft.is_empty() {
        recalled.to_string()
    } else {
        format!("{recalled}\n{draft}")
    }
}

/// The route `text` takes from a view whose send scope is `scope`, as a
/// key two messages share only when they are sent the same way: the kind of
/// send, then the names it goes to, case folded and in order. `post` is a
/// post in each channel named, the scope's for an unaddressed line; `in` is
/// `@Name` while a channel is shown, one post in that channel addressed to
/// them; `to` is any other address run, such as `@Tamsin` or `#deploy
/// @Tamsin`, a direct message to each agent beside each channel's post. So
/// `#deploy @Tamsin` from the operator view and `@Tamsin` in `#deploy` never
/// join. `None` for a slash command, a bare `/` among them: it has no route.
fn route(text: &str, scope: &str, in_channel: bool) -> Option<String> {
    if crate::slash::parse(text).is_some() || crate::slash::find_slash_partial(text).is_some() {
        return None;
    }
    let recipients = parse_addressed(text);
    let name = |r: &Addressed| match r {
        Addressed::Agent(n) => format!("@{}", n.to_lowercase()),
        Addressed::Group(n) => format!("#{}", n.to_lowercase()),
    };
    let mut names: Vec<String> = recipients.iter().map(name).collect();
    names.sort();
    names.dedup();
    let scope = format!("#{}", scope.to_lowercase());
    let key = if recipients.is_empty() {
        format!("post {scope}")
    } else if in_channel && recipients.iter().all(|r| matches!(r, Addressed::Agent(_))) {
        format!("in {scope} {}", names.join(" "))
    } else if recipients.iter().all(|r| matches!(r, Addressed::Group(_))) {
        format!("post {}", names.join(" "))
    } else {
        format!("to {}", names.join(" "))
    };
    Some(key)
}

impl ChatPane {
    /// Now on the chat's monotonic clock, which a countdown runs on: the
    /// time since the pane was made, on a live clock, so a step of the wall
    /// clock never moves one; the pinned instant since the epoch, on a
    /// pinned clock, which [`super::Chat::advance`] moves.
    pub(super) fn mono(&self) -> Duration {
        match self.clock {
            Clock::Live => self.started.elapsed(),
            Clock::Pinned { now, .. } => now.duration_since(UNIX_EPOCH).unwrap_or_default(),
        }
    }

    /// The route `text`, written in `fg`, takes as the send resolves it: the
    /// agent of a DM view for an unaddressed line there, else [`route`]
    /// from the view's send scope, so the operator view and `#open` share
    /// `post #open`. `None` for a slash command.
    pub(super) fn destination(&self, text: &str, fg: &Tab) -> Option<String> {
        if let Tab::Dm(member) = fg {
            if parse_addressed(text).is_empty() && crate::slash::parse(text).is_none() && crate::slash::find_slash_partial(text).is_none() {
                return Some(format!("dm {member}"));
            }
        }
        route(text, &tabs::send_scope(fg), matches!(fg, Tab::Channel(_)))
    }

    /// Enter on a message with a send delay set: hold it in the feed, or
    /// join it to the message already waiting for its route. False leaves it
    /// to be sent at once, as Enter always sent: a slash command, a bare
    /// `/`, an empty line, a view with nothing to send, no delay.
    pub(super) fn hold_draft(&mut self, fg: &Tab) -> bool {
        // No delay and nothing waiting: Enter's own path, untouched.
        if self.config.send_delay.is_zero() && self.pending.is_empty() {
            return false;
        }
        let text = self.input.text().trim_end().to_string();
        if text.is_empty() || !fg.composes() {
            return false;
        }
        let Some(dest) = self.destination(&text, fg) else { return false };
        let label = self.flag_label(&text, fg).unwrap_or_else(|| format!("#{}", tabs::send_scope(fg)));
        match self.pending.hold(&text, fg.clone(), dest, label, self.mono(), self.config.send_delay) {
            Hold::Now => false,
            Hold::Held(_) => {
                self.send_anyway = None;
                self.tab_cycle = None;
                self.input.clear();
                self.settle_box();
                self.show_newest();
                self.invalidate();
                // With no delay, a message that joined one still waiting
                // goes out with it now.
                self.send_due();
                true
            }
        }
    }

    /// Send now (Ctrl+Enter, Ctrl+X Ctrl+S): the draft at once, to a quiet
    /// agent too, after the message waiting for its route, joined as Enter
    /// would join it: Claude Code's `chat:sendNow` sends the queued messages
    /// and the draft. With nothing in the box every message waiting goes
    /// now. With nothing waiting it is the send-now Enter always had.
    pub(super) fn send_now(&mut self) {
        let fg = self.normal_tab();
        let text = self.input.text().trim_end().to_string();
        let dest = self.destination(&text, &fg);
        if self.pending.sendable() == 0 || !fg.composes() || (dest.is_none() && !text.is_empty()) {
            return self.send(true);
        }
        self.send_anyway = None;
        let Some(dest) = dest.filter(|_| !text.is_empty()) else {
            self.send_each(Held::sendable, true);
            return self.invalidate();
        };
        let label = self.flag_label(&text, &fg).unwrap_or_else(|| format!("#{}", tabs::send_scope(&fg)));
        match self.pending.hold(&text, fg, dest, label, self.mono(), Duration::ZERO) {
            Hold::Now => self.send(true),
            Hold::Held(id) => {
                self.input.clear();
                self.settle_box();
                if let Some(h) = self.pending.recall(id) {
                    self.send_held(h, true);
                }
                self.invalidate();
            }
        }
    }

    /// Send each held message whose countdown has ended, through the path
    /// Enter sends by, from the view it was written in.
    pub(super) fn send_due(&mut self) {
        let now = self.mono();
        if self.send_each(|h| h.refused.is_none() && h.due <= now, false) {
            self.invalidate();
        }
    }

    /// Send each entry `which` picks, oldest first, one at a time: the
    /// others stay in the feed, and noted, until their own send. False
    /// when it picked none; true once each has been sent or refused.
    fn send_each(&mut self, which: impl Fn(&Held) -> bool, anyway: bool) -> bool {
        let ids: Vec<u64> = self.pending.iter().filter(|h| which(h)).map(|h| h.id).collect();
        for &id in &ids {
            if let Some(h) = self.pending.recall(id) {
                self.send_held(h, anyway);
            }
        }
        !ids.is_empty()
    }

    /// Send `h` now, from the view it was written in, to the route it was
    /// held for; false when it did not go and stays in the feed, refused,
    /// with the reason on the status line. The route is resolved again
    /// first: a channel dissolved meanwhile, or a view that now resolves
    /// elsewhere, holds the message back rather than send it to `#open` or
    /// anywhere else it was not written for.
    fn send_held(&mut self, h: Held, anyway: bool) -> bool {
        // Noted as being sent until the send returns, so a panic or a
        // forced exit meanwhile keeps it marked as such, and noted again
        // once it has gone or is back in the feed.
        let _moving = unsent::moving();
        self.note_flight(Some(unsent::Noted { flight: true, ..Self::noted(&h) }), true);
        let sent = self.send_held_noted(h, anyway);
        self.note_unsent();
        sent
    }

    /// [`ChatPane::send_held`] with the text noted as being sent.
    fn send_held_noted(&mut self, h: Held, anyway: bool) -> bool {
        let names = self.strip_names();
        let fg = tabs::normalize(h.tab.clone(), &names);
        if self.destination(&h.text, &fg).as_deref() != Some(h.dest.as_str()) {
            let why = match &h.tab {
                Tab::Channel(c) if fg != h.tab => format!("not sent: #{c} is gone"),
                _ => format!("not sent: it no longer goes to {}", h.label),
            };
            return self.refuse(h, why, false);
        }
        if self.send_text(&h.text, fg, anyway, false) {
            return true;
        }
        // A quiet agent: the refusal at Enter offers Enter again; this one
        // offers it once the message is recalled into its view.
        let quiet = self.send_anyway.take().is_some();
        let why = match self.status.split_once(": ") {
            Some((_, what)) if quiet => format!("not sent: {what}; recall it and Enter sends it anyway"),
            _ => format!("not sent to {}: {}", h.label, self.status),
        };
        self.refuse(h, why, quiet)
    }

    /// Keep `h` in the feed, refused, and say `why`; false, for
    /// [`ChatPane::send_held`].
    fn refuse(&mut self, h: Held, why: String, quiet: bool) -> bool {
        self.pending.refuse(h, Refused { quiet });
        self.say(why, true);
        false
    }

    /// Put `text`, recalled, back in the compose box, before what the box
    /// holds, the keyboard in the box and the caret at the end of the text
    /// recalled.
    fn put_back(&mut self, text: &str) {
        let draft = self.input.text().to_string();
        self.input.set(with_draft(text, &draft), text.chars().count());
        self.tab_cycle = None;
        self.dispatch(Ev::Compose);
        self.dirty = true;
    }

    /// Esc in the compose box with a message waiting: the newest one back
    /// in the box, joined text and all, and not sent.
    pub(super) fn recall_newest(&mut self) {
        if let Some(h) = self.pending.recall_newest() {
            self.recalled(h);
        }
    }

    /// A click on the waiting message `i` of the feed, or Enter on it
    /// selected: that one back in the box, and not sent.
    pub(super) fn recall_entry(&mut self, i: usize) -> bool {
        let Some(id) = i.checked_sub(self.pending_from).and_then(|n| self.pending.iter().nth(n)).map(|h| h.id) else { return false };
        if let Some(h) = self.pending.recall(id) {
            self.feed.selected = None;
            self.recalled(h);
        }
        true
    }

    /// `h` recalled: shown in the view it was written in, while that view
    /// is there, so Enter sends it where it was held for; then back in the
    /// box. A draft in the box of the view left behind is parked as that
    /// view's draft ([`ChatPane::park_draft`]), never carried along. A
    /// quiet refusal recalled unchanged is one Enter sends anyway. A kept
    /// text recalled leaves its file claimed until the box's text goes.
    fn recalled(&mut self, h: Held) {
        let names = self.strip_names();
        let view_known = !matches!(h.source, Source::Kept { view_known: false, .. });
        if view_known && tabs::normalize(h.tab.clone(), &names) == h.tab && self.normal_tab() != h.tab {
            self.park_draft();
            self.set_tab(h.tab.clone());
        }
        self.send_anyway = None;
        match &h.source {
            Source::Enter => {}
            Source::Parked { origins } => self.box_origins.extend(origins.iter().cloned()),
            Source::Kept { path, .. } => self.box_origins.push(path.clone()),
        }
        self.put_back(&h.text);
        if h.refused.is_some_and(|r| r.quiet) && self.input.text() == h.text {
            self.send_anyway = Some((h.text.clone(), h.tab.clone()));
        }
        let said = if matches!(h.source, Source::Kept { maybe_sent: true, .. }) {
            format!("recalled: it was being sent to {} as a chat ended and may have gone; check before Enter", h.label)
        } else if matches!(h.source, Source::Kept { partial: true, .. }) {
            format!("recalled: cut short by a crash, the draft written for {}", h.label)
        } else if !view_known {
            "recalled: kept by an earlier build, which did not note its view; the flag shows where Enter sends it".to_string()
        } else if h.is_draft() {
            format!("recalled: the draft written for {}", h.label)
        } else {
            format!("recalled: not sent to {}", h.label)
        };
        self.say(said, false);
        self.invalidate();
    }

    /// The draft in the box, as a draft entry of the view it was written
    /// in, with the kept files it carries: the box is then empty.
    fn park_draft(&mut self) {
        let Some(text) = self.draft() else { return };
        let fg = self.foreground.clone();
        let (dest, label) = self.route_of(&text, &fg);
        let origins = std::mem::take(&mut self.box_origins);
        self.pending.add(Held { id: 0, text, tab: fg, dest, label, due: Duration::ZERO, refused: None, touched: 0, source: Source::Parked { origins } }, false);
        self.input.clear();
    }

    /// The view shown with the box empty: a draft entry written in it comes
    /// back into the box.
    pub(super) fn draft_back(&mut self) {
        if !self.input.is_empty() || !self.foreground.composes() {
            return;
        }
        let fg = self.foreground.clone();
        let Some(id) = self.pending.iter().find(|h| h.is_draft() && h.tab == fg && !matches!(h.source, Source::Kept { view_known: false, .. })).map(|h| h.id) else { return };
        if let Some(h) = self.pending.recall(id) {
            match h.source {
                Source::Parked { origins } => self.box_origins.extend(origins),
                Source::Kept { path, .. } => self.box_origins.push(path),
                Source::Enter => {}
            }
            self.input.set(h.text.clone(), h.text.chars().count());
            self.say(format!("back: the draft written for {}", h.label), false);
            self.invalidate();
        }
    }

    /// The route and the flag's label `text` has, written in `fg`; a slash
    /// command has no route.
    fn route_of(&mut self, text: &str, fg: &Tab) -> (String, String) {
        let dest = self.destination(text, fg).unwrap_or_default();
        let label = self.flag_label(text, fg).unwrap_or_else(|| format!("#{}", tabs::send_scope(fg)));
        (dest, label)
    }

    /// The box's text was sent, held, or kept: the kept files it carried
    /// are removed. A dry run removes nothing.
    pub(super) fn settle_box(&mut self) {
        let origins = std::mem::take(&mut self.box_origins);
        if !self.dry_run {
            unsent::settle(origins);
        }
    }

    /// The status line's countdown, for the messages counting down whose
    /// headers the last frame did not draw: a short pane, a feed scrolled
    /// back, or a view whose feed shows nothing waiting. The soonest of
    /// those and where it goes, with how many there are when there are
    /// several; `more` before the count when the feed shows another
    /// countdown's header, so the status line never repeats one the feed
    /// shows. Then `Esc recall` while the box has the keyboard, which the
    /// footer then leaves out, since at 80 columns the countdown leaves the
    /// footer no room for it. `None` while every countdown's header is on
    /// screen, and while a fresh result holds the status line. The tick
    /// that moves the feed's countdown redraws this one.
    pub(super) fn countdown_status(&self) -> Option<String> {
        if self.status_fresh() {
            return None;
        }
        let shown = self.foreground.composes();
        let counting: Vec<(usize, &Held)> = self.pending.iter().enumerate().filter(|(_, h)| h.refused.is_none()).collect();
        let unseen: Vec<&Held> = counting.iter().filter(|(n, _)| !(shown && self.feed.header_row(self.pending_from + n).is_some())).map(|(_, h)| *h).collect();
        let next = unseen.iter().min_by_key(|h| h.due)?;
        let left = left(next, self.mono());
        let mut said = match (unseen.len(), unseen.len() < counting.len()) {
            (1, false) => format!("sending in {left}s → {}", next.label),
            (n, false) => format!("{n} waiting, next in {left}s → {}", next.label),
            (n, true) => format!("{n} more waiting, next in {left}s → {}", next.label),
        };
        if self.focus_kind() == super::focus::Kind::Compose {
            said.push_str(" · Esc recall");
        }
        Some(said)
    }

    /// The feed's entries for the messages waiting, after the messages: each
    /// marked on the warning ground with its countdown, on the error ground
    /// as not sent, or as a draft, where it goes, and the key that recalls
    /// it. Shown in every view that sends, since Esc recalls the newest
    /// wherever the keyboard is in the box.
    pub(super) fn pending_entries(&self, fg: &Tab) -> Vec<Entry> {
        if !fg.composes() {
            return Vec::new();
        }
        let now = self.mono();
        let newest = self.pending.iter().max_by_key(|h| h.touched).map(|h| h.id);
        self.pending
            .iter()
            .map(|h| {
                // A band as a message's header is: each step on a ground of
                // its own.
                let how = if newest == Some(h.id) { " Esc or click recalls " } else { " click recalls " };
                let state = match h.refused {
                    _ if matches!(h.source, Source::Kept { maybe_sent: true, .. }) => Seg::on(" may have been sent ", Ground::Err).bold(),
                    _ if matches!(h.source, Source::Kept { partial: true, .. }) => Seg::on(" draft, cut short ", Ground::Accent).bold(),
                    _ if h.is_draft() => Seg::on(" draft ", Ground::Accent).bold(),
                    None => Seg::on(format!(" sending in {}s ", left(h, now)), Ground::Warn).bold(),
                    Some(_) => Seg::on(" not sent ", Ground::Err).bold(),
                };
                let to = match h.source {
                    Source::Kept { view_known: false, .. } => " → where, not noted ".to_string(),
                    _ => format!(" → {} ", h.label),
                };
                let header = vec![state, Seg::on(to, Ground::AccentDim), Seg::on(how, Ground::Rule)];
                let body = h.text.split('\n').map(|l| Line::styled(l.to_string(), theme::muted())).collect();
                Entry { header, body }
            })
            .collect()
    }

    /// What quitting now would lose of what waits, as the exit guard names
    /// it: the messages Enter held, and the drafts parked. A restored entry
    /// is still in its kept file and is not named.
    pub(super) fn pending_unsaved(&self) -> Option<String> {
        let waiting = match self.pending.sendable() {
            0 => None,
            1 => Some("a message waiting to send".to_string()),
            n => Some(format!("{n} messages waiting to send")),
        };
        let parked: Vec<&Held> = self.pending.iter().filter(|h| matches!(h.source, Source::Parked { .. })).collect();
        let parked = match parked.as_slice() {
            [] => None,
            [h] => Some(format!("a draft written for {}", h.label)),
            more => Some(format!("{} drafts written in other views", more.len())),
        };
        let parts: Vec<String> = [waiting, parked].into_iter().flatten().collect();
        (!parts.is_empty()).then(|| parts.join(" and "))
    }

    /// The exit guard's button that keeps what the chat can keep and quits:
    /// with messages waiting, Send and quit, which sends them; with none,
    /// Quit, keep the drafts, so drafts alone, or drafts a Send and quit
    /// could not keep after its message went, can always be kept rather
    /// than dropped. Its label says when a draft, in the box or parked, is
    /// kept for the next chat.
    pub(super) fn pending_quit_keeping(&self) -> Option<(String, char)> {
        let drafts = usize::from(!self.input.is_empty()) + self.pending.iter().filter(|h| matches!(h.source, Source::Parked { .. })).count();
        let draft = match drafts {
            0 => "",
            1 => ", keep the draft",
            _ => ", keep the drafts",
        };
        match self.pending.sendable() {
            0 if drafts == 0 => None,
            0 => Some((format!("Quit{draft}"), 'k')),
            1 => Some((format!("Send and quit{draft}"), 's')),
            n => Some((format!("Send {n} and quit{draft}"), 's')),
        }
    }

    /// Quitting, the operator chose to send what waits: each message Enter
    /// held, now, once, to a quiet agent too, as send-now sends. False when
    /// one did not go: it stays in the feed, refused, the reason on the
    /// status line, and the chat stays open rather than lose it. A draft
    /// and a restored entry are not sent: Enter never sent them.
    pub(super) fn send_all_now(&mut self) -> bool {
        self.send_each(Held::sendable, true);
        self.invalidate();
        !self.pending.iter().any(Held::sendable)
    }

    /// The chat ends unasked: a signal, or the terminal hanging up
    /// ([`agent_tui::Pane::ending`]). Enter meant send, and closing the
    /// terminal is no recall, so each message waiting is sent now, once, as
    /// Send and quit sends it, with no retry; one that does not go then is
    /// kept for the next chat ([`ChatPane::keep_what_did_not_go`]). This is
    /// the one place the choice is made: to keep what waits unsent instead,
    /// leave it waiting here.
    pub(super) fn send_waiting_at_unasked_exit(&mut self) {
        self.send_all_now();
    }

    /// The chat ends unasked: what waits that did not go, the drafts parked
    /// and the draft in the box are kept for the next chat, so nothing
    /// typed is lost unsaid. A restored entry stays in its kept file. The
    /// operator said: "I don't want to drop messages".
    pub(super) fn keep_what_did_not_go(&mut self) {
        self.keep(|h| !matches!(h.source, Source::Kept { .. }));
    }

    /// Send and quit keeps the draft in the box and the drafts parked for
    /// the next chat; false, with the reason on the status line, when they
    /// could not be kept.
    pub(super) fn keep_draft(&mut self) -> bool {
        self.keep(|h| matches!(h.source, Source::Parked { .. }))
    }

    /// The draft in the box as a record of the view shown, a channel
    /// dissolved meanwhile included: that is the view it was written in.
    fn box_record(&mut self) -> Option<unsent::Record> {
        let text = self.draft()?;
        let fg = self.foreground.clone();
        let (dest, label) = self.route_of(&text, &fg);
        Some(unsent::Record { kind: unsent::Kind::Draft, tab: Some(fg), dest, label, text })
    }

    /// `h` as a record.
    fn record(h: &Held) -> unsent::Record {
        let kind = if h.is_draft() { unsent::Kind::Draft } else { unsent::Kind::Held };
        unsent::Record { kind, tab: Some(h.tab.clone()), dest: h.dest.clone(), label: h.label.clone(), text: h.text.clone() }
    }

    /// The kept files the entries `held` and the box carry.
    fn origins(&self, held: &[Held]) -> Vec<std::path::PathBuf> {
        let mut o = self.box_origins.clone();
        for h in held {
            o.extend(Self::parked_origins(h));
        }
        o
    }

    /// The kept files a parked draft carries.
    fn parked_origins(h: &Held) -> Vec<std::path::PathBuf> {
        match &h.source {
            Source::Parked { origins } => origins.clone(),
            _ => Vec::new(),
        }
    }

    /// Keep the entries `which` picks, and the draft in the box, a file
    /// each. An entry leaves the feed only once its file is written, and
    /// the kept files a written text carried are removed then; what is
    /// noted follows each file ([`unsent::note`]). True when all were
    /// kept, or there was nothing to keep. False, with the reason on the
    /// status line, when one could not be: it stays in the feed, or in the
    /// box, with the files it carried. A box draft that was written while
    /// another text failed carries its new file, so keeping again replaces
    /// it. A dry run writes nothing and takes the entries.
    fn keep(&mut self, which: impl Fn(&Held) -> bool) -> bool {
        if self.dry_run {
            self.pending.take_where(which);
            return true;
        }
        let _moving = unsent::moving();
        let ids: Vec<u64> = self.pending.iter().filter(|h| which(h)).map(|h| h.id).collect();
        let mut failed = None;
        for id in ids {
            let Some(h) = self.pending.iter().find(|h| h.id == id).cloned() else { continue };
            match unsent::write(&self.unsent_dir, &self.writer, &[(Self::record(&h), Self::parked_origins(&h))]).remove(0) {
                Ok(_) => {
                    self.pending.take_where(|o| o.id == id);
                    unsent::settle(Self::parked_origins(&h));
                    self.note_unsent();
                }
                Err(e) => {
                    failed.get_or_insert(e);
                }
            }
        }
        if let Some(r) = self.box_record() {
            match unsent::write(&self.unsent_dir, &self.writer, &[(r, self.box_origins.clone())]).remove(0) {
                Ok(path) => {
                    unsent::settle(std::mem::replace(&mut self.box_origins, vec![path]));
                    self.note_unsent();
                }
                Err(e) => {
                    failed.get_or_insert(e);
                }
            }
        }
        // What is noted is what stays after this keep, whether or not all
        // of it was kept, the box's draft with the file it is now in: a
        // panic now writes no text a second time.
        self.note_unsent();
        if let Some((path, e)) = failed {
            // The reason first: a narrow status line keeps it, and the
            // path after it.
            self.say(format!("not kept ({e}): {}", path.display()), true);
            self.invalidate();
            return false;
        }
        // All kept: the box's draft is in its new file, which the shell's
        // discard at the end of the chat must leave in place. The caller
        // ends the chat and clears what is noted ([`unsent::clear`]).
        self.box_origins.clear();
        true
    }

    /// What the chat would keep if it ended now, noted for a panic and a
    /// forced exit ([`unsent::keep_noted`]): the messages Enter held, the
    /// drafts parked and the draft in the box, each with the kept files it
    /// carries. A restored entry is still in its kept file.
    pub(super) fn note_unsent(&mut self) {
        self.note_flight(None, true);
    }

    /// [`ChatPane::note_unsent`] with `flight`, a text being sent, noted as
    /// such; `with_box` false leaves out the draft in the box, which is the
    /// text being sent.
    pub(super) fn note_flight(&mut self, flight: Option<unsent::Noted>, with_box: bool) {
        if self.dry_run {
            return;
        }
        let mut texts: Vec<unsent::Noted> = self.pending.iter().filter(|h| !matches!(h.source, Source::Kept { .. })).map(Self::noted).collect();
        if with_box {
            texts.extend(self.box_noted());
        }
        texts.extend(flight);
        unsent::note(&self.unsent_dir, &self.writer, texts);
    }

    /// `h` as a text noted.
    fn noted(h: &Held) -> unsent::Noted {
        let origins = match &h.source {
            Source::Parked { origins } => origins.clone(),
            Source::Kept { path, .. } => vec![path.clone()],
            Source::Enter => Vec::new(),
        };
        unsent::Noted { record: Self::record(h), origins, flight: false }
    }

    /// The draft in the box as a text noted, with the kept files it
    /// carries.
    pub(super) fn box_noted(&mut self) -> Option<unsent::Noted> {
        let record = self.box_record()?;
        Some(unsent::Noted { record, origins: self.box_origins.clone(), flight: false })
    }

    fn draft(&self) -> Option<String> {
        Some(self.input.text().trim_end().to_string()).filter(|d| !d.is_empty())
    }

    /// Restore what the earlier chats kept unsent ([`unsent::claim`]). The
    /// newest draft whose view is still there opens in the box of that
    /// view, shown. Every other text is an entry in the feed: a message
    /// not sent, or a draft, recalled into its own view by Esc or a click.
    /// A file that could not be read stays where it is, named on the
    /// status line. The files claimed stay until their text has gone.
    pub(super) fn restore_unsent(&mut self) {
        let found = unsent::claim(&self.unsent_dir, &self.writer);
        let names = self.strip_names();
        let live = |t: &Option<Tab>| t.as_ref().is_some_and(|t| t.composes() && tabs::normalize(t.clone(), &names) == *t);
        let boxed = if self.input.is_empty() { found.claimed.iter().rposition(|c| c.record.kind == unsent::Kind::Draft && !c.partial && live(&c.record.tab)) } else { None };
        let mut said = Vec::new();
        if let Some(c) = boxed.map(|i| &found.claimed[i]) {
            let tab = c.record.tab.clone().unwrap_or_default();
            if self.foreground != tab {
                self.set_tab(tab);
            }
            self.input.set(c.record.text.clone(), c.record.text.chars().count());
            self.box_origins.push(c.path.clone());
            said.push(format!("restored: the draft for {} is in the box", c.record.label));
        }
        let (mut entries, mut gone, mut unsure, mut partial) = (0, Vec::new(), 0, 0);
        for (i, c) in found.claimed.into_iter().enumerate() {
            if Some(i) == boxed {
                continue;
            }
            let r = c.record;
            let draft = r.kind == unsent::Kind::Draft;
            if draft && r.tab.is_some() && !live(&r.tab) {
                gone.push(r.label.clone());
            }
            if r.kind == unsent::Kind::Sending {
                unsure += 1;
            }
            partial += usize::from(c.partial);
            let source = Source::Kept { path: c.path, draft, view_known: r.tab.is_some(), partial: c.partial, maybe_sent: r.kind == unsent::Kind::Sending };
            self.pending.add(Held { id: 0, text: r.text, tab: r.tab.unwrap_or_default(), dest: r.dest, label: r.label, due: Duration::ZERO, refused: None, touched: 0, source }, true);
            entries += 1;
        }
        let lead = if said.is_empty() { "restored: " } else { "" };
        match entries {
            0 => {}
            1 => said.push(format!("{lead}one kept text is in the feed; Esc or a click recalls it")),
            n => said.push(format!("{lead}{n} kept texts are in the feed; a click recalls each")),
        }
        match unsure {
            0 => {}
            1 => said.push("one was being sent as a chat ended and may have gone: check before Enter".into()),
            n => said.push(format!("{n} were being sent as a chat ended and may have gone: check before Enter")),
        }
        match partial {
            0 => {}
            1 => said.push("one was cut short by a crash".into()),
            n => said.push(format!("{n} were cut short by a crash")),
        }
        if !gone.is_empty() {
            said.push(format!("the view of the draft for {} is gone", gone.join(", ")));
        }
        let error = !found.unreadable.is_empty() || !found.unjudged.is_empty();
        // The reason first, so a narrow status line keeps it; the names and
        // the directory after.
        if !found.unreadable.is_empty() {
            said.push(format!("not restored (unreadable, left in place): {} in {}", found.unreadable.join(", "), self.unsent_dir.display()));
        }
        for (name, why) in &found.unjudged {
            said.push(format!("not restored (its claim could not be checked: {why}): {name} in {}", self.unsent_dir.display()));
        }
        if !said.is_empty() {
            self.say(said.join("; "), error);
            self.dirty = true;
            self.invalidate();
        }
    }

    /// Quitting, the operator chose to discard what waits: the messages
    /// held, the drafts parked and the draft in the box, with the kept
    /// files they carried. A restored entry the operator never recalled
    /// stays in its kept file: the guard did not name it.
    pub(super) fn discard_pending(&mut self) {
        let _moving = unsent::moving();
        let taken = self.pending.take_all();
        let origins = self.origins(&taken);
        self.box_origins.clear();
        if !self.dry_run {
            unsent::settle(origins);
        }
        self.note_unsent();
        self.invalidate();
    }

    /// Each tick: send what is due, and draw the feed again when a
    /// countdown it shows has moved. While the exit guard asks whether to
    /// send or discard what waits, nothing is sent from under it; Back lets
    /// a countdown that ended meanwhile send at the next tick.
    pub(super) fn tick_pending(&mut self) {
        if !self.ix.guarded() {
            self.send_due();
        }
        let shown = self.pending.countdown(self.mono());
        if shown != self.pending_shown {
            self.pending_shown = shown;
            self.invalidate();
        }
        self.note_unsent();
    }

    /// Whether a message waits, for Esc in the box.
    pub(super) fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DELAY: Duration = Duration::from_secs(5);

    fn at(secs: u64) -> Duration {
        Duration::from_secs(1_790_000_000 + secs)
    }

    fn hold(p: &mut Pending, text: &str, dest: &str, now: Duration) -> Hold {
        p.hold(text, Tab::Merged, dest.into(), dest.into(), now, DELAY)
    }

    fn texts(p: &Pending) -> Vec<&str> {
        p.iter().map(|h| h.text.as_str()).collect()
    }

    #[test]
    fn messages_to_one_destination_join_and_others_wait_apart() {
        let mut p = Pending::default();
        assert_eq!(hold(&mut p, "first", "#deploy", at(0)), Hold::Held(1));
        assert_eq!(hold(&mut p, "to tamsin", "@tamsin", at(1)), Hold::Held(2), "another destination waits on its own");
        assert_eq!(hold(&mut p, "second", "#deploy", at(2)), Hold::Held(1), "the same destination joins the first");
        assert_eq!(texts(&p), ["first\nsecond", "to tamsin"]);
        assert_eq!(p.iter().count(), 2);
    }

    #[test]
    fn a_joined_message_starts_the_countdown_again() {
        let mut p = Pending::default();
        hold(&mut p, "first", "#deploy", at(0));
        assert_eq!(p.countdown(at(0)), [Some(5)]);
        assert_eq!(p.countdown(at(3)), [Some(2)]);
        hold(&mut p, "second", "#deploy", at(4));
        assert_eq!(p.countdown(at(4)), [Some(5)], "five seconds from the second Enter");
        assert!(p.due(at(5)).is_empty(), "not due when the first countdown would have ended");
        assert_eq!(p.due(at(9)).into_iter().map(|h| h.text).collect::<Vec<_>>(), ["first\nsecond"]);
        assert!(p.is_empty());
    }

    #[test]
    fn the_countdown_rounds_up_to_whole_seconds() {
        let mut p = Pending::default();
        hold(&mut p, "x", "#open", at(0));
        assert_eq!(p.countdown(at(0) + Duration::from_millis(100)), [Some(5)]);
        assert_eq!(p.countdown(at(4) + Duration::from_millis(900)), [Some(1)]);
        assert_eq!(p.countdown(at(6)), [Some(0)], "past due reads as nothing left");
    }

    #[test]
    fn each_message_is_sent_when_its_own_countdown_ends() {
        let mut p = Pending::default();
        hold(&mut p, "a", "#deploy", at(0));
        hold(&mut p, "b", "@tamsin", at(2));
        assert!(p.due(at(4)).is_empty());
        let first = p.due(at(5));
        assert_eq!(first.iter().map(|h| h.text.as_str()).collect::<Vec<_>>(), ["a"]);
        assert_eq!(texts(&p), ["b"]);
        assert_eq!(p.due(at(7)).len(), 1);
        assert!(p.is_empty());
    }

    #[test]
    fn esc_recalls_the_newest_message_whole() {
        let mut p = Pending::default();
        hold(&mut p, "one", "#deploy", at(0));
        hold(&mut p, "two", "@tamsin", at(1));
        hold(&mut p, "three", "#deploy", at(2));
        let r = p.recall_newest().expect("held");
        assert_eq!(r.text, "one\nthree", "the message written to last, joined text and all");
        assert_eq!(p.recall_newest().map(|h| h.text), Some("two".into()));
        assert_eq!(p.recall_newest(), None);
        assert!(p.due(at(60)).is_empty(), "a recalled message is never sent");
    }

    #[test]
    fn a_click_recalls_the_message_clicked() {
        let mut p = Pending::default();
        hold(&mut p, "one", "#deploy", at(0));
        hold(&mut p, "two", "@tamsin", at(1));
        assert_eq!(p.recall(1).map(|h| h.text), Some("one".into()));
        assert_eq!(p.recall(1), None, "recalled once");
        assert_eq!(texts(&p), ["two"]);
    }

    #[test]
    fn a_recalled_message_goes_before_the_draft() {
        assert_eq!(with_draft("held", ""), "held");
        assert_eq!(with_draft("held\njoined", "typed since"), "held\njoined\ntyped since");
    }

    #[test]
    fn with_no_delay_a_message_is_sent_at_once() {
        let mut p = Pending::default();
        assert_eq!(p.hold("now", Tab::Merged, "#open".into(), "#open".into(), at(0), Duration::ZERO), Hold::Now);
        assert!(p.is_empty(), "nothing is held");
        // A message held before the delay was set to 0 takes the next one
        // and is due at once, so the two go out together.
        hold(&mut p, "waiting", "#open", at(0));
        assert_eq!(p.hold("now", Tab::Merged, "#open".into(), "#open".into(), at(1), Duration::ZERO), Hold::Held(1));
        assert_eq!(p.due(at(1)).into_iter().map(|h| h.text).collect::<Vec<_>>(), ["waiting\nnow"]);
    }

    #[test]
    fn quitting_takes_every_message() {
        let mut p = Pending::default();
        hold(&mut p, "a", "#deploy", at(0));
        hold(&mut p, "b", "@tamsin", at(1));
        assert_eq!(p.take_all().len(), 2);
        assert!(p.is_empty());
    }

    #[test]
    fn a_refused_message_waits_apart_takes_nothing_and_is_never_due() {
        let mut p = Pending::default();
        hold(&mut p, "to lark", "to @lark", at(0));
        hold(&mut p, "to deploy", "post #deploy", at(1));
        let due = p.due(at(5));
        let h = due.into_iter().next().expect("due");
        p.refuse(h, Refused { quiet: true });
        assert_eq!(texts(&p), ["to lark", "to deploy"], "back in its place among the others");
        assert_eq!(p.countdown(at(5)), [None, Some(1)], "no countdown on a refused one");
        assert_eq!(hold(&mut p, "again", "to @lark", at(6)), Hold::Held(3), "a refused message takes nothing: a new one waits apart");
        assert!(p.due(at(60)).iter().all(|h| h.refused.is_none()), "nothing sends a refused message by itself");
        assert_eq!(texts(&p), ["to lark"]);
        // Esc recalls the refused one: written to last.
        hold(&mut p, "older", "post #infra", at(70));
        let first = p.iter().next().map(|h| h.id).unwrap();
        let h = p.recall(first).unwrap();
        p.refuse(h, Refused { quiet: false });
        assert_eq!(p.recall_newest().map(|h| h.text), Some("to lark".into()));
    }

    #[test]
    fn a_route_names_the_kind_of_send_and_who_it_goes_to() {
        let r = |t: &str, scope: &str, in_channel: bool| route(t, scope, in_channel);
        // The operator view and #open share #open.
        assert_eq!(r("hello", "open", false), r("also", "open", true));
        assert_eq!(r("hello", "open", false).as_deref(), Some("post #open"));
        assert_eq!(r("#open hi", "open", false).as_deref(), Some("post #open"), "an explicit #open is #open's post");
        // `@Name` in a channel is a post there, apart from the channel's
        // own and from a DM to them.
        assert_eq!(r("@Tamsin y", "deploy", true).as_deref(), Some("in #deploy @tamsin"));
        assert_ne!(r("@Tamsin y", "deploy", true), r("plain", "deploy", true));
        assert_ne!(r("@Tamsin y", "deploy", true), r("@Tamsin y", "open", false));
        // `#deploy @Tamsin` is a post and a DM, never `@Tamsin` in #deploy.
        assert_eq!(r("#deploy @Tamsin x", "open", false).as_deref(), Some("to #deploy @tamsin"));
        assert_eq!(r("#deploy @Tamsin x", "deploy", true).as_deref(), Some("to #deploy @tamsin"));
        assert_ne!(r("#deploy @Tamsin x", "open", false), r("@Tamsin y", "deploy", true));
        // Order and case do not split a route.
        assert_eq!(r("@Tamsin @Cleo a", "open", false), r("@cleo @tamsin b", "open", false));
        // A slash command, a bare `/` among them, has none.
        assert_eq!(r("/", "open", false), None);
        assert_eq!(r("/channels", "deploy", true), None);
    }
}
