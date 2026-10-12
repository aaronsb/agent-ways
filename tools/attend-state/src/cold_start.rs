//! The cold-start rule: what a conduit delivers on its first scan when the
//! session has no seen-set yet.
//!
//! Both conduits apply it, the Stop-hook drain and the peers sensor under
//! `attend run`, so a session sees the same messages whichever conduit runs
//! first after it enrolls (ADR-172, addendum of 2026-10-01).
//!
//! - **Addressed messages** (the project tray: `attend send --to`) are
//!   addressed to the sessions live in the project when they were sent
//!   (ADR-401), so one sent since this session began is delivered whatever
//!   its age: the newest [`ADDRESSED_MAX`]; any beyond that are marked seen
//!   and counted. One sent before the session began was for whoever was
//!   there then, read or not, and a new session is replayed nothing from
//!   before it began (ADR-404 D3b): it is backlog. Where the session's
//!   start is unknown, addressed mail is judged by age as below.
//! - **`#open` and channel messages** younger than [`FRESH_WINDOW`] are live
//!   conversation and are delivered. Older ones are backlog: marked seen
//!   without being shown, and counted.
//!
//! The count goes out once, as [`Plan::note`], with the first delivery, so
//! nothing is consumed silently; `attend inbox` still lists every message.
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
/// cold start. It exceeds the sensor's checkpoint cadence, so the first
/// checkpoint cannot race a live message into the backlog.
pub const FRESH_WINDOW: Duration = Duration::from_secs(120);

/// At most this many addressed messages are delivered on a cold start.
pub const ADDRESSED_MAX: usize = 50;

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
        1 => Some("1 older message held back; reading it is optional: attend inbox".to_string()),
        n => Some(format!("{n} older messages held back; reading them is optional: attend inbox")),
    }
}

/// Apply the rule to the pending messages from other senders, each given by
/// its room and age. `session_age` is how long ago the session began, when
/// known: addressed mail no older than that was addressed to it.
pub fn plan(pending: &[(&Room, Duration)], session_age: Option<Duration>) -> Plan {
    let mut deliver: Vec<bool> = pending
        .iter()
        .map(|(room, age)| match (room, session_age) {
            (Room::Project, Some(began)) => *age <= began,
            _ => *age <= FRESH_WINDOW,
        })
        .collect();
    // Addressed mail beyond the newest ADDRESSED_MAX is counted, not shown.
    let mut addressed: Vec<usize> = (0..pending.len()).filter(|&i| matches!(pending[i].0, Room::Project)).collect();
    addressed.sort_by_key(|&i| pending[i].1);
    for &i in addressed.iter().skip(ADDRESSED_MAX) {
        deliver[i] = false;
    }
    let not_shown = deliver.iter().filter(|d| !**d).count();
    Plan { deliver, not_shown }
}

/// How long ago session `session_id` began, for [`plan`]: its Claude Code
/// process's start, from the session record. A `/clear` or a resume keeps
/// the session's seen-set (it moves with the id), so the cold start this
/// serves is a new session's, or one that turns listening back on.
pub fn session_age(session_id: &str) -> Option<Duration> {
    attend_presence::session::started_at(session_id).map(|t| t.elapsed().unwrap_or_default())
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

    #[test]
    fn addressed_mail_since_the_session_began_is_delivered_whatever_its_age() {
        let p = plan(&[(&Room::Project, 10 * HOUR), (&Room::Open, 10 * HOUR), (&Room::Open, Duration::ZERO)], Some(11 * HOUR));
        assert_eq!(p.deliver, vec![true, false, true]);
        assert_eq!(p.note().as_deref(), Some("1 older message held back; reading it is optional: attend inbox"));
    }

    /// The observation behind the rule: a new session in a project was
    /// replayed the mail its predecessor there had read and answered.
    #[test]
    fn addressed_mail_from_before_the_session_began_is_backlog() {
        let p = plan(&[(&Room::Project, 10 * HOUR), (&Room::Project, HOUR), (&Room::Project, Duration::ZERO)], Some(2 * HOUR));
        assert_eq!(p.deliver, vec![false, true, true]);
        let fresh = plan(&[(&Room::Project, FRESH_WINDOW / 2)], Some(Duration::from_secs(10)));
        assert_eq!(fresh.deliver, vec![false], "live a moment before the session began is still before it");
    }

    #[test]
    fn addressed_mail_with_no_known_start_is_judged_by_age() {
        let p = plan(&[(&Room::Project, HOUR), (&Room::Project, FRESH_WINDOW / 2)], None);
        assert_eq!(p.deliver, vec![false, true]);
    }

    #[test]
    fn channel_backlog_is_counted_like_open() {
        let ch = Room::Channel("side".into());
        let p = plan(&[(&ch, 3 * FRESH_WINDOW), (&ch, FRESH_WINDOW / 2)], Some(10 * HOUR));
        assert_eq!(p.deliver, vec![false, true], "the session's start does not reach channels");
    }

    #[test]
    fn only_the_newest_addressed_messages_are_shown() {
        let ages: Vec<Duration> = (0..ADDRESSED_MAX as u64 + 3).map(|i| Duration::from_secs(i * 60)).collect();
        let pending: Vec<(&Room, Duration)> = ages.iter().rev().map(|a| (&Room::Project, *a)).collect();
        let p = plan(&pending, Some(10 * HOUR));
        assert_eq!(p.not_shown, 3);
        // Input runs oldest first, so the three oldest are the ones held back.
        assert_eq!(p.deliver[..3], [false, false, false]);
        assert!(p.deliver[3..].iter().all(|d| *d));
        assert_eq!(p.note().as_deref(), Some("3 older messages held back; reading them is optional: attend inbox"));
    }

    #[test]
    fn nothing_held_back_says_nothing() {
        assert_eq!(plan(&[], None).note(), None);
    }
}
