//! Messages held after Enter before they are sent (`attend.chat.send_delay`).
//!
//! Enter on a message holds it here, shown in the feed with a countdown, and
//! it is sent when the countdown ends, through the path Enter sends by. While
//! it waits, Esc in the compose box recalls the newest one into the box and a
//! click recalls the one clicked; nothing held is sent then. Enter on another
//! message to the same destination, while one waits, joins it on a new line
//! and starts its countdown again, so a burst goes out as one message. A
//! different destination waits on its own.
//!
//! The queue is pure state over the chat's clock: every method takes the time
//! it acts at, so the tests step the clock rather than sleep, and a pinned
//! clock holds a frame's countdown still.

use std::time::{Duration, SystemTime};

use agent_tui::feed::Entry;
use agent_tui::ratatui::text::Line;
use agent_tui::theme::{self, Ground, Seg};

use super::ix::Ev;
use super::ChatPane;
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
    /// Where it goes, as the send resolves it ([`super::ChatPane::destination`]):
    /// what joins two messages.
    pub(super) dest: String,
    /// Where it goes, as the destination flag named it.
    pub(super) label: String,
    /// When it is sent.
    pub(super) due: SystemTime,
    /// When it was last written to: the newest is recalled first.
    touched: u64,
}

/// What [`Pending::hold`] did with a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Hold {
    /// No delay and nothing waiting for its destination: send it now, as
    /// Enter always did.
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

    pub(super) fn len(&self) -> usize {
        self.held.len()
    }

    /// The messages held, oldest first.
    pub(super) fn iter(&self) -> impl Iterator<Item = &Held> {
        self.held.iter()
    }

    /// Hold `text`, written in `tab`, for `dest`, at `now`, for `delay`. A
    /// message already waiting for `dest` takes it on a new line and waits
    /// `delay` again from `now`. With no delay, a message nothing waits for
    /// is sent at once ([`Hold::Now`]); one that joins a waiting message is
    /// due at once, so the two go out together.
    pub(super) fn hold(&mut self, text: &str, tab: Tab, dest: String, label: String, now: SystemTime, delay: Duration) -> Hold {
        self.writes += 1;
        let due = now + delay;
        if let Some(h) = self.held.iter_mut().find(|h| h.dest == dest) {
            h.text = format!("{}\n{text}", h.text);
            h.due = due;
            h.touched = self.writes;
            return Hold::Held(h.id);
        }
        if delay.is_zero() {
            return Hold::Now;
        }
        self.next_id += 1;
        self.held.push(Held { id: self.next_id, text: text.to_string(), tab, dest, label, due, touched: self.writes });
        Hold::Held(self.next_id)
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

    /// Take every message due by `now`, in the order they were held.
    pub(super) fn due(&mut self, now: SystemTime) -> Vec<Held> {
        let (due, kept) = std::mem::take(&mut self.held).into_iter().partition(|h| h.due <= now);
        self.held = kept;
        due
    }

    /// Take every message, due or not: sent now or discarded as the operator
    /// quits.
    pub(super) fn take_all(&mut self) -> Vec<Held> {
        std::mem::take(&mut self.held)
    }

    /// The whole seconds left on each message's countdown at `now`, rounded
    /// up, oldest first: what the feed shows, so it is drawn again when one
    /// of them changes.
    pub(super) fn countdown(&self, now: SystemTime) -> Vec<u64> {
        self.held.iter().map(|h| left(h, now)).collect()
    }
}

