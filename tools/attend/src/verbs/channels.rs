//! `join`, `leave` and `channels`: channel membership, the chat-idiom verbs
//! (ADR-173).
//!
//! Storage is untouched by the vocabulary: channels are still `@name/`
//! signal namespaces with membership in `_groups.yaml` (attend-groups).
//! A leading `#` on a name is tolerated everywhere.

use crate::util::{current_session, groups_for};
use crate::verbs::peers::JoinedChannel;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Joined {
    pub name: String,
    pub pinned: bool,
    /// What the join did with the channel's history, as one parenthesized
    /// clause; empty when there was none to speak of.
    pub note: String,
    /// The name peers see this session by, which the join tells the
    /// session as `you are <name>`; `None` for an unresolved session.
    pub you_are: Option<String>,
}

/// Why a join did not take.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JoinError {
    /// The name is not a valid channel name. Nothing was written.
    InvalidName { message: String },
    /// Writing the membership or the channel's directory failed.
    WriteFailed { message: String },
}

impl std::fmt::Display for JoinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidName { message } | Self::WriteFailed { message } => f.write_str(message),
        }
    }
}

impl std::error::Error for JoinError {}

/// Join channel `name`, creating it if absent, and enroll this session for
/// delivery (#720). Joining marks the channel's older history seen, so a late
/// join does not deliver it; a join that answers an invitation keeps the
/// recent history to read.
pub fn join(name: &str, pin: bool) -> Result<Joined, JoinError> {
    let name = name.trim_start_matches('#');
    attend_groups::validate_group_name(name).map_err(|message| JoinError::InvalidName { message })?;
    let me = current_session();
    let r = groups_for(&me);
    // Enrolled first, so a crash between the two never leaves a member
    // that is not enrolled; a failed join takes the enrollment back.
    crate::util::enroll_by_join(&me);
    // The backlog is marked before the membership is written, so no
    // conduit scans the room while its history is still unseen. A join
    // that answers an invitation keeps the room's recent history to read;
    // one that does not, holds back what is older than the fresh window.
    let note = if r.take_invite(name) {
        crate::util::brief_invited_room(&me, &r, name)
    } else {
        crate::util::backlog_note(crate::util::baseline_joined_room(&me, &r, name))
    };
    match r.join(name, pin) {
        Ok(()) => Ok(Joined { name: name.to_string(), pinned: pin, note, you_are: crate::util::you_are(&me) }),
        Err(message) => {
            crate::util::settle_join_enrollment(&me, &r);
            Err(JoinError::WriteFailed { message })
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Left {
    pub name: String,
    /// False when this session was not in the channel, so nothing changed.
    pub was_member: bool,
}

/// Leave channel `name`. A session left in no channel withdraws its join
/// enrollment.
pub fn leave(name: &str) -> Left {
    let name = name.trim_start_matches('#');
    let me = current_session();
    let r = groups_for(&me);
    if !r.my_groups().iter().any(|(n, _)| n == name) {
        return Left { name: name.to_string(), was_member: false };
    }
    r.leave(name).ok();
    crate::util::settle_join_enrollment(&me, &r);
    Left { name: name.to_string(), was_member: true }
}

/// A named channel. The base channel `#open` is implicit and not listed:
/// every session is in it.
#[derive(Debug, Clone, Serialize)]
pub struct Channel {
    pub name: String,
    pub members: usize,
    pub joined: bool,
    pub pinned: bool,
    pub description: Option<String>,
}

/// Every named channel, sorted by name. Listing first drops members whose
/// sessions are gone, as the CLI listing always has.
pub fn list() -> Vec<Channel> {
    let r = groups_for(&current_session());
    r.cleanup_stale();
    let joined: std::collections::HashSet<String> = r.my_groups().into_iter().map(|(n, _)| n).collect();
    // Single read of the whole file (PR #407 nit): a second read for
    // descriptions could misalign against a concurrent write.
    // Members/pinned/description all come from this one snapshot.
    let state = attend_groups::load_groups(&crate::util::signals_base());
    let mut names: Vec<&String> = state.keys().collect();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let entry = &state[name];
            Channel {
                name: name.clone(),
                members: entry.members.len(),
                joined: joined.contains(name),
                pinned: entry.pinned,
                description: entry.description.clone(),
            }
        })
        .collect()
}

/// The channels this session is in. Read-only, so a hook macro can call it
/// on every fire without [`list`]'s cleanup rewriting shared state.
pub fn joined() -> Vec<JoinedChannel> {
    groups_for(&current_session()).my_groups().into_iter().map(|(name, pinned)| JoinedChannel { name, pinned }).collect()
}
