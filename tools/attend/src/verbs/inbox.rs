//! `inbox`: the messages in this session's receive set, read-only.
//!
//! Listing and reading consult no seen-set and mark nothing: consumption is
//! the drain's and the peers sensor's (ADR-172, ADR-501 item 4).

use crate::util::{current_session, groups_for, Session};
use agent_identity::{is_valid_signal_id, parse_signal, wire_line};
use attend_groups::Room;
use attend_instances::view::render_sender_label;
use attend_instances::SnapshotCache;
use serde::Serialize;
use std::time::SystemTime;

/// Which page of the inbox to list. Page 1 is the newest `limit` messages.
#[derive(Debug, Clone, Copy)]
pub struct InboxQuery {
    pub limit: usize,
    pub page: usize,
    /// Only messages older than this unix timestamp.
    pub before: Option<u64>,
}

/// Messages on one inbox page when the caller names no limit.
pub const DEFAULT_LIMIT: usize = 25;

impl Default for InboxQuery {
    fn default() -> Self {
        Self { limit: DEFAULT_LIMIT, page: 1, before: None }
    }
}

/// One message from a peer.
#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub id: String,
    /// `#open`, `project` or `channel`.
    pub scope: &'static str,
    /// The sender's display label, plain. Presentation only.
    pub sender: String,
    /// The wire `from` field (`claude:<session-id>`), the canonical sender.
    pub from: String,
    /// The sender's working directory.
    pub source: String,
    pub message: String,
    /// The id this message threads under, if any.
    pub re: Option<String>,
    /// File mtime, seconds since the epoch.
    pub ts: u64,
    #[serde(skip)]
    pub mtime: SystemTime,
}

/// One page of the inbox, oldest first within the page.
#[derive(Debug, Clone, Serialize)]
pub struct InboxPage {
    pub page: usize,
    pub limit: usize,
    /// Messages in the inbox (after `before`), on every page.
    pub total: usize,
    pub pages: usize,
    /// Messages older than this page's oldest.
    pub older: usize,
    /// The `before` cursor for the next-older page: this page's oldest
    /// timestamp. `None` on an empty page.
    pub cursor: Option<u64>,
    pub messages: Vec<Message>,
}

/// List the messages from peers in this session's receive set, the same set
/// the peers sensor and the drain read, paged newest-first.
pub fn list(query: InboxQuery) -> InboxPage {
    list_with(query, &SnapshotCache::new())
}

/// [`list`], reading sender names through `instances`, the registry
/// snapshots of this listing. A caller that renders the senders again
/// passes the same cache, so the listing reads each registry once.
pub fn list_with(query: InboxQuery, instances: &SnapshotCache) -> InboxPage {
    let me = current_session();
    // An unresolved session's id is a `pid-` placeholder no sender carries,
    // so nothing is skipped as its own.
    let ids = attend_presence::enrollment::reading_ids(&me);
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    let mut messages: Vec<Message> = Vec::new();
    // A message a delete marked, or a tombstoned session sent, is gone from
    // every listing (ADR-404 D3e).
    let marks = attend_groups::tombstones::Marks::load(&crate::util::signals_base());

    for dir in &groups_for(&me).receive_dirs(&me.origin_path) {
        let Ok(dir_entries) = std::fs::read_dir(&dir.path) else { continue };
        let scope = match dir.room {
            Room::Open => "#open",
            Room::Project => "project",
            Room::Channel(_) => "channel",
        };
        for entry in dir_entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("signal") {
                continue;
            }
            if path.file_name().and_then(|n| n.to_str()).is_some_and(|n| marks.hides(&attend_state::seen_key(n))) {
                continue;
            }
            let mtime = std::fs::metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(std::time::UNIX_EPOCH);
            let Ok(content) = agent_identity::signal::read_signal_file(&path) else { continue };
            let Some(sig) = parse_signal(wire_line(&content)) else { continue };
            if marks.hides_sender(&sig.sender_id(), mtime) {
                continue;
            }
            // Skip own messages, under this id or an earlier one.
            if sig.from.split_once(':').is_some_and(|(_, identity)| ids.contains(&identity)) {
                continue;
            }
            // Another session's direct message in a shared project tray.
            if dir.room.is_tray() && !sig.tray_reads(&ids) {
                continue;
            }
            messages.push(Message {
                id: path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string(),
                scope,
                sender: render_sender_label(&sig.from, sig.cwd, &agent_theme::Painter::plain(), instances),
                from: sig.from.to_string(),
                source: sig.cwd.to_string(),
                message: sig.message.to_string(),
                re: sig.reply_to.map(str::to_string),
                ts: mtime_secs(mtime),
                mtime,
            });
        }
    }

    // Sort chronologically, oldest first (ledger order). Signals written
    // within one timestamp tick, such as the parts of a long message, share
    // an mtime; their ids carry the send time and order them.
    messages.sort_by_cached_key(|m| (m.mtime, agent_identity::signal_send_order(&m.id), m.id.clone()));
    if let Some(ts) = query.before {
        messages.retain(|m| m.ts < ts);
    }

    // Page over the oldest-first ledger; page 1 = the newest `limit`, and
    // higher page numbers walk back into history. The never-reaped ledger
    // can be long, so a bounded page keeps the listing usable.
    let limit = query.limit.max(1);
    let page = query.page.max(1);
    let total = messages.len();
    let end = total.saturating_sub((page - 1) * limit);
    let start = end.saturating_sub(limit);
    let messages: Vec<Message> = messages.drain(start..end).collect();
    InboxPage {
        page,
        limit,
        total,
        pages: total.div_ceil(limit),
        older: start,
        cursor: messages.first().map(|m| m.ts),
        messages,
    }
}

