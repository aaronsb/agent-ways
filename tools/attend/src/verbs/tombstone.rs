//! `tombstone`: mark a session that has exited so every reader skips its
//! messages (ADR-404 D3e).
//!
//! The mark is an empty file in the mark store
//! (`attend_groups::tombstones`). It refuses a session that is live, and
//! one a live session reads as, its ids before `/clear`, the caller's own
//! included: those messages are still that session's to read. No message
//! is marked or removed; `attend message delete` does that.

use serde::Serialize;

use crate::util::signals_base;
use crate::verbs::delete::{caller_ids, live_sessions, reading_ids_of};
use attend_groups::tombstones;

/// A tombstone written or removed.
#[derive(Debug, Clone, Serialize)]
pub struct Tombstoned {
    pub session: String,
    /// Whether the session is tombstoned now.
    pub tombstoned: bool,
}

/// Every tombstone.
#[derive(Debug, Clone, Serialize)]
pub struct Tombstones {
    pub sessions: Vec<String>,
}

/// Why a tombstone was not written or removed.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TombstoneError {
    /// The session is live.
    Live { session: String },
    /// A live session, or the caller, reads as this id.
    ReadAs { session: String, by: String },
    /// Not in the form of a session id.
    InvalidId { session: String },
    /// `--undo` found no tombstone.
    NotTombstoned { session: String },
    /// The mark store could not be written.
    Write { error: String },
}

impl std::fmt::Display for TombstoneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TombstoneError::Live { session } => write!(f, "{session} is live; only a session that has exited is tombstoned"),
            TombstoneError::ReadAs { session, by } if session == by => write!(f, "{session} is this session"),
            TombstoneError::ReadAs { session, by } => {
                write!(f, "{session} is an earlier id of live session {by}, which still reads its mail")
            }
            TombstoneError::InvalidId { session } => write!(f, "`{session}` is not a session id"),
            TombstoneError::NotTombstoned { session } => write!(f, "{session} is not tombstoned"),
            TombstoneError::Write { error } => write!(f, "could not write the tombstone: {error}"),
        }
    }
}

/// Tombstone session `session`.
pub fn tombstone(session: &str) -> Result<Tombstoned, TombstoneError> {
    if !tombstones::valid_session_id(session) {
        return Err(TombstoneError::InvalidId { session: session.to_string() });
    }
    let caller = caller_ids();
    if caller.iter().any(|id| id == session) {
        return Err(TombstoneError::ReadAs { session: session.to_string(), by: caller[0].clone() });
    }
    if attend_presence::alive(session, &Default::default()) {
        return Err(TombstoneError::Live { session: session.to_string() });
    }
    for (live, _) in live_sessions() {
        if reading_ids_of(&live).iter().any(|id| id == session) {
            return Err(TombstoneError::ReadAs { session: session.to_string(), by: live });
        }
    }
    tombstones::mark_session(&signals_base(), session).map_err(|e| TombstoneError::Write { error: e.to_string() })?;
    Ok(Tombstoned { session: session.to_string(), tombstoned: true })
}

/// Remove session `session`'s tombstone.
pub fn undo(session: &str) -> Result<Tombstoned, TombstoneError> {
    match tombstones::unmark_session(&signals_base(), session) {
        Ok(true) => Ok(Tombstoned { session: session.to_string(), tombstoned: false }),
        Ok(false) => Err(TombstoneError::NotTombstoned { session: session.to_string() }),
        Err(e) => Err(TombstoneError::Write { error: e.to_string() }),
    }
}

/// Every tombstoned session.
pub fn list() -> Tombstones {
    Tombstones { sessions: tombstones::sessions(&signals_base()) }
}
