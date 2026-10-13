//! The cold-start rule: what a conduit delivers on its first scan when the
//! session has no seen-set yet.
//!
//! Both conduits apply it, the Stop-hook drain and the peers sensor under
//! `attend run`, so a session sees the same messages whichever conduit runs
//! first after it enrolls (ADR-172, addendum of 2026-10-01).
//!
//! A message is replayed to a session that was among those it was said to
//! (ADR-404 D3b), judged by the [`Window`] the session's own records give:
//!
//! - **Addressed messages** (the project tray: `attend message send --to`) are
//!   addressed to the sessions live in the project when they were sent
//!   (ADR-401). One sent since this session's Claude Code process started
//!   is delivered whatever its age, the newest [`ADDRESSED_MAX`]. One sent
//!   before was for whoever was there then, read or not: a new session is
//!   replayed nothing from before it began.
//! - **`#open` and channel messages** posted since the session started
//!   listening are delivered, the newest [`ROOM_MAX`]: from then it was a
//!   member (`#open` takes an implicit join on enrollment, D3a; a later
//!   channel join baselines that channel's older messages itself). A message
//!   younger than [`FRESH_WINDOW`] is live conversation and is delivered in
//!   any case.
//! - Where a record gives no time, the room is judged by [`FRESH_WINDOW`]
//!   alone.
//!
//! Anything else is backlog: marked seen without being shown, and counted.
//! The count goes out once, as [`Plan::note`], with the first delivery, so
//! nothing is consumed silently; `attend message inbox` still lists every message.
//!
//! A room joined later is cold for that room alone: [`room_backlog`] names
//! its old messages, which the join marks seen before the membership is
//! written, so a warm session that joins a channel is not handed its
//! history either (ADR-136 Decision 2: the baseline "keeps a fresh join
//! from dumping history").

use std::path::Path;
use std::time::Duration;

use attend_groups::Room;

/// Messages in `#open` or a channel younger than this are delivered on a
/// cold start whatever the session's records say. It exceeds the sensor's
/// checkpoint cadence, so the first checkpoint cannot race a live message
/// into the backlog.
pub const FRESH_WINDOW: Duration = Duration::from_secs(120);

/// At most this many addressed messages are delivered on a cold start.
pub const ADDRESSED_MAX: usize = 50;

/// At most this many `#open` and channel messages are delivered on a cold
/// start, the newest.
pub const ROOM_MAX: usize = 50;

/// How far back a cold start replays, from the session's own records.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Window {
    /// How long ago the session's Claude Code process started: addressed
    /// mail no older was addressed to it.
    pub session: Option<Duration>,
    /// How long ago it started listening: `#open` and channel messages no
    /// older were said while it was a member.
    pub listening: Option<Duration>,
}

impl Window {
    /// The window for session `session_id`: its process start from its
    /// Claude Code session record, its listening start from its enrollment
    /// record. A `/clear` or a resume keeps the seen-set (it moves with the
    /// id), so the cold start this serves is a new session's, or one that
    /// turns listening back on.
    pub fn for_session(session_id: &str) -> Self {
        let ago = |t: std::time::SystemTime| t.elapsed().unwrap_or_default();
        Window {
            session: attend_presence::session::started_at(session_id).map(ago),
            listening: attend_presence::enrollment::listening_since(session_id).map(ago),
        }
    }

    /// Whether a message of `age` in `room` was said to the session.
    fn reaches(&self, room: &Room, age: Duration) -> bool {
        match (room, self.session, self.listening) {
            (Room::Project, Some(began), _) => age <= began,
            (Room::Project, None, _) | (_, _, None) => age <= FRESH_WINDOW,
            (_, _, Some(joined)) => age <= joined.max(FRESH_WINDOW),
        }
    }
}

/// What the cold start does with each pending message, in input order.
#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    /// `true`: deliver it. `false`: mark it seen without showing it.
    pub deliver: Vec<bool>,
    /// How many messages are marked without being shown.
    pub not_shown: usize,
}

