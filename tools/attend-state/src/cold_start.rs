//! The cold-start rule: what a conduit delivers on its first scan when the
//! session has no seen-set yet.
//!
//! Both conduits apply it, the Stop-hook drain and the peers sensor under
//! `attend run`, so a session sees the same messages whichever conduit runs
//! first after it enrolls (ADR-172, addendum of 2026-10-01).
//!
//! - **Addressed messages** (the project tray: `attend send --to`) are never
//!   baselined away, whatever their age. The newest [`ADDRESSED_MAX`] are
//!   delivered; any beyond that are marked seen and counted.
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
        match self.not_shown {
            0 => None,
            1 => Some("1 earlier message not shown; attend inbox".to_string()),
            n => Some(format!("{n} earlier messages not shown; attend inbox")),
        }
    }
}

/// Apply the rule to the pending messages from other senders, each given by
/// its room and age.
pub fn plan(pending: &[(&Room, Duration)]) -> Plan {
    let mut deliver: Vec<bool> = pending
        .iter()
        .map(|(room, age)| matches!(room, Room::Project) || *age <= FRESH_WINDOW)
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn addressed_mail_is_delivered_whatever_its_age() {
        let p = plan(&[(&Room::Project, 10 * HOUR), (&Room::Open, 10 * HOUR), (&Room::Open, Duration::ZERO)]);
        assert_eq!(p.deliver, vec![true, false, true]);
        assert_eq!(p.note().as_deref(), Some("1 earlier message not shown; attend inbox"));
    }

    #[test]
    fn channel_backlog_is_counted_like_open() {
        let ch = Room::Channel("side".into());
        let p = plan(&[(&ch, 3 * FRESH_WINDOW), (&ch, FRESH_WINDOW / 2)]);
        assert_eq!(p.deliver, vec![false, true]);
    }

    #[test]
    fn only_the_newest_addressed_messages_are_shown() {
        let ages: Vec<Duration> = (0..ADDRESSED_MAX as u64 + 3).map(|i| Duration::from_secs(i * 60)).collect();
        let pending: Vec<(&Room, Duration)> = ages.iter().rev().map(|a| (&Room::Project, *a)).collect();
        let p = plan(&pending);
        assert_eq!(p.not_shown, 3);
        // Input runs oldest first, so the three oldest are the ones held back.
        assert_eq!(p.deliver[..3], [false, false, false]);
        assert!(p.deliver[3..].iter().all(|d| *d));
        assert_eq!(p.note().as_deref(), Some("3 earlier messages not shown; attend inbox"));
    }

    #[test]
    fn nothing_held_back_says_nothing() {
        assert_eq!(plan(&[]).note(), None);
    }
}
