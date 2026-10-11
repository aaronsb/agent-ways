//! `inbox`: the messages in this session's receive set, read-only.
//!
//! Listing and reading consult no seen-set and mark nothing: consumption is
//! the drain's and the peers sensor's (ADR-172, ADR-501 item 4).

use crate::util::{current_session, groups_for};
use agent_identity::{is_valid_signal_id, parse_signal};
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

impl Default for InboxQuery {
    fn default() -> Self {
        Self { limit: 25, page: 1, before: None }
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
    let me = current_session();
    // An unresolved session's id is a `pid-` placeholder no sender carries,
    // so nothing is skipped as its own.
    let own_session_id = me.session_id.as_str();
    let mut messages: Vec<Message> = Vec::new();
    // One registry snapshot per distinct cwd for this listing.
    let instances = SnapshotCache::new();

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
            let mtime = std::fs::metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(std::time::UNIX_EPOCH);
            let Ok(content) = std::fs::read_to_string(&path) else { continue };
            let Some(sig) = parse_signal(wire_line(&content)) else { continue };
            // Skip own messages
            if sig.from.split_once(':').is_some_and(|(_, identity)| identity == own_session_id) {
                continue;
            }
            messages.push(Message {
                id: path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string(),
                scope,
                sender: render_sender_label(sig.from, sig.cwd, &agent_theme::Painter::plain(), &instances),
                from: sig.from.to_string(),
                source: sig.cwd.to_string(),
                message: sig.message.to_string(),
                re: sig.reply_to.map(str::to_string),
                ts: mtime_secs(mtime),
                mtime,
            });
        }
    }

    // Sort chronologically — oldest first (ledger order)
    messages.sort_by_key(|m| m.mtime);
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
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "no message by that id — already consumed or expired"),
            Self::Unreadable { error } => write!(f, "the message exists but could not be read: {error}"),
            Self::Corrupt => write!(f, "the message exists but its wire format is corrupt"),
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
        let content = std::fs::read_to_string(&path).map_err(|e| ReadError::Unreadable { error: e.to_string() })?;
        let sig = parse_signal(wire_line(&content)).ok_or(ReadError::Corrupt)?;
        let mtime = std::fs::metadata(&path).ok().and_then(|m| m.modified().ok()).unwrap_or(std::time::UNIX_EPOCH);
        let scope = match dir.room {
            Room::Open => "#open",
            Room::Project => "project",
            Room::Channel(_) => "channel",
        };
        return Ok(Message {
            id: id.to_string(),
            scope,
            sender: render_sender_label(sig.from, sig.cwd, &agent_theme::Painter::plain(), &SnapshotCache::new()),
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

/// A signal file's record without the one newline `format_signal` ends it
/// with. Only that newline goes: the message is the rest of the line as the
/// sender wrote it, trailing spaces and inner newlines included.
fn wire_line(content: &str) -> &str {
    content.strip_suffix('\n').unwrap_or(content)
}

/// Seconds since the epoch for a file mtime (0 if before the epoch).
fn mtime_secs(t: SystemTime) -> u64 {
    t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}
