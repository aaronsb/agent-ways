//! `join`, `add`, `leave` and `channels`: channel membership, the chat-idiom verbs
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
            crate::util::settle_channel_enrollment(&me);
            Err(JoinError::WriteFailed { message })
        }
    }
}

/// A member added to a channel (ADR-404 D1).
#[derive(Debug, Clone, Serialize)]
pub struct Added {
    /// The channel, without its `#`.
    pub name: String,
    /// The member by the name peers see it by.
    pub member: String,
    pub session_id: String,
    /// How many of the channel's messages it has to read.
    pub to_read: usize,
    /// How many older ones it was not handed.
    pub held: usize,
    /// Why the message that tells the member was not written; `None` when
    /// it was.
    pub not_told: Option<String>,
}

/// Why an add did not take. Nothing was written.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AddError {
    /// The member names no single live session.
    NoSession { message: String },
    /// No channel was named, and this session is not in exactly one.
    NoChannel { joined: Vec<String> },
    /// `#open`, an unknown channel, a member already in it, or a failed write.
    Refused { message: String },
}

impl std::fmt::Display for AddError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSession { message } | Self::Refused { message } => f.write_str(message),
            Self::NoChannel { joined } if joined.is_empty() => f.write_str("name the channel: this session is in none"),
            Self::NoChannel { joined } => {
                let names: Vec<String> = joined.iter().map(|n| format!("#{n}")).collect();
                write!(f, "name the channel: this session is in {}", names.join(", "))
            }
        }
    }
}

impl std::error::Error for AddError {}

/// Add the session `member` names to `channel` (ADR-404 D1), or to the one
/// channel this session is in when none is named. It is enrolled with no
/// answer from it, told by a direct message, and handed the channel's
/// recent history to read, as a join answering an invitation is.
pub fn add(member: &str, channel: Option<&str>) -> Result<Added, AddError> {
    let me = current_session();
    let (session_id, name, root) =
        crate::verbs::send::resolve_session(&me, member).map_err(|e| AddError::NoSession { message: e.to_string() })?;
    let channel = match channel {
        Some(c) => c.trim_start_matches('#').to_string(),
        None => match groups_for(&me).joined_group_names().as_slice() {
            [one] => one.clone(),
            joined => return Err(AddError::NoChannel { joined: joined.to_vec() }),
        },
    };
    let target = attend_membership::Target { session_id: &session_id, root: &root };
    let actor = crate::verbs::send::actor(&me);
    match attend_membership::add(&attend_membership::Bus::here(), &actor, &channel, target) {
        Ok(added) => Ok(Added {
            name: channel,
            member: name,
            session_id: session_id.clone(),
            to_read: added.to_read,
            held: added.held,
            not_told: added.told.err(),
        }),
        Err(attend_membership::Refused::AlreadyMember(c)) => {
            Err(AddError::Refused { message: format!("{name} is already in #{c}") })
        }
        Err(e) => Err(AddError::Refused { message: e.to_string() }),
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
    crate::util::settle_channel_enrollment(&me);
    Left { name: name.to_string(), was_member: true }
}

/// A named channel. The base channel `#open` is implicit and not listed:
/// every session is in it.
#[derive(Debug, Clone, Serialize)]
pub struct Channel {
    pub name: String,
    pub members: usize,
    /// The members by name ([`crate::verbs::peers::member_names`]), for a
    /// channel this session is in; empty for any other, which shows only
    /// its count.
    pub member_names: Vec<String>,
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
    let roster = crate::verbs::peers::peers();
    let mut names: Vec<&String> = state.keys().collect();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let entry = &state[name];
            Channel {
                name: name.clone(),
                members: entry.members.len(),
                member_names: if joined.contains(name) {
                    crate::verbs::peers::member_names(&entry.members, &roster.me, &roster.peers)
                } else {
                    Vec::new()
                },
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
