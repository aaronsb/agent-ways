//! `unread`: how many messages this session would be delivered, by room,
//! without marking any of them read (ADR-406 item 10).
//!
//! The count is the drain's own scan (`scan_pending`) over the rooms the
//! session receives, against its seen-set. It marks nothing read. Its only
//! writes are the ones `attend listen status` makes too: an enrollment this
//! Claude Code process made under its id before `/clear` moves to the
//! current id, and an opt-out whose `attend run` has exited is applied.
//!
//! When a message addressed to the session waits, it also reports whether
//! the session's prompt cache has lapsed, which holds that message for the
//! operator's next turn.
//!
//! A session that is not listening, or that no conduit has baselined yet,
//! is counted as its cold start would deliver when it listens: addressed
//! mail and fresh messages, with the older backlog it would hold back
//! counted apart.

use crate::util::{adopt_previous_enrollment, current_session, groups_for};
use attend_groups::Room;
use serde::Serialize;

/// This session's unread messages.
#[derive(Debug, Clone, Serialize)]
pub struct Unread {
    /// Every unread message, across rooms.
    pub unread: usize,
    /// Older backlog a cold start would mark read without showing. Zero for
    /// a listening session already baselined.
    pub held_back: usize,
    /// Whether the drain delivers to this session (`attend listen`).
    pub listening: bool,
    /// A message that would wake the session is waiting and its prompt
    /// cache has lapsed, so nothing wakes it: what waits goes with the
    /// operator's next turn (ADR-406 item 1).
    pub cache_lapsed: bool,
    /// One entry per room the session receives, in the drain's order:
    /// its project tray, `#open`, then each joined channel.
    pub rooms: Vec<RoomCount>,
}

/// The unread count in one room.
#[derive(Debug, Clone, Serialize)]
pub struct RoomCount {
    /// As the drain labels the room: `project`, `#open`, `@<channel>`.
    pub label: String,
    /// `project`, `open` or `channel`.
    pub kind: &'static str,
    /// The channel's name, for a channel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub count: usize,
}

/// Count this session's unread messages. Marks nothing read.
pub fn unread() -> Unread {
    let me = current_session();
    // First, so the seen-set and channels read below are the ones this
    // session has after `/clear`, not an empty new id's.
    let listening = me.resolved() && adopt_previous_enrollment(&me);
    let dirs = groups_for(&me).receive_dirs(&me.origin_path);
    let snapshot = me.resolved().then(|| attend_state::StateStore::new(Some(me.session_id.clone())).load()).flatten();
    // Not listening, the next delivery comes after the session listens
    // again, which reopens the cold start.
    let baselining = !listening || !snapshot.as_ref().is_some_and(|s| s.baselined);
    let seen = snapshot.map(|s| s.seen_signals).unwrap_or_default();
    let ids = attend_presence::enrollment::reading_ids(&me);
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    let scan = crate::cmd::inbox::scan_pending(&dirs, &seen, &ids, baselining);

    let mut rooms: Vec<RoomCount> = Vec::new();
    for dir in &dirs {
        if rooms.iter().any(|r| room_of(r) == dir.room) {
            continue;
        }
        let (label, kind, name) = match &dir.room {
            Room::Project => ("project".to_string(), "project", None),
            Room::Open => ("#open".to_string(), "open", None),
            Room::Channel(n) => (format!("@{n}"), "channel", Some(n.clone())),
        };
        let count = scan.delivered.iter().filter(|d| d.room == dir.room).count();
        rooms.push(RoomCount { label, kind, name, count });
    }
    // Read only when something addressed waits: the clock costs a `ways`
    // run, and with nothing to hold there is no hold to report.
    let cache_lapsed = scan.delivered.iter().any(|d| d.wakes) && crate::util::cache_lapsed(&me);
    Unread {
        unread: scan.delivered.len(),
        held_back: scan.held_back,
        listening,
        cache_lapsed,
        rooms,
    }
}

/// The count in one phrase: `N unread`, or `N waiting · cold` while the
/// prompt cache has lapsed under an addressed message.
pub fn headline(u: &Unread) -> String {
    if u.cache_lapsed {
        format!("{} waiting · cold", u.unread)
    } else {
        format!("{} unread", u.unread)
    }
}

fn room_of(r: &RoomCount) -> Room {
    match (r.kind, &r.name) {
        ("channel", Some(n)) => Room::Channel(n.clone()),
        ("open", _) => Room::Open,
        _ => Room::Project,
    }
}
