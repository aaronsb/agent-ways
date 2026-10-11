//! `listen`: whether the Stop-hook drain delivers to this session, set
//! without joining a channel (ADR-406 item 10).
//!
//! Listening is enrollment (ADR-172, addendum of 2026-10-01). A session
//! listens once it runs `attend listen on`, runs `attend run`, or joins a
//! channel, and stops at `attend listen off` or `attend scene private`.
//! While it is not listening the drain delivers nothing and marks nothing
//! read; the messages wait in the store.

use crate::util::{adopt_previous_enrollment, current_session, Session};
use attend_presence::enrollment::{self, Source};
use serde::Serialize;

/// This session's listening state.
#[derive(Debug, Clone, Serialize)]
pub struct Listening {
    pub session_id: String,
    /// Whether the session id was resolved from a Claude Code session. An
    /// unresolved session never listens: the drain does not run for it.
    pub resolved: bool,
    pub listening: bool,
    /// The ways it is listening: `listen`, `run`, `join`. Empty when not.
    pub via: Vec<&'static str>,
    /// An `attend run` holds the session. It listens until that run exits,
    /// whatever `listen off` said.
    pub run_live: bool,
}

/// Why `on` or `off` changed nothing.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ListenError {
    /// No Claude Code session owns this process, so there is no session to
    /// listen for.
    Unresolved,
    /// The enrollment record could not be written.
    Unwritable { error: String },
}

impl std::fmt::Display for ListenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unresolved => write!(f, "no Claude Code session owns this process, so there is no session to listen for"),
            Self::Unwritable { error } => write!(f, "could not record the listening state: {error}"),
        }
    }
}

impl std::error::Error for ListenError {}

/// This session's state, read now. An enrollment this Claude Code process
/// made under its id before `/clear` counts, and moves to the current id.
pub fn status() -> Listening {
    let me = current_session();
    let listening = me.resolved() && adopt_previous_enrollment(&me);
    state(&me, listening)
}

/// Turn listening on. When the session was not listening, the cold-start
/// rule applies again at the next drain: what was addressed to its project
/// meanwhile is delivered, older `#open` and channel backlog is held back
/// and counted. Delivers nothing itself.
pub fn on() -> Result<Listening, ListenError> {
    let me = current_session();
    if !me.resolved() {
        return Err(ListenError::Unresolved);
    }
    let was = adopt_previous_enrollment(&me);
    enrollment::enroll(&me.session_id, Source::Listen).map_err(|e| ListenError::Unwritable { error: e.to_string() })?;
    if !was {
        attend_state::StateStore::new(Some(me.session_id.clone())).reopen_cold_start();
    }
    // A listening session looks alive to peers and to `/purge` from now,
    // not from its first drain.
    attend_presence::heartbeat::touch(&me.session_id).ok();
    Ok(state(&me, true))
}

/// Stop listening. Channel memberships stay; the drain delivers nothing
/// until the session listens again. While an `attend run` holds the session
/// it keeps listening until that run exits.
pub fn off() -> Result<Listening, ListenError> {
    let me = current_session();
    if !me.resolved() {
        return Err(ListenError::Unresolved);
    }
    adopt_previous_enrollment(&me);
    enrollment::opt_out(&me.session_id).map_err(|e| ListenError::Unwritable { error: e.to_string() })?;
    let listening = enrollment::is_enrolled(&me.session_id);
    Ok(state(&me, listening))
}

fn state(me: &Session, listening: bool) -> Listening {
    let resolved = me.resolved();
    Listening {
        session_id: me.session_id.clone(),
        resolved,
        listening,
        via: if listening { enrollment::sources(&me.session_id).into_iter().map(Source::as_str).collect() } else { Vec::new() },
        run_live: resolved && attend_presence::heartbeat::run_is_live(&me.session_id),
    }
}
