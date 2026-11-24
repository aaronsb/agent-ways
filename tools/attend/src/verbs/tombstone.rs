// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! `tombstone`: mark a session that has exited so every reader skips its
//! messages (ADR-404 D3e).
//!
//! The mark is a file in the mark store (`attend_groups::tombstones`), and
//! it hides what the session sent up to the mark. Only an agent's session
//! id is tombstoned, and only one that has gone (`attend_presence::gone`)
//! that no session that may still read the bus reads as, its ids before
//! `/clear` and the caller's own included: those messages are still that
//! session's to read ([`Readers::refusal`]). No message is marked or
//! removed; `attend message delete` does that.
//!
//! `--ended` tombstones, as one batch, every agent that sent a message
//! still shown on the bus and that the same rule allows, and `--undo`
//! takes a batch back.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::SystemTime;

use serde::Serialize;

use crate::util::signals_base;
use crate::verbs::delete::{scope_dirs, Scope};
use crate::verbs::recipients::{caller_ids, live_sessions, reading_ids_of};
use attend_groups::tombstones::{self, Marks, SessionUndo};

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

/// What `--ended` tombstoned, or would on a dry run.
#[derive(Debug, Clone, Serialize)]
pub struct Ended {
    pub dry_run: bool,
    /// How many sessions it tombstoned, or would.
    pub count: usize,
    /// The batch `--undo` names. `None` on a dry run or when there was
    /// nothing to tombstone.
    pub batch: Option<String>,
    /// The sessions, sorted.
    pub sessions: Vec<String>,
}

/// What `--undo` took back of a batch.
#[derive(Debug, Clone, Serialize)]
pub struct EndedUndone {
    pub batch: String,
    /// How long ago the batch was written, in seconds.
    pub age_secs: u64,
    /// How many tombstones it removed, or moved back to their earlier cut.
    pub restored: usize,
    /// Those sessions.
    pub sessions: Vec<String>,
    /// The batch's sessions it left: tombstoned again since, or no longer
    /// tombstoned.
    pub kept: Vec<String>,
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
    /// `--undo` found no such batch, or none at all.
    NoBatch { batch: Option<String> },
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
            TombstoneError::NoBatch { batch: Some(b) } => write!(f, "no tombstone batch {b} to undo"),
            TombstoneError::NoBatch { batch: None } => write!(f, "no tombstone batch to undo"),
            TombstoneError::Write { error } => write!(f, "could not write the tombstone: {error}"),
        }
    }
}

/// Who may still read the bus, read once: the ids the caller reads as, and
/// each live session with the ids it reads as.
struct Readers {
    caller: Vec<String>,
    live: Vec<(String, Vec<String>)>,
}

impl Readers {
    /// The readers as they stand now.
    fn now() -> Readers {
        let live = live_sessions().into_iter().map(|(id, _)| {
            let ids = reading_ids_of(&id);
            (id, ids)
        });
        Readers { caller: caller_ids(), live: live.collect() }
    }

    /// Why `session` is not tombstoned, if it is not: it is no agent's
    /// session id, the caller reads as it, it is live, a live session
    /// reads as it since a `/clear`, or `gone` does not hold for it.
    fn refusal(&self, session: &str, gone: &dyn Fn(&str) -> bool) -> Option<TombstoneError> {
        let session_s = || session.to_string();
        if !tombstones::valid_session_id(session) {
            return Some(TombstoneError::InvalidId { session: session_s() });
        }
        if self.caller.iter().any(|id| id == session) {
            return Some(TombstoneError::ReadAs { session: session_s(), by: self.caller[0].clone() });
        }
        for (live, ids) in &self.live {
            if live == session {
                return Some(TombstoneError::NotExited { session: session_s() });
            }
            if ids.iter().any(|id| id == session) {
                return Some(TombstoneError::ReadAs { session: session_s(), by: live.clone() });
            }
        }
        (!gone(session)).then(|| TombstoneError::NotExited { session: session_s() })
    }
}

/// Tombstone session `session`.
pub fn tombstone(session: &str) -> Result<Tombstoned, TombstoneError> {
    if let Some(e) = Readers::now().refusal(session, &attend_presence::gone) {
        return Err(e);
    }
    let moved =
        tombstones::mark_session(&signals_base(), session).map_err(|e| TombstoneError::Write { error: e.to_string() })?;
    Ok(Tombstoned { session: session.to_string(), tombstoned: true, moved, at: Some(agent_fmt::when::now_utc_iso()) })
}

/// Remove session `session`'s tombstone, whichever tombstone wrote it.
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

/// Tombstone every ended session, as one batch, or with `dry_run` say which
/// it would: each agent with a message still shown on the bus that the
/// single tombstone's rule allows ([`Readers::refusal`]).
pub fn ended(dry_run: bool) -> Result<Ended, TombstoneError> {
    let base = signals_base();
    let readers = Readers::now();
    let sessions: Vec<String> =
        senders_shown(&base).into_iter().filter(|s| readers.refusal(s, &attend_presence::gone).is_none()).collect();
    if dry_run || sessions.is_empty() {
        return Ok(Ended { dry_run, count: sessions.len(), batch: None, sessions });
    }
    match tombstones::mark_sessions(&base, &sessions) {
        Ok(Some((batch, sessions))) => Ok(Ended { dry_run, count: sessions.len(), batch: Some(batch), sessions }),
        Ok(None) => Ok(Ended { dry_run, count: 0, batch: None, sessions: Vec::new() }),
        Err(e) => Err(TombstoneError::Write { error: e.to_string() }),
    }
}

/// The agents with a message on the bus that no reader skips, in every
/// room a delete `--everywhere` covers. An agent whose every message is
/// marked, or sent before its tombstone, shows nothing to tombstone.
fn senders_shown(base: &Path) -> BTreeSet<String> {
    let marks = Marks::load(base);
    let Ok((dirs, _)) = scope_dirs(base, &Scope::Everywhere) else { return BTreeSet::new() };
    let mut out = BTreeSet::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_string) else { continue };
            if !name.ends_with(".signal") || marks.hides(&attend_state::seen_key(&name)) {
                continue;
            }
            let Ok(sent) = entry.metadata().and_then(|m| m.modified()) else { continue };
            let Ok(content) = agent_identity::signal::read_signal_file(&entry.path()) else { continue };
            let Some(sig) = agent_identity::parse_signal(agent_identity::wire_line(&content)) else { continue };
            let sender = sig.sender_id();
            if tombstones::valid_session_id(&sender) && !marks.hides_sender(&sender, sent) {
                out.insert(sender);
            }
        }
    }
    out
}

/// Take back tombstone batch `batch`, or the newest when `None`.
pub fn undo_ended(batch: Option<&str>) -> Result<EndedUndone, TombstoneError> {
    match tombstones::undo_sessions(&signals_base(), batch) {
        Ok(SessionUndo::Undone { batch, restored, kept }) => Ok(EndedUndone {
            age_secs: tombstones::batch_time(&batch)
                .and_then(|t| SystemTime::now().duration_since(t).ok())
                .map_or(0, |d| d.as_secs()),
            batch,
            restored: restored.len(),
            sessions: restored,
            kept,
        }),
        Ok(SessionUndo::NoBatch) => Err(TombstoneError::NoBatch { batch: batch.map(str::to_string) }),
        Err(e) => Err(TombstoneError::Write { error: e.to_string() }),
    }
}
