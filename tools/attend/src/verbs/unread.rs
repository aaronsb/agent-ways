//! `unread`: how many messages the drain would deliver to this session now,
//! by room, without marking any of them read (ADR-406 item 10).
//!
//! The count is the drain's own scan (`scan_pending`) over the rooms the
//! session receives, against its seen-set, with nothing written. A session
//! no conduit has baselined yet is counted as its cold start would deliver:
//! addressed mail and fresh messages, with the older backlog it would hold
//! back counted apart.

use crate::util::{current_session, groups_for};
use attend_groups::Room;
use serde::Serialize;

/// This session's unread messages.
#[derive(Debug, Clone, Serialize)]
pub struct Unread {
    /// Every unread message, across rooms.
    pub unread: usize,
    /// Older backlog a cold start would mark read without showing. Zero for
    /// a session already baselined.
    pub held_back: usize,
    /// Whether the drain delivers to this session (`attend listen`).
    pub listening: bool,
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

/// Count this session's unread messages. Reads only.
pub fn unread() -> Unread {
    let me = current_session();
    let dirs = groups_for(&me).receive_dirs(&me.origin_path);
    let snapshot = me.resolved().then(|| attend_state::StateStore::new(Some(me.session_id.clone())).load()).flatten();
    let baselining = !snapshot.as_ref().is_some_and(|s| s.baselined);
    let seen = snapshot.map(|s| s.seen_signals).unwrap_or_default();
    let scan = crate::cmd::inbox::scan_pending(&dirs, &seen, &me.session_id, baselining);

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
    Unread {
        unread: scan.delivered.len(),
        held_back: scan.held_back,
        listening: me.resolved() && attend_presence::enrollment::is_enrolled(&me.session_id),
        rooms,
    }
}

fn room_of(r: &RoomCount) -> Room {
    match (r.kind, &r.name) {
        ("channel", Some(n)) => Room::Channel(n.clone()),
        ("open", _) => Room::Open,
        _ => Room::Project,
    }
}