/// The whole seconds left before `h` is sent at `now`, rounded up: `5s` the
/// moment it is held for five seconds, `1s` in its last second.
pub(super) fn left(h: &Held, now: SystemTime) -> u64 {
    let d = h.due.duration_since(now).unwrap_or_default();
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

/// `dest` as a key two messages share when they go to the same place: the
/// destination flag's names, without its marks, case folded and in order,
/// so `@Tamsin @Cleo` and `@cleo @tamsin` join and `#deploy` and
/// `#deploy @Tamsin` do not.
fn dest_key(label: &str) -> String {
    let mut names: Vec<String> = label.split_whitespace().filter(|w| w.starts_with(['@', '#'])).map(str::to_lowercase).collect();
    names.sort();
    names.join(" ")
}

impl ChatPane {
    /// Where `text`, written in `fg`, goes, as the send resolves it: the
    /// agent of a DM view for an unaddressed line there, else the
    /// destination flag's names as the operator view or a channel resolve
    /// them ([`super::keys::destination_label`]), so the operator view and
    /// `#open` share `#open`, and `@Name` in a channel is that channel's.
    pub(super) fn destination(&self, text: &str, fg: &Tab) -> String {
        if let Tab::Dm(member) = fg {
            if crate::legend::parse_addressed(text).is_empty() {
                return format!("dm {member}");
            }
        }
        let in_channel = matches!(fg, Tab::Channel(_));
        let label = super::destination_label(text, &tabs::send_scope(fg), in_channel, &[], &[]).unwrap_or_default();
        dest_key(&label)
    }

    /// Enter on a message with a send delay set: hold it in the feed, or
    /// join it to the message already waiting for its destination. False
    /// leaves it to be sent at once, as Enter always sent: a slash command,
    /// an empty line, a view with nothing to send, no delay.
    pub(super) fn hold_draft(&mut self, fg: &Tab) -> bool {
        // No delay and nothing waiting: Enter's own path, untouched.
        if self.config.send_delay.is_zero() && self.pending.is_empty() {
            return false;
        }
        let text = self.input.text().trim_end().to_string();
        if text.is_empty() || crate::slash::parse(&text).is_some() || !fg.composes() {
            return false;
        }
        let dest = self.destination(&text, fg);
        let label = self.flag_label(&text, fg).unwrap_or_else(|| format!("#{}", tabs::send_scope(fg)));
        match self.pending.hold(&text, fg.clone(), dest, label, self.now(), self.config.send_delay) {
            Hold::Now => false,
            Hold::Held(_) => {
                self.send_anyway = None;
                self.tab_cycle = None;
                self.input.clear();
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
    /// agent too, after the message waiting for its destination, joined as
    /// Enter would join it: Claude Code's `chat:sendNow` sends the queued
    /// messages and the draft. With nothing in the box every message waiting
    /// goes now. With nothing waiting it is the send-now Enter always had.
    pub(super) fn send_now(&mut self) {
        let fg = self.normal_tab();
        let text = self.input.text().trim_end().to_string();
        if self.pending.is_empty() || crate::slash::parse(&text).is_some() || !fg.composes() {
            return self.send(true);
        }
        self.send_anyway = None;
        if text.is_empty() {
            for h in self.pending.take_all() {
                self.send_held(h, true);
            }
            return self.invalidate();
        }
        let dest = self.destination(&text, &fg);
        let label = self.flag_label(&text, &fg).unwrap_or_else(|| format!("#{}", tabs::send_scope(&fg)));
        match self.pending.hold(&text, fg, dest, label, self.now(), Duration::ZERO) {
            Hold::Now => self.send(true),
            Hold::Held(id) => {
                self.input.clear();
                if let Some(h) = self.pending.recall(id) {
                    self.send_held(h, true);
                }
                self.invalidate();
            }
        }
    }

    /// Send each held message whose countdown has ended, through the path
    /// Enter sends by, from the view it was written in. A refused one comes
    /// back to the compose box with the reason on the status line.
    pub(super) fn send_due(&mut self) {
        let due = self.pending.due(self.now());
        if due.is_empty() {
            return;
        }
        for h in due {
            self.send_held(h, false);
        }
        self.invalidate();
    }

    /// Send `h` now, from the view it was written in as the strip has it
    /// now; false when it was refused and is back in the box.
    fn send_held(&mut self, h: Held, anyway: bool) -> bool {
        let names = self.strip_names();
        let fg = tabs::normalize(h.tab.clone(), &names);
        let sent = self.send_text(&h.text, fg, anyway, false);
        if !sent {
            self.put_back(&h.text, true);
        }
        sent
    }

    /// Put `text` back in the compose box, before what the box holds. A
    /// recall puts the keyboard in the box and the caret at the end of the
    /// text recalled; a send refused as its countdown ended (`refused`)
    /// leaves the keyboard where it is and the caret where it was in the
    /// draft being typed.
    fn put_back(&mut self, text: &str, refused: bool) {
        let draft = self.input.text().to_string();
        let joined = with_draft(text, &draft);
        let recalled = text.chars().count();
        let caret = if refused && !draft.is_empty() { recalled + 1 + self.input.cursor() } else { recalled };
        self.input.set(joined, caret);
        self.tab_cycle = None;
        if !refused {
            self.dispatch(Ev::Compose);
        }
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

    fn recalled(&mut self, h: Held) {
        self.send_anyway = None;
        self.put_back(&h.text, false);
        self.say(format!("recalled: not sent to {}", h.label), false);
        self.invalidate();
    }

    /// The feed's entries for the messages waiting, after the messages: each
    /// marked on the warning ground with its countdown, where it goes, and
    /// the key that recalls it. Shown in every view that sends, since Esc
    /// recalls the newest wherever the keyboard is in the box.
    pub(super) fn pending_entries(&self, fg: &Tab) -> Vec<Entry> {
        if !fg.composes() {
            return Vec::new();
        }
        let now = self.now();
        let newest = self.pending.iter().max_by_key(|h| h.touched).map(|h| h.id);
        self.pending
            .iter()
            .map(|h| {
                // A band as a message's header is: each step on a ground of
                // its own, the countdown on the warning ground.
                let how = if newest == Some(h.id) { " Esc or click recalls " } else { " click recalls " };
                let header = vec![
                    Seg::on(format!(" sending in {}s ", left(h, now)), Ground::Warn).bold(),
                    Seg::on(format!(" → {} ", h.label), Ground::AccentDim),
                    Seg::on(how, Ground::Rule),
                ];
                let body = h.text.split('\n').map(|l| Line::styled(l.to_string(), theme::muted())).collect();
                Entry { header, body }
            })
            .collect()
    }

    /// The messages waiting, as the exit guard names them.
    pub(super) fn pending_unsaved(&self) -> Option<String> {
        match self.pending.len() {
            0 => None,
            1 => Some("a message waiting to send".into()),
            n => Some(format!("{n} messages waiting to send")),
        }
    }

    /// The exit guard's button that sends the messages waiting.
    pub(super) fn pending_quit_keeping(&self) -> Option<(String, char)> {
        match self.pending.len() {
            0 => None,
            1 => Some(("Send and quit".into(), 's')),
            n => Some((format!("Send {n} and quit"), 's')),
        }
    }

    /// Quitting, the operator chose to send what waits: each now, to a
    /// quiet agent too, as send-now sends. False when one was refused: it
    /// is back in the compose box, the reason on the status line, and the
    /// chat stays open rather than lose it.
    pub(super) fn send_all_now(&mut self) -> bool {
        let mut all = true;
        for h in self.pending.take_all() {
            all &= self.send_held(h, true);
        }
        self.invalidate();
        all
    }

    /// Quitting, the operator chose to discard what waits.
    pub(super) fn discard_pending(&mut self) {
        self.pending.take_all();
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
        let shown = self.pending.countdown(self.now());
        if shown != self.pending_shown {
            self.pending_shown = shown;
            self.invalidate();
        }
    }

    /// Whether a message waits, for Esc in the box.
    pub(super) fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    const DELAY: Duration = Duration::from_secs(5);

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_790_000_000 + secs)
    }

    fn hold(p: &mut Pending, text: &str, dest: &str, now: SystemTime) -> Hold {
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
        assert_eq!(p.len(), 2);
    }

    #[test]
    fn a_joined_message_starts_the_countdown_again() {
        let mut p = Pending::default();
        hold(&mut p, "first", "#deploy", at(0));
        assert_eq!(p.countdown(at(0)), [5]);
        assert_eq!(p.countdown(at(3)), [2]);
        hold(&mut p, "second", "#deploy", at(4));
        assert_eq!(p.countdown(at(4)), [5], "five seconds from the second Enter");
        assert!(p.due(at(5)).is_empty(), "not due when the first countdown would have ended");
        assert_eq!(p.due(at(9)).into_iter().map(|h| h.text).collect::<Vec<_>>(), ["first\nsecond"]);
        assert!(p.is_empty());
    }

    #[test]
    fn the_countdown_rounds_up_to_whole_seconds() {
        let mut p = Pending::default();
        hold(&mut p, "x", "#open", at(0));
        assert_eq!(p.countdown(at(0) + Duration::from_millis(100)), [5]);
        assert_eq!(p.countdown(at(4) + Duration::from_millis(900)), [1]);
        assert_eq!(p.countdown(at(6)), [0], "past due reads as nothing left");
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
}