/// Why a message could not be read.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReadError {
    /// No message by that id: already reaped, or never there. A benign race.
    NotFound,
    /// The file exists and could not be read.
    Unreadable { error: String },
    /// The file exists and its wire format is corrupt.
    Corrupt,
    /// A delete marked it (ADR-404 D3e).
    Deleted { id: String, batch: String },
    /// A tombstoned session sent it.
    Tombstoned { id: String, session: String },
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "no message by that id — already consumed or expired"),
            Self::Unreadable { error } => write!(f, "the message exists but could not be read: {error}"),
            Self::Corrupt => write!(f, "the message exists but its wire format is corrupt"),
            Self::Deleted { id, batch } => write!(f, "{id} was deleted; attend message delete --undo {batch} restores it"),
            Self::Tombstoned { id, session } => {
                write!(f, "{id} was sent by tombstoned session {session}; attend message tombstone {session} --undo restores it")
            }
        }
    }
}

impl std::error::Error for ReadError {}

/// Read one message by id from this session's receive set. An id outside
/// the signal-id alphabet names no message, so it never becomes a path.
pub fn read(id: &str) -> Result<Message, ReadError> {
    if !is_valid_signal_id(id) {
        return Err(ReadError::NotFound);
    }
    let me = current_session();
    let target = format!("{id}.signal");
    for dir in &groups_for(&me).receive_dirs(&me.origin_path) {
        let path = dir.path.join(&target);
        if !path.is_file() {
            continue;
        }
        // File exists: from here on, any failure is a corrupt-file
        // condition, not a benign "already consumed" miss. Distinguish
        // them so operators can tell partial-write / disk-full bugs
        // from ordinary races.
        let content = agent_identity::signal::read_signal_file(&path).map_err(|e| ReadError::Unreadable { error: e.to_string() })?;
        let sig = parse_signal(wire_line(&content)).ok_or(ReadError::Corrupt)?;
        // Another session's direct message is no message to this one.
        let ids = attend_presence::enrollment::reading_ids(&me);
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        if dir.room.is_tray() && !sig.tray_reads(&ids) {
            return Err(ReadError::NotFound);
        }
        let mtime = std::fs::metadata(&path).ok().and_then(|m| m.modified().ok()).unwrap_or(std::time::UNIX_EPOCH);
        let base = crate::util::signals_base();
        if let Some(batch) = attend_groups::tombstones::batch_of(&base, &target) {
            return Err(ReadError::Deleted { id: id.to_string(), batch });
        }
        let sender = sig.sender_id();
        if attend_groups::tombstones::Marks::load(&base).hides_sender(&sender, mtime) {
            return Err(ReadError::Tombstoned { id: id.to_string(), session: sender });
        }
        let scope = match dir.room {
            Room::Open => "#open",
            Room::Project => "project",
            Room::Channel(_) => "channel",
        };
        return Ok(Message {
            id: id.to_string(),
            scope,
            sender: render_sender_label(&sig.from, sig.cwd, &agent_theme::Painter::plain(), &SnapshotCache::new()),
            from: sig.from.to_string(),
            source: sig.cwd.to_string(),
            message: sig.message.to_string(),
            re: sig.reply_to.map(str::to_string),
            ts: mtime_secs(mtime),
            mtime,
        });
    }
    Err(ReadError::NotFound)
}

pub use agent_identity::signal::{is_short_id, short_id};

/// The full id of the message whose handle is `handle`: first in `me`'s
/// receive set (its own messages there included), then in every channel's
/// directory, so a handle for a message in a channel `me` has left still
/// names it and the reply can say the channel was left. Where two share a
/// handle, the newest wins.
pub fn resolve_short_id(me: &Session, handle: &str) -> Option<String> {
    let receive: Vec<std::path::PathBuf> = groups_for(me).receive_dirs(&me.origin_path).into_iter().map(|d| d.path).collect();
    newest_with_handle(&receive, handle).or_else(|| {
        let channels: Vec<std::path::PathBuf> = std::fs::read_dir(crate::util::signals_base())
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir() && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('@')))
            .collect();
        newest_with_handle(&channels, handle)
    })
}

fn newest_with_handle(dirs: &[std::path::PathBuf], handle: &str) -> Option<String> {
    let mut best: Option<(SystemTime, String)> = None;
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(id) = name.to_str().and_then(|n| n.strip_suffix(".signal")) else { continue };
            if short_id(id) != handle {
                continue;
            }
            let mtime = entry.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            if best.as_ref().is_none_or(|(t, _)| mtime > *t) {
                best = Some((mtime, id.to_string()));
            }
        }
    }
    best.map(|(_, id)| id)
}

/// Seconds since the epoch for a file mtime (0 if before the epoch).
fn mtime_secs(t: SystemTime) -> u64 {
    t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The vectors the attend mod's `shortId` test pins too
    /// (plugins/attend/tests/attend.test.ts), so the two never drift.
    #[test]
    fn short_id_matches_the_mods_handle() {
        assert_eq!(short_id("peer-sess-1-1"), "73nki");
        assert_eq!(short_id(""), "tntfp");
        assert_eq!(short_id("\u{e9}-x"), "zer0p");
        assert_eq!(short_id("a"), "9wi7g");
        assert!(is_short_id("73nki"));
        assert!(!is_short_id("peer-sess-1-1"));
        assert!(!is_short_id("AB12C"));
    }
}