impl Plan {
    /// The line that announces the messages not shown, if any.
    pub fn note(&self) -> Option<String> {
        held_back_note(self.not_shown)
    }
}

/// The line that announces `n` messages marked seen without being shown.
/// It says the session may read them and need not: they are backlog from
/// before it enrolled or joined, and nothing in them is owed a reply.
pub fn held_back_note(n: usize) -> Option<String> {
    match n {
        0 => None,
        1 => Some("1 older message held back; reading it is optional: attend message inbox".to_string()),
        n => Some(format!("{n} older messages held back; reading them is optional: attend message inbox")),
    }
}

/// Apply the rule to the pending messages from other senders, each given by
/// its room and age, over the session's `window`.
pub fn plan(pending: &[(&Room, Duration)], window: Window) -> Plan {
    let mut deliver: Vec<bool> = pending.iter().map(|(room, age)| window.reaches(room, *age)).collect();
    // Beyond the newest of each kind, a message is counted, not shown.
    for (addressed, max) in [(true, ADDRESSED_MAX), (false, ROOM_MAX)] {
        let mut kind: Vec<usize> =
            (0..pending.len()).filter(|&i| deliver[i] && matches!(pending[i].0, Room::Project) == addressed).collect();
        kind.sort_by_key(|&i| pending[i].1);
        for &i in kind.iter().skip(max) {
            deliver[i] = false;
        }
    }
    let not_shown = deliver.iter().filter(|d| !**d).count();
    Plan { deliver, not_shown }
}

/// The seen-set keys of the signals in a room's directory older than
/// [`FRESH_WINDOW`]: the backlog a session joining the room does not get.
/// Younger messages are live conversation and stay deliverable. Empty when
/// the directory cannot be read.
pub fn room_backlog(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut keys: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            if !name.ends_with(".signal") {
                return None;
            }
            let age = e.metadata().ok()?.modified().ok()?.elapsed().unwrap_or_default();
            (age > FRESH_WINDOW).then(|| crate::seen_key(&name))
        })
        .collect();
    keys.sort();
    keys
}

