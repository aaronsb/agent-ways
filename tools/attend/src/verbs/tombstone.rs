//! `tombstone`: mark a session that has exited so every reader skips its
//! messages (ADR-404 D3e).
//!
//! The mark is an empty file in the mark store
//! (`attend_groups::tombstones`), and it hides what the session sent up to
//! the mark. Only an agent's session id is tombstoned, and only one that
//! has gone (`attend_presence::gone`) that no session that may
//! still read the bus reads as, its ids before `/clear` and the caller's
//! own included: those messages are still that session's to read. No
//! message is marked or removed; `attend message delete` does that.

use serde::Serialize;

use crate::util::signals_base;
use crate::verbs::recipients::{caller_ids, live_sessions, reading_ids_of};
use attend_groups::tombstones;

/// A tombstone written or removed.
#[derive(Debug, Clone, Serialize)]
pub struct Tombstoned {
    pub session: String,
    /// Whether the session is tombstoned now.
    pub tombstoned: bool,
    /// Whether a tombstone was already there and moved to now.
    pub moved: bool,
    /// When the tombstone cuts, as UTC; what the session sent up to then
    /// is hidden. `None` once removed.
    pub at: Option<String>,
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
    /// Nothing says the session has exited.
    NotExited { session: String },
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
            TombstoneError::NotExited { session } => {
                write!(
                    f,
                    "{session} has not exited; only a session this host saw exit, or one with no record here whose last heartbeat is {} days old, is tombstoned",
                    crate::cmd::cleanup::MARK_REAP_DAYS
                )
            }
            TombstoneError::ReadAs { session, by } if session == by => write!(f, "{session} is this session"),
            TombstoneError::ReadAs { session, by } => {
                write!(f, "{session} is an earlier id of live session {by}, which still reads its mail")
            }
            TombstoneError::InvalidId { session } => write!(f, "`{session}` is not a session id; only an agent's session is tombstoned"),
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
    for (live, _) in live_sessions() {
        if live == session {
            return Err(TombstoneError::NotExited { session: session.to_string() });
        }
        if reading_ids_of(&live).iter().any(|id| id == session) {
            return Err(TombstoneError::ReadAs { session: session.to_string(), by: live });
        }
    }
    if !attend_presence::gone(session) {
        return Err(TombstoneError::NotExited { session: session.to_string() });
    }
    let moved =
        tombstones::mark_session(&signals_base(), session).map_err(|e| TombstoneError::Write { error: e.to_string() })?;
    Ok(Tombstoned { session: session.to_string(), tombstoned: true, moved, at: Some(agent_fmt::when::now_utc_iso()) })
}

/// Remove session `session`'s tombstone.
pub fn undo(session: &str) -> Result<Tombstoned, TombstoneError> {
    match tombstones::unmark_session(&signals_base(), session) {
        Ok(true) => Ok(Tombstoned { session: session.to_string(), tombstoned: false, moved: false, at: None }),
        Ok(false) => Err(TombstoneError::NotTombstoned { session: session.to_string() }),
        Err(e) => Err(TombstoneError::Write { error: e.to_string() }),
    }
}

/// Every tombstoned session.
pub fn list() -> Tombstones {
    Tombstones { sessions: tombstones::sessions(&signals_base()) }
}