/// The seen-set keys of a room's signals beyond the newest `keep`, by age:
/// what an invited session's join holds back of the room's history, so the
/// briefing it is handed is capped as addressed mail is ([`ADDRESSED_MAX`]).
pub fn room_beyond_newest(dir: &Path, keep: usize) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut aged: Vec<(Duration, String)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            if !name.ends_with(".signal") {
                return None;
            }
            let age = e.metadata().ok()?.modified().ok()?.elapsed().unwrap_or_default();
            Some((age, crate::seen_key(&name)))
        })
        .collect();
    // Newest first; everything after the first `keep` is held back.
    aged.sort();
    let mut out: Vec<String> = aged.into_iter().skip(keep).map(|(_, k)| k).collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beyond_the_newest_is_everything_but_the_newest_n() {
        let dir = std::env::temp_dir().join(format!("attend-room-newest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let now = std::time::SystemTime::now();
        for (name, mins) in [("a.signal", 30), ("b.signal", 20), ("c.signal", 10), ("note.invite", 40)] {
            let f = std::fs::File::create(dir.join(name)).unwrap();
            f.set_modified(now - Duration::from_secs(mins * 60)).unwrap();
        }
        assert_eq!(room_beyond_newest(&dir, 2), vec!["a.signal".to_string()]);
        assert!(room_beyond_newest(&dir, 50).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_rooms_backlog_is_its_old_signals_only() {
        let dir = std::env::temp_dir().join(format!("attend-room-backlog-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let old = std::time::SystemTime::now() - 3 * FRESH_WINDOW;
        for name in ["old-1.signal", "old-2.signal", "notes.txt"] {
            let f = std::fs::File::create(dir.join(name)).unwrap();
            f.set_modified(old).unwrap();
        }
        std::fs::write(dir.join("live.signal"), "x").unwrap();
        assert_eq!(room_backlog(&dir), vec!["old-1.signal".to_string(), "old-2.signal".to_string()]);
        assert!(room_backlog(&dir.join("absent")).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    const HOUR: Duration = Duration::from_secs(3600);
    const MINUTE: Duration = Duration::from_secs(60);

    /// A session whose process started `session` ago and that has listened
    /// since `listening` ago.
    fn window(session: Option<Duration>, listening: Option<Duration>) -> Window {
        Window { session, listening }
    }

    #[test]
    fn addressed_mail_since_the_session_began_is_delivered_whatever_its_age() {
        let w = window(Some(11 * HOUR), Some(MINUTE));
        let p = plan(&[(&Room::Project, 10 * HOUR), (&Room::Open, 10 * HOUR), (&Room::Open, Duration::ZERO)], w);
        assert_eq!(p.deliver, vec![true, false, true]);
        assert_eq!(p.note().as_deref(), Some("1 older message held back; reading it is optional: attend message inbox"));
    }

    /// The observation behind the rule: a new session in a project was
    /// replayed the mail its predecessor there had read and answered.
    #[test]
    fn addressed_mail_from_before_the_session_began_is_backlog() {
        let w = window(Some(2 * HOUR), Some(2 * HOUR));
        let p = plan(&[(&Room::Project, 10 * HOUR), (&Room::Project, HOUR), (&Room::Project, Duration::ZERO)], w);
        assert_eq!(p.deliver, vec![false, true, true]);
        let fresh = plan(&[(&Room::Project, FRESH_WINDOW / 2)], window(Some(Duration::from_secs(10)), None));
        assert_eq!(fresh.deliver, vec![false], "live a moment before the session began is still before it");
    }

    #[test]
    fn with_no_known_time_a_room_is_judged_by_age() {
        let p = plan(&[(&Room::Project, HOUR), (&Room::Project, FRESH_WINDOW / 2)], Window::default());
        assert_eq!(p.deliver, vec![false, true]);
        let p = plan(&[(&Room::Open, HOUR), (&Room::Open, FRESH_WINDOW / 2)], window(Some(10 * HOUR), None));
        assert_eq!(p.deliver, vec![false, true], "the process start does not reach #open");
    }

    /// #950 review, finding 1: a session held for minutes before its first
    /// drain still gets what `#open` and its channels carried since it
    /// started listening, and not what came before.
    #[test]
    fn room_messages_since_listening_began_are_delivered() {
        let ch = Room::Channel("side".into());
        let w = window(Some(10 * HOUR), Some(10 * MINUTE));
        let p = plan(&[(&Room::Open, HOUR), (&Room::Open, 8 * MINUTE), (&ch, 3 * FRESH_WINDOW), (&ch, HOUR)], w);
        assert_eq!(p.deliver, vec![false, true, true, false]);
        let just = plan(&[(&Room::Open, FRESH_WINDOW / 2)], window(None, Some(Duration::ZERO)));
        assert_eq!(just.deliver, vec![true], "the fresh window holds whenever listening began");
    }

    #[test]
    fn only_the_newest_of_each_kind_are_shown() {
        let ages: Vec<Duration> = (0..ADDRESSED_MAX as u64 + 3).map(|i| MINUTE * i as u32).collect();
        let w = window(Some(10 * HOUR), Some(10 * HOUR));
        for room in [Room::Project, Room::Open] {
            let pending: Vec<(&Room, Duration)> = ages.iter().rev().map(|a| (&room, *a)).collect();
            let p = plan(&pending, w);
            assert_eq!(p.not_shown, 3, "{room:?}");
            // Input runs oldest first, so the three oldest are the ones held back.
            assert_eq!(p.deliver[..3], [false, false, false]);
            assert!(p.deliver[3..].iter().all(|d| *d));
            assert_eq!(p.note().as_deref(), Some("3 older messages held back; reading them is optional: attend message inbox"));
        }
    }

    #[test]
    fn nothing_held_back_says_nothing() {
        assert_eq!(plan(&[], Window::default()).note(), None);
    }
}

