//! Shared focus-group state I/O for attend and attend-chat (ADR-170).
//!
//! Groups are named signal namespaces that agents — and, since
//! ADR-170, humans — focus on and release. Storage: `@group-name/`
//! directories under the signals base, with membership tracked in
//! `_groups.yaml`.
//!
//! This crate is the single owner of the `_groups.yaml` wire format.
//! It was extracted from `attend::groups` when attend-chat gained its
//! `/join` write path; before that, attend-chat carried a read-only
//! byte-mirror of the parser kept honest by cross-crate golden tests.
//! Those tests now live here, next to the one implementation.
//!
//! **Member identity.** A member id is either a Claude Code session
//! UUID (claude sessions, via attend) or a sanitized username (humans,
//! via attend-chat). Both kinds are judged for liveness the same way:
//! against the heartbeat sidecar (`attend_presence::heartbeat`, ADR-129). The
//! yaml does not distinguish them — liveness was always
//! heartbeat-shaped, not UUID-shaped.

pub mod readers;
pub mod tombstones;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use agent_settings::writer::Lock;

/// Group membership entry.
#[derive(Debug, Clone, Default)]
pub struct GroupEntry {
    /// Whether this group persists when empty.
    pub pinned: bool,
    /// Member ids currently in this group: Claude Code session UUIDs
    /// for claude sessions, sanitized usernames for humans (ADR-170).
    pub members: Vec<String>,
    /// Optional single-line channel description (#404). Absent in
    /// pre-#404 files — the parser treats a missing key as `None`, so
    /// old state round-trips untouched.
    pub description: Option<String>,
}

/// Group state manager.
#[derive(Clone)]
pub struct Groups {
    base: PathBuf,
    member_id: String,
    /// How long a write waits for another writer's lock; `None` waits for
    /// as long as it is held ([`Groups::waiting_at_most`]).
    lock_wait: Option<Duration>,
}

const GROUP_PREFIX: &str = "@";

/// The refusal of a write that waited its [`Groups::waiting_at_most`]
/// limit for another writer's lock.
pub const BUSY: &str = "channels are busy; try again";

/// Why [`Groups::add`] refused. Nothing was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddError {
    /// No channel by that name: an add makes no channel.
    UnknownChannel(String),
    /// The member is in the channel already.
    AlreadyMember(String),
    /// A name or id refused, the lock not taken, or the add's own step
    /// failed.
    Failed(String),
}

impl std::fmt::Display for AddError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AddError::UnknownChannel(name) => write!(f, "#{name}: unknown channel"),
            AddError::AlreadyMember(name) => write!(f, "already in #{name}"),
            AddError::Failed(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for AddError {}

/// The directory under the signals base that backs `#open`, the base
/// channel every enrolled member receives (ADR-124).
pub const BROADCAST_DIR: &str = "_broadcast";

/// The display name of the base channel, `#open` (ADR-124). A send to a
/// channel by this name goes to [`BROADCAST_DIR`].
pub const BASE_CHANNEL: &str = "open";

/// The directory under the signals base that holds the person trays:
/// `_people/<member-id>/`, one per person a message was addressed to
/// (ADR-401, `to`). attend-chat reads the tray of the person at its
/// keyboard. A person tray is never reaped; a signal in it is, when its
/// sender's project is gone.
pub const PEOPLE_DIR: &str = "_people";

/// The tray of the person with member id `member` (ADR-170) under `base`.
pub fn person_dir(base: &Path, member: &str) -> PathBuf {
    base.join(PEOPLE_DIR).join(member)
}

/// The extension of a pending invitation's record, `@<channel>/<member>.invite`.
const INVITE: &str = "invite";

/// The extension of the record of a briefing an add owes a member,
/// `@<channel>/<member>.brief` ([`Groups::brief`]).
const BRIEF: &str = "brief";

/// A pending invitation to a channel. Its record is `by:` and `at:` lines;
/// one written before they were recorded is an empty file and reads with
/// neither.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invite {
    pub channel: String,
    /// The invitee's member id: a session id for an agent.
    pub member: String,
    /// The inviter's member id.
    pub by: Option<String>,
    /// When the invitation was sent, to the second.
    pub at: Option<std::time::SystemTime>,
}

/// Which room a receive directory is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Room {
    /// The member's project tray: messages sent to its project with `--to`.
    Project,
    /// `#open`, under [`BROADCAST_DIR`].
    Open,
    /// A joined channel, by name.
    Channel(String),
}

impl Room {
    /// Whether this is a tray, read by its owner alone, rather than a room
    /// many share. The read filter and the wake rule ask it (ADR-401, `to`).
    pub fn is_tray(&self) -> bool {
        matches!(self, Room::Project)
    }

    /// Whether this is a named channel, every post in which wakes its idle
    /// members (ADR-406 item 2).
    pub fn is_channel(&self) -> bool {
        matches!(self, Room::Channel(_))
    }
}

/// One directory a member reads signals from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiveDir {
    pub path: PathBuf,
    pub room: Room,
}

/// The single-line description contract, shared by every write path
/// (create, set_description). Control characters — newlines above
/// all — are rejected, not folded: a folded payload still surprises,
/// and the yaml wire format is line-oriented (PR #407 review).
fn normalize_description(text: &str) -> Result<Option<String>, String> {
    if text.chars().any(|c| c.is_control()) {
        return Err("description must be a single line of printable text".to_string());
    }
    let t = text.trim();
    Ok((!t.is_empty()).then(|| t.to_string()))
}

/// Member-kind discriminator (#406). Agent ids are Claude Code session
/// UUIDs (8-4-4-4-12 hex) or the `pid-<n>` unresolved fallback; human
/// ids are ADR-170 sanitized usernames, which are neither. Encoded
/// once, here, next to the member-identity documentation — every
/// policy that distinguishes the kinds must call this, not re-derive
/// the shape.
pub fn is_agent_member(id: &str) -> bool {
    if id.starts_with("pid-") {
        return true;
    }
    let parts: Vec<&str> = id.split('-').collect();
    parts.len() == 5
        && [8usize, 4, 4, 4, 12]
            .iter()
            .zip(&parts)
            .all(|(len, p)| p.len() == *len && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

impl Groups {
    /// `member_id` is the identity this manager acts for: a session
    /// UUID when the caller is attend, a sanitized username when the
    /// caller is attend-chat (ADR-170).
    ///
    /// The member-scoped operations (`join`, `leave`, `take_invite`,
    /// `my_groups`, `joined_group_names`) act for it, and the records
    /// written for another member (`invite`, `brief`) name it as their
    /// writer; the collection operations (`dissolve`, `cleanup_stale`,
    /// `all_groups`, `members`, `pin`/`unpin`, `add`) act on
    /// the group state as a whole and ignore it.
    pub fn new(signals_base: &Path, member_id: &str) -> Self {
        Self {
            base: signals_base.to_path_buf(),
            member_id: member_id.to_string(),
            lock_wait: None,
        }
    }

    /// Wait at most `limit` for another writer to release `_groups.yaml`.
    /// A write that waits longer is refused with [`BUSY`] and changes
    /// nothing; a method that returns no `Result` skips its write. For a
    /// caller on a screen's own thread, which must not hang on a stuck
    /// holder.
    pub fn waiting_at_most(mut self, limit: Duration) -> Self {
        self.lock_wait = Some(limit);
        self
    }

    /// Every directory this member receives from, in a fixed order: the
    /// project tray of `origin` (named by `claude_sessions::attend_key`),
    /// `#open`, then each joined channel. The one receive set the drain,
    /// `attend message inbox`, `attend peer status` and the peers sensor all read.
    pub fn receive_dirs(&self, origin: &str) -> Vec<ReceiveDir> {
        let mut dirs = vec![
            ReceiveDir { path: self.base.join(claude_sessions::attend_key(origin)), room: Room::Project },
            ReceiveDir { path: self.base.join(BROADCAST_DIR), room: Room::Open },
        ];
        dirs.extend(
            self.joined_group_names()
                .into_iter()
                .map(|name| ReceiveDir { path: self.group_dir(&name), room: Room::Channel(name) }),
        );
        dirs
    }

    /// Path to a named group's signal directory.
    pub fn group_dir(&self, name: &str) -> PathBuf {
        self.base.join(format!("{GROUP_PREFIX}{name}"))
    }

    /// The channel whose directory holds a file named `file`, searching
    /// every channel directory on disk, joined or not.
    pub fn channel_holding(&self, file: &str) -> Option<String> {
        fs::read_dir(&self.base).ok()?.flatten().find_map(|entry| {
            let name = entry.file_name().to_str()?.strip_prefix(GROUP_PREFIX)?.to_string();
            (!name.is_empty() && entry.path().join(file).is_file()).then_some(name)
        })
    }

    /// Path to the groups state file.
    fn state_path(&self) -> PathBuf {
        self.base.join("_groups.yaml")
    }

    /// Join a named group. Creates the group if it doesn't exist, unless an
    /// earlier channel's messages are left in its folder ([`Groups::create`]).
    ///
    /// The entry and the directory are both written under the state lock,
    /// which the orphan sweep in [`Groups::cleanup_stale`] also holds, so
    /// the sweep never sees a joined channel's directory without its entry.
    pub fn join(&self, name: &str, pin: bool) -> Result<(), String> {
        validate_group_name(name)?;

        let _lock = self.lock_state()?;
        let mut state = self.load_state();
        if !state.contains_key(name) {
            self.refuse_leftover(name)?;
        }
        let entry = state.entry(name.to_string()).or_insert(GroupEntry {
            pinned: false,
            members: Vec::new(),
            description: None,
        });
        if !entry.members.contains(&self.member_id) {
            entry.members.push(self.member_id.clone());
        }
        if pin {
            entry.pinned = true;
        }
        self.save_state(&state);

        let dir = self.group_dir(name);
        fs::create_dir_all(&dir).map_err(|e| format!("creating group dir: {e}"))?;
        // A join by any path answers a pending invitation.
        fs::remove_file(self.record_path(name, &self.member_id, INVITE)).ok();
        Ok(())
    }

    /// Create a channel WITHOUT joining it (#404) — the operator's
    /// explicit lifecycle verb, distinct from join's create-implicitly.
    /// Created channels are pinned: an empty unpinned group is swept
    /// by cleanup, so an explicit create must persist until someone
    /// joins or the operator dissolves it. A name whose directory an
    /// earlier channel left with messages in it, and no entry, is refused
    /// until it is dissolved: the new channel would open on them.
    pub fn create(&self, name: &str, description: Option<&str>) -> Result<(), String> {
        validate_group_name(name)?;
        // Same single-line contract as set_description — enforced HERE,
        // not per-surface: a newline in the description is a yaml
        // injection that mints fabricated groups/members (PR #407
        // review, blocking finding — reproduced against the built
        // crate).
        let description = match description {
            Some(d) => normalize_description(d)?,
            None => None,
        };
        let _lock = self.lock_state()?;
        let mut state = self.load_state();
        if state.contains_key(name) {
            return Err(format!("#{name} already exists"));
        }
        self.refuse_leftover(name)?;
        state.insert(
            name.to_string(),
            GroupEntry {
                pinned: true,
                members: Vec::new(),
                description,
            },
        );
        self.save_state(&state);
        fs::create_dir_all(self.group_dir(name))
            .map_err(|e| format!("creating group dir: {e}"))?;
        Ok(())
    }

    /// Refuse to make channel `name` over the messages an earlier channel
    /// left in its folder: the new channel would open on them. Called with
    /// the state lock held, for a name no entry names.
    fn refuse_leftover(&self, name: &str) -> Result<(), String> {
        let left = fs::read_dir(self.group_dir(name))
            .map(|d| d.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "signal")).count())
            .unwrap_or(0);
        if left == 0 {
            return Ok(());
        }
        let messages = if left == 1 { "message" } else { "messages" };
        Err(format!("#{name} has {left} {messages} left on disk from an earlier channel; dissolve it first (attend channel dissolve {name})"))
    }

    /// Set or replace a channel's description (#404). Single-line by
    /// contract — the yaml wire format is line-oriented.
    pub fn set_description(&self, name: &str, text: &str) -> Result<(), String> {
        let normalized = normalize_description(text)?;
        let _lock = self.lock_state()?;
        let mut state = self.load_state();
        let Some(entry) = state.get_mut(name) else {
            return Err(format!("#{name}: unknown group"));
        };
        entry.description = normalized;
        self.save_state(&state);
        Ok(())
    }

    /// Move every membership of `old` to `new`, for a member whose id
    /// changed under it (Claude Code's `/clear` gives a session a new id).
    pub fn rename_member(&self, old: &str, new: &str) {
        let Ok(_lock) = self.lock_state() else { return };
        let mut state = self.load_state();
        if rename_in(&mut state, old, new) {
            self.save_state(&state);
        }
        self.move_records(old, new);
    }

    /// Move `old`'s invitations and briefings to `new`: each follows its
    /// member to the new id.
    fn move_records(&self, old: &str, new: &str) {
        for (_, dir) in channel_dirs(&self.base) {
            for kind in [INVITE, BRIEF] {
                let from = dir.join(format!("{old}.{kind}"));
                if from.is_file() {
                    fs::rename(&from, dir.join(format!("{new}.{kind}"))).ok();
                }
            }
        }
    }

    /// The record `<member>.<kind>` in channel `name`: a sibling of the
    /// room's signals, which every scanner skips (they read `.signal` files
    /// only), gone with the room when it is dissolved.
    fn record_path(&self, name: &str, member: &str, kind: &str) -> PathBuf {
        self.group_dir(name).join(format!("{member}.{kind}"))
    }

    /// Write record `kind` for `member` in channel `name`, naming this
    /// manager's member as its writer and now as its time. An existing
    /// record is replaced.
    fn write_record(&self, name: &str, member: &str, kind: &str) -> Result<(), String> {
        validate_group_name(name)?;
        validate_member_id(member)?;
        let dir = self.group_dir(name);
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let mut body = String::new();
        if !self.member_id.is_empty() {
            body += &format!("by: {}\n", self.member_id);
        }
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        body += &format!("at: {}\n", now.as_secs());
        agent_settings::writer::write_atomic(&self.record_path(name, member, kind), body).map_err(|e| e.to_string())
    }

    /// Record that `member` was invited to `name` by this manager's member.
    /// The invitee's own `attend channel join` answers it
    /// ([`Groups::take_invite`]); any join of the invitee clears it. A
    /// second invitation replaces the first.
    pub fn invite(&self, name: &str, member: &str) -> Result<(), String> {
        self.write_record(name, member, INVITE)
    }

    /// Withdraw `member`'s invitation to `name`, for one whose message was
    /// not sent.
    pub fn uninvite(&self, name: &str, member: &str) {
        if validate_group_name(name).is_ok() {
            fs::remove_file(self.record_path(name, member, INVITE)).ok();
        }
    }

    /// Whether `member` has a pending invitation to `name`.
    pub fn is_invited(&self, name: &str, member: &str) -> bool {
        validate_group_name(name).is_ok() && self.record_path(name, member, INVITE).is_file()
    }

    /// The pending invitations to `name`, by invitee.
    pub fn invites(&self, name: &str) -> Vec<Invite> {
        if validate_group_name(name).is_err() {
            return Vec::new();
        }
        let mut out = read_records(&self.group_dir(name), name, INVITE);
        out.sort_by(|a, b| a.member.cmp(&b.member));
        out
    }

    /// Whether this member had a pending invitation to `name`; the
    /// invitation is spent either way.
    pub fn take_invite(&self, name: &str) -> bool {
        validate_group_name(name).is_ok() && fs::remove_file(self.record_path(name, &self.member_id, INVITE)).is_ok()
    }

    /// Enroll `member` in channel `name` on its behalf (ADR-404 D1's add),
    /// and spend any invitation it had there. The channel must exist: an
    /// add makes no channel. Refused for a member already in it.
    ///
    /// `admit` runs under the channels lock once the add is sure to be
    /// written, before the membership is. The caller records there what the
    /// member is owed: its enrollment and its briefing. The sweep takes the
    /// same lock, so it never finds the member without them, and an add
    /// that is refused has written none of them. An `admit` that fails
    /// leaves the membership unwritten.
    pub fn add(&self, name: &str, member: &str, admit: impl FnOnce() -> Result<(), String>) -> Result<(), AddError> {
        validate_group_name(name).map_err(AddError::Failed)?;
        validate_member_id(member).map_err(AddError::Failed)?;
        let _lock = self.lock_state().map_err(AddError::Failed)?;
        let mut state = self.load_state();
        let dir = self.group_dir(name);
        if !state.contains_key(name) && !dir.is_dir() {
            return Err(AddError::UnknownChannel(name.to_string()));
        }
        if state.get(name).is_some_and(|e| e.members.iter().any(|m| m == member)) {
            return Err(AddError::AlreadyMember(name.to_string()));
        }
        fs::create_dir_all(&dir).map_err(|e| AddError::Failed(format!("creating group dir: {e}")))?;
        admit().map_err(AddError::Failed)?;
        state.entry(name.to_string()).or_default().members.push(member.to_string());
        self.save_state(&state);
        fs::remove_file(self.record_path(name, member, INVITE)).ok();
        Ok(())
    }

    /// Record that `member`, added to `name` before its cold start has run,
    /// is owed the room's history from before the add: its cold start
    /// delivers it whatever its age ([`briefings`]). The record stays after
    /// that cold start, until the member leaves or is removed: by then every
    /// message from before the add is in the member's seen-set, delivered or
    /// held back, and the seen-set drops a key only when its message is
    /// gone, so a later cold start finds none of them to deliver.
    pub fn brief(&self, name: &str, member: &str) -> Result<(), String> {
        self.write_record(name, member, BRIEF)
    }

    /// Whether `member` is owed a briefing in `name`.
    pub fn is_briefed(&self, name: &str, member: &str) -> bool {
        validate_group_name(name).is_ok() && self.record_path(name, member, BRIEF).is_file()
    }

    /// Withdraw `member`'s briefing in `name`, for an add that did not take.
    pub fn unbrief(&self, name: &str, member: &str) {
        if validate_group_name(name).is_ok() {
            fs::remove_file(self.record_path(name, member, BRIEF)).ok();
        }
    }

    /// Remove every invitation and briefing whose member has gone
    /// ([`attend_presence::gone`]): it exited on this host, or no session
    /// record here names it and its heartbeat is absent or long stale. An
    /// invitee is not a member, so the membership sweep does not see it,
    /// and Claude Code deletes a session's record when it exits: an invitee
    /// that exits without ever listening leaves neither a record nor a
    /// heartbeat. A session that runs and never listened is still a live
    /// invitee.
    ///
    /// An enrollment record left holding only a process that has exited
    /// goes too, once no invitation or briefing here names its session.
    pub fn prune_records(&self) {
        self.prune_records_with(attend_presence::gone);
        let named = self.recorded_members();
        attend_presence::enrollment::prune_exited(|sid| named.contains(sid));
    }

    /// Every member an invitation or a briefing names, in any channel.
    pub fn recorded_members(&self) -> std::collections::HashSet<String> {
        channel_dirs(&self.base)
            .into_iter()
            .filter_map(|(_, dir)| fs::read_dir(dir).ok())
            .flat_map(|entries| entries.flatten())
            .filter_map(|entry| record_name(&entry.file_name()).map(|(member, _)| member))
            .collect()
    }

    /// [`Groups::prune_records`] with an injectable judgement of which
    /// members have gone.
    pub fn prune_records_with<F: Fn(&str) -> bool>(&self, has_exited: F) {
        for (_, dir) in channel_dirs(&self.base) {
            let Ok(entries) = fs::read_dir(&dir) else { continue };
            for entry in entries.flatten() {
                let Some((member, _)) = record_name(&entry.file_name()) else { continue };
                if has_exited(&member) {
                    fs::remove_file(entry.path()).ok();
                }
            }
        }
    }

    /// Leave a named group.
    pub fn leave(&self, name: &str) -> Result<(), String> {
        let _lock = self.lock_state()?;
        let mut state = self.load_state();
        if let Some(entry) = state.get_mut(name) {
            entry.members.retain(|m| m != &self.member_id);
        }
        fs::remove_file(self.record_path(name, &self.member_id, BRIEF)).ok();
        // Clean up empty unpinned groups in one write
        if state.get(name).is_some_and(|e| e.members.is_empty() && !e.pinned) {
            let dir = self.group_dir(name);
            if dir.is_dir() {
                fs::remove_dir_all(&dir).ok();
            }
            state.remove(name);
        }
        self.save_state(&state);
        Ok(())
    }

    /// Remove an arbitrary member from a group — the operator's
    /// `/kick` (ADR-173 / issue #393). Removal is unilateral by
    /// design: it only *reduces* the target's routing reach, unlike
    /// entry, which requires the member's own `join` (invite-consent
    /// asymmetry). Same empty-group cleanup as [`leave`]. Errors on
    /// an unknown group or a non-member so the TUI can report
    /// precisely.
    pub fn kick(&self, name: &str, member: &str) -> Result<(), String> {
        // #406: removal is unilateral between peers, but an agent may
        // never remove a HUMAN member — the operator power flows one
        // way. Enforced here, at the shared API, so no surface (current
        // or future) can reach around it.
        if is_agent_member(&self.member_id) && !is_agent_member(member) {
            return Err("agents cannot kick a human member".to_string());
        }
        let _lock = self.lock_state()?;
        let mut state = self.load_state();
        let Some(entry) = state.get_mut(name) else {
            return Err(format!("#{name}: unknown group"));
        };
        let before = entry.members.len();
        entry.members.retain(|m| m != member);
        if entry.members.len() == before {
            return Err(format!("{member} is not a member of #{name}"));
        }
        fs::remove_file(self.record_path(name, member, BRIEF)).ok();
        if state.get(name).is_some_and(|e| e.members.is_empty() && !e.pinned) {
            let dir = self.group_dir(name);
            if dir.is_dir() {
                fs::remove_dir_all(&dir).ok();
            }
            state.remove(name);
        }
        self.save_state(&state);
        Ok(())
    }

    /// Pin a group so it persists when empty.
    pub fn pin(&self, name: &str) {
        let Ok(_lock) = self.lock_state() else { return };
        let mut state = self.load_state();
        if let Some(entry) = state.get_mut(name) {
            entry.pinned = true;
            self.save_state(&state);
        }
    }

    /// Unpin a group.
    pub fn unpin(&self, name: &str) {
        let Ok(_lock) = self.lock_state() else { return };
        let mut state = self.load_state();
        if let Some(entry) = state.get_mut(name) {
            entry.pinned = false;
        }
        // Clean up if now empty and unpinned — one write
        if state.get(name).is_some_and(|e| e.members.is_empty() && !e.pinned) {
            let dir = self.group_dir(name);
            if dir.is_dir() {
                fs::remove_dir_all(&dir).ok();
            }
            state.remove(name);
        }
        self.save_state(&state);
    }

    /// Dissolve a group — remove it and notify members.
    pub fn dissolve(&self, name: &str) -> Result<Vec<String>, String> {
        let _lock = self.lock_state()?;
        let mut state = self.load_state();
        let members = state
            .remove(name)
            .map(|e| e.members)
            .unwrap_or_default();
        self.save_state(&state);

        // Remove the signal directory
        let dir = self.group_dir(name);
        if dir.is_dir() {
            fs::remove_dir_all(&dir).ok();
        }
        Ok(members)
    }

    /// List channels this member has joined.
    pub fn my_groups(&self) -> Vec<(String, bool)> {
        let state = self.load_state();
        state
            .iter()
            .filter(|(_, entry)| entry.members.contains(&self.member_id))
            .map(|(name, entry)| (name.clone(), entry.pinned))
            .collect()
    }

    /// List member ids in a named group, or None if the group does not
    /// exist. Returns raw `_groups.yaml` membership — callers that need
    /// a liveness-checked view should filter with
    /// `attend_presence::alive` (see attend's `cmd_send` for the
    /// routing-validation shape).
    pub fn members(&self, name: &str) -> Option<Vec<String>> {
        self.load_state().get(name).map(|e| e.members.clone())
    }

    /// List all active channels with member counts and pin state.
    pub fn all_groups(&self) -> Vec<(String, usize, bool)> {
        let state = self.load_state();
        let mut rooms: Vec<(String, usize, bool)> = state
            .iter()
            .map(|(name, entry)| (name.clone(), entry.members.len(), entry.pinned))
            .collect();
        rooms.sort_by(|a, b| a.0.cmp(&b.0));
        rooms
    }

    /// Get group names this member is focused on (for signal routing).
    pub fn joined_group_names(&self) -> Vec<String> {
        self.my_groups().into_iter().map(|(name, _)| name).collect()
    }

    /// Clean up stale members (sessions or humans whose heartbeat has
    /// gone stale — see [`member_alive`]), then sweep orphan group
    /// dirs.
    ///
    /// The orphan sweep (ADR-170) covers what the member pass can't
    /// see: a `@name/` directory with no `_groups.yaml` entry at all.
    /// Those accumulate when an entry is removed without the dir (or
    /// was never written) and would otherwise render in the chat's
    /// channel bar forever. Only dirs older than the grace window are
    /// candidates. The sweep holds the state lock, as `join` does from
    /// its entry through its directory, so a join never lands between
    /// the sweep's read of the yaml and its removal.
    ///
    /// A reserved name (`open`, `broadcast`) can never be a group, so an
    /// `@open/` or `@broadcast/` dir is an orphan and is swept like one.
    ///
    /// A member dropped from its last channel loses the enrollment an add
    /// gave it (ADR-404 D1). A join's survives the sweep: liveness does not
    /// undo a session's own enrollment (#720).
    ///
    /// A member that is not live but that a live session's record names as
    /// an earlier id is renamed to that session, as the `/clear` move that
    /// carried its enrollment would have done ([`Groups::cleanup_stale_moving`]).
    pub fn cleanup_stale(&self) {
        let is_live = |m: &str| attend_presence::alive(m, &std::collections::HashSet::new());
        for member in self.cleanup_stale_moving(is_live, attend_presence::enrollment::live_successor, attend_presence::heartbeat::DEFAULT_GRACE) {
            self.when_in_no_channel(&member, || {
                attend_presence::enrollment::withdraw(&member, attend_presence::enrollment::Source::Add).ok();
            });
        }
        self.prune_records();
    }

    /// Run `settle` under the channels lock when `member` is in no channel.
    /// An add records its enrollment under the same lock, so none lands
    /// between the check and `settle`. Returns whether it ran, `Some(false)`
    /// for a member in a channel, or `None` when the lock was not taken in
    /// time.
    pub fn when_in_no_channel(&self, member: &str, settle: impl FnOnce()) -> Option<bool> {
        let Ok(_lock) = self.lock_state() else { return None };
        if self.load_state().values().any(|e| e.members.iter().any(|m| m == member)) {
            return Some(false);
        }
        settle();
        Some(true)
    }

    /// [`Groups::cleanup_stale`] with an injectable liveness predicate
    /// and orphan-age grace, so tests can drive both passes without a
    /// heartbeat sidecar or mtime manipulation. Returns the members it
    /// dropped from a channel.
    pub fn cleanup_stale_with<F: Fn(&str) -> bool>(
        &self,
        is_live: F,
        orphan_grace: std::time::Duration,
    ) -> Vec<String> {
        self.cleanup_stale_moving(is_live, |_| None, orphan_grace)
    }

    /// [`Groups::cleanup_stale_with`], renaming a member that is not live
    /// to the id `successor` gives it, with its invitations and briefings,
    /// in place of dropping it: a `/clear` move that stopped after carrying
    /// the enrollment, finished under the lock the sweep holds. In that
    /// crash case it finishes the memberships, invitations and briefs. The
    /// registry slot, the last-inbound and banner state and the old
    /// heartbeat stay under the old id until the session's own next
    /// adoption.
    pub fn cleanup_stale_moving<F: Fn(&str) -> bool, S: Fn(&str) -> Option<String>>(
        &self,
        is_live: F,
        successor: S,
        orphan_grace: std::time::Duration,
    ) -> Vec<String> {
        let Ok(_lock) = self.lock_state() else { return Vec::new() };
        let mut state = self.load_state();
        let mut dropped: Vec<String> = Vec::new();
        let mut moved: Vec<(String, String)> = Vec::new();

        let members: std::collections::BTreeSet<String> = state.values().flat_map(|e| e.members.iter().cloned()).collect();
        for sid in members {
            if is_live(&sid) {
                continue;
            }
            match successor(&sid) {
                Some(new) => moved.push((sid, new)),
                None => dropped.push(sid),
            }
        }
        for (old, new) in &moved {
            rename_in(&mut state, old, new);
        }
        for entry in state.values_mut() {
            entry.members.retain(|sid| !dropped.contains(sid));
        }

        if !dropped.is_empty() || !moved.is_empty() {
            self.save_state(&state);
        }
        for (old, new) in &moved {
            self.move_records(old, new);
        }

        // Clean up empty unpinned channels
        let to_remove: Vec<String> = state
            .iter()
            .filter(|(_, e)| e.members.is_empty() && !e.pinned)
            .map(|(name, _)| name.clone())
            .collect();

        for name in &to_remove {
            let dir = self.group_dir(name);
            if dir.is_dir() {
                fs::remove_dir_all(&dir).ok();
            }
        }

        if !to_remove.is_empty() {
            for name in &to_remove {
                state.remove(name);
            }
            self.save_state(&state);
        }

        // Orphan-dir sweep: on-disk `@name/` dirs the yaml doesn't
        // know about, aged past the grace window.
        let Ok(entries) = fs::read_dir(&self.base) else {
            return dropped;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(bare) = name.strip_prefix(GROUP_PREFIX) else {
                continue;
            };
            if bare.is_empty() || state.contains_key(bare) {
                continue;
            }
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let old_enough = fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| std::time::SystemTime::now().duration_since(t).ok())
                .is_some_and(|age| age > orphan_grace);
            if !old_enough {
                continue;
            }
            fs::remove_dir_all(&path).ok();
        }
        dropped
    }

    // ── State persistence ──────────────────────────────────────

    fn load_state(&self) -> HashMap<String, GroupEntry> {
        load_groups(&self.base)
    }

    /// Hold `_groups.yaml` for one read-modify-write. Every method that
    /// saves the state takes this before it loads, so two writers (attend
    /// sessions, a chat instance, two threads of one process) apply their
    /// edits in turn: without it, each saves the state it loaded, and the
    /// later save drops the earlier one's membership.
    ///
    /// `Ok(None)` when the lock cannot be taken: the lock file cannot be
    /// made, in which case the save cannot land either, or the OS refuses
    /// the lock itself (`ENOLCK` on a network filesystem), in which case
    /// the write goes ahead unlocked and can lose a concurrent writer's
    /// change. Nothing reports that: this crate has no channel for a
    /// warning, and stderr would draw over attend-chat's screen. `Err` only
    /// when [`Groups::waiting_at_most`] set a wait and another writer held
    /// the lock past it.
    fn lock_state(&self) -> Result<Option<Lock>, String> {
        let path = self.state_path();
        let taken = match self.lock_wait {
            None => Lock::acquire(&path).map(Some),
            Some(limit) => Lock::acquire_within(&path, limit),
        };
        match taken {
            Ok(Some(lock)) => Ok(Some(lock)),
            Ok(None) => Err(BUSY.to_string()),
            Err(_) => Ok(None),
        }
    }

    fn save_state(&self, state: &HashMap<String, GroupEntry>) {
        let path = self.state_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        // Atomic, through a temporary file unique to the writer, so a
        // reader never sees a torn file.
        agent_settings::writer::write_atomic(&path, serialize_groups_yaml(state)).ok();
    }
}

// ── Helpers ────────────────────────────────────────────────────

/// Read-only load of the group state under `base`. This is the entry
/// point for consumers that render membership without acting for a
/// member (attend-chat's channel legend and chip glyphs).
pub fn load_groups(base: &Path) -> HashMap<String, GroupEntry> {
    let path = base.join("_groups.yaml");
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };
    parse_groups_yaml(&content)
}

pub fn validate_group_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("group name cannot be empty".to_string());
    }
    if name.starts_with('_') || name.starts_with('@') {
        return Err("group name cannot start with _ or @".to_string());
    }
    if name.contains('/') || name.contains(' ') {
        return Err("group name cannot contain / or spaces".to_string());
    }
    // '#' would survive a single sigil-strip and mint a phantom whose
    // yaml header parses as a comment; ':' is the yaml key terminator.
    // Both are wire hazards, not names (PR #407 review).
    if name.contains('#') || name.contains(':') {
        return Err("group name cannot contain # or :".to_string());
    }
    // `broadcast` backs `_broadcast/`; `open` is the display name of
    // the base channel (ADR-124) — both would alias the commons if
    // we let a user create an explicit group with either name.
    if name == "broadcast" || name == "open" {
        return Err(format!("'{name}' is reserved"));
    }
    Ok(())
}

/// A member id a record can be named for: not empty, and no path.
pub fn validate_member_id(member: &str) -> Result<(), String> {
    if member.is_empty() || member.contains(['/', '\\']) || member.starts_with('.') {
        return Err(format!("'{member}' is not a member id"));
    }
    Ok(())
}

/// Every channel directory under `base`, by channel name.
fn channel_dirs(base: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = fs::read_dir(base) else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.strip_prefix(GROUP_PREFIX)?.to_string();
            (!name.is_empty() && e.path().is_dir()).then(|| (name, e.path()))
        })
        .collect()
}

/// The member and kind a record's file name gives, for an invitation or a
/// briefing; `None` for any other file.
fn record_name(file: &std::ffi::OsStr) -> Option<(String, &'static str)> {
    let file = file.to_str()?;
    [INVITE, BRIEF].into_iter().find_map(|kind| {
        let member = file.strip_suffix(kind)?.strip_suffix('.')?;
        (!member.is_empty()).then(|| (member.to_string(), kind))
    })
}

/// The `by:` and `at:` of a record's body.
fn parse_record(body: &str) -> (Option<String>, Option<std::time::SystemTime>) {
    let (mut by, mut at) = (None, None);
    for line in body.lines() {
        match line.split_once(':').map(|(k, v)| (k.trim(), v.trim())) {
            Some(("by", v)) if !v.is_empty() => by = Some(v.to_string()),
            Some(("at", v)) => {
                at = v.parse::<u64>().ok().map(|s| std::time::UNIX_EPOCH + Duration::from_secs(s));
            }
            _ => {}
        }
    }
    (by, at)
}

/// The records of `kind` in channel directory `dir`.
fn read_records(dir: &Path, channel: &str, kind: &str) -> Vec<Invite> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|e| {
            let (member, k) = record_name(&e.file_name())?;
            if k != kind {
                return None;
            }
            let (by, at) = parse_record(&fs::read_to_string(e.path()).unwrap_or_default());
            Some(Invite { channel: channel.to_string(), member, by, at })
        })
        .collect()
}

/// Every pending invitation under `base`, by channel, each list by invitee.
/// Read-only, for a surface that shows who was invited where.
pub fn load_invites(base: &Path) -> HashMap<String, Vec<Invite>> {
    let mut out = HashMap::new();
    for (name, dir) in channel_dirs(base) {
        let mut invites = read_records(&dir, &name, INVITE);
        if !invites.is_empty() {
            invites.sort_by(|a, b| a.member.cmp(&b.member));
            out.insert(name, invites);
        }
    }
    out
}

/// The briefings `member` is owed, as each channel and the time of the add
/// that owes it ([`Groups::brief`]). The cold start reads them.
pub fn briefings(base: &Path, member: &str) -> Vec<(String, std::time::SystemTime)> {
    let mut out: Vec<(String, std::time::SystemTime)> = channel_dirs(base)
        .into_iter()
        .filter_map(|(name, dir)| {
            let body = fs::read_to_string(dir.join(format!("{member}.{BRIEF}"))).ok()?;
            Some((name, parse_record(&body).1?))
        })
        .collect();
    out.sort();
    out
}

/// Move every membership of `old` in `state` to `new`. Returns whether any
/// moved.
fn rename_in(state: &mut HashMap<String, GroupEntry>, old: &str, new: &str) -> bool {
    let mut changed = false;
    for entry in state.values_mut() {
        if entry.members.iter().any(|m| m == old) {
            entry.members.retain(|m| m != old && m != new);
            entry.members.push(new.to_string());
            changed = true;
        }
    }
    changed
}

/// Whether a member is still alive for cleanup: [`attend_presence::alive`]
/// with no process scan. A fresh heartbeat counts (ADR-129): a claude
/// session's attend touches it on every tick, an enrolled session's drain
/// at every turn end and the attend mod's `listen beat` every 30 seconds;
/// a human's attend-chat touches the username heartbeat on its refresh tick
/// (ADR-170). So does a listening session whose recorded Claude Code process
/// still runs on this host (ADR-129, addendum of 2026-10-13), so a member
/// through a long turn is not pruned. An earlier id that such a session's
/// record names counts too ([`attend_presence::enrollment::live_successor`]):
/// a `/clear`'s move carries the enrollment before it renames the
/// memberships, and the sweep renames such an id rather than dropping it.
///
/// A Claude process that never listened is not consulted: with nothing
/// to deliver to it, for cleanup purposes it is as good as a dead claude.
pub fn member_alive(member_id: &str) -> bool {
    attend_presence::alive(member_id, &std::collections::HashSet::new()) || attend_presence::enrollment::live_successor(member_id).is_some()
}

// ── Minimal YAML parser/serializer ─────────────────────────────
// Keeps the crate dependency-free on serde.

pub fn parse_groups_yaml(content: &str) -> HashMap<String, GroupEntry> {
    let mut rooms = HashMap::new();
    let mut current_room: Option<String> = None;
    let mut current_pinned = false;
    let mut current_members: Vec<String> = Vec::new();
    let mut current_description: Option<String> = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let indent = line.len() - line.trim_start().len();

        // Top-level: group name
        if indent == 0 && trimmed.ends_with(':') {
            // Save previous group
            if let Some(ref name) = current_room {
                rooms.insert(
                    name.clone(),
                    GroupEntry {
                        pinned: current_pinned,
                        members: current_members.clone(),
                        description: current_description.take(),
                    },
                );
            }
            current_room = Some(trimmed.trim_end_matches(':').to_string());
            current_pinned = false;
            current_members = Vec::new();
            current_description = None;
            continue;
        }

        // Second-level: group properties
        if indent == 2 {
            if let Some((key, value)) = trimmed.split_once(':') {
                let key = key.trim();
                let value = value.trim();
                match key {
                    "pinned" => current_pinned = value == "true",
                    "description" if !value.is_empty() => {
                        current_description = Some(value.to_string());
                    }
                    "members" => {} // array header, items follow
                    _ => {}
                }
            }
        }

        // Third-level: member list items
        if indent == 4 {
            if let Some(member) = trimmed.strip_prefix("- ") {
                current_members.push(member.to_string());
            }
        }
    }

    // Save last group
    if let Some(ref name) = current_room {
        rooms.insert(
            name.clone(),
            GroupEntry {
                pinned: current_pinned,
                members: current_members,
                description: current_description,
            },
        );
    }

    rooms
}

pub fn serialize_groups_yaml(state: &HashMap<String, GroupEntry>) -> String {
    let mut out = String::new();
    let mut names: Vec<&String> = state.keys().collect();
    names.sort();

    for name in names {
        let entry = &state[name];
        out.push_str(&format!("{name}:\n"));
        out.push_str(&format!("  pinned: {}\n", entry.pinned));
        if let Some(ref d) = entry.description {
            out.push_str(&format!("  description: {d}\n"));
        }
        out.push_str("  members:\n");
        for member in &entry.members {
            out.push_str(&format!("    - {member}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir_like() -> PathBuf {
        // A counter, not the clock: two tests starting in the same clock tick
        // shared a directory and raced on it.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "attend-groups-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn test_validate_group_name() {
        assert!(validate_group_name("deploy").is_ok());
        assert!(validate_group_name("my-room").is_ok());
        assert!(validate_group_name("").is_err());
        assert!(validate_group_name("_internal").is_err());
        assert!(validate_group_name("@bad").is_err());
        assert!(validate_group_name("has/slash").is_err());
        assert!(validate_group_name("has space").is_err());
        assert!(validate_group_name("broadcast").is_err());
        // ADR-124: `open` is the base channel display name — reserved
        // so nobody can shadow it with a real focus group.
        assert!(validate_group_name("open").is_err());
    }

    #[test]
    fn test_yaml_roundtrip() {
        let mut state = HashMap::new();
        state.insert(
            "deploy".to_string(),
            GroupEntry {
                pinned: true,
                members: vec!["session-abc".to_string(), "session-xyz".to_string()],
                description: None,
            },
        );
        state.insert(
            "collab".to_string(),
            GroupEntry {
                pinned: false,
                members: vec!["session-abc".to_string()],
                description: None,
            },
        );

        let yaml = serialize_groups_yaml(&state);
        let parsed = parse_groups_yaml(&yaml);

        assert_eq!(parsed.len(), 2);
        assert!(parsed["deploy"].pinned);
        assert_eq!(parsed["deploy"].members.len(), 2);
        assert!(!parsed["collab"].pinned);
        assert_eq!(parsed["collab"].members.len(), 1);
    }

    /// The exact byte sequence both former parsers (attend's and
    /// attend-chat's pre-ADR-170 mirror) agreed on. Kept as the wire-
    /// format anchor now that there is a single implementation: if
    /// parsing this string ever produces a different map, the on-disk
    /// format contract has changed and every deployed reader is at
    /// risk.
    const GROUPS_YAML_GOLDEN: &str = concat!(
        "\n",
        "# leading comment\n",
        "deploy:\n",
        "  pinned: true\n",
        "  members:\n",
        "    - sess-a\n",
        "    - sess-b\n",
        "\n",
        "infra:\n",
        "  pinned: false\n",
        "  members: []\n",
        "collab:\n",
        "  pinned: false\n",
        "  members:\n",
        "    - sess-c\n",
    );

    #[test]
    fn golden_parses_stably() {
        let parsed = parse_groups_yaml(GROUPS_YAML_GOLDEN);
        assert_eq!(parsed.len(), 3);
        assert!(parsed["deploy"].pinned);
        assert_eq!(parsed["deploy"].members, vec!["sess-a", "sess-b"]);
        assert!(!parsed["infra"].pinned);
        assert!(parsed["infra"].members.is_empty());
        assert!(!parsed["collab"].pinned);
        assert_eq!(parsed["collab"].members, vec!["sess-c"]);
    }

    #[test]
    fn join_then_leave_roundtrip() {
        let base = tempdir_like();
        let g = Groups::new(&base, "member-a");
        g.join("deploy", false).unwrap();
        assert!(base.join("@deploy").is_dir());
        assert_eq!(g.members("deploy").unwrap(), vec!["member-a"]);

        g.leave("deploy").unwrap();
        // Last member out of an unpinned group dissolves it.
        assert!(g.members("deploy").is_none());
        assert!(!base.join("@deploy").exists());
    }

    #[test]
    fn join_is_idempotent() {
        let base = tempdir_like();
        let g = Groups::new(&base, "member-a");
        g.join("deploy", false).unwrap();
        g.join("deploy", false).unwrap();
        assert_eq!(g.members("deploy").unwrap(), vec!["member-a"]);
    }

    #[test]
    fn leave_keeps_pinned_group() {
        let base = tempdir_like();
        let g = Groups::new(&base, "member-a");
        g.join("deploy", true).unwrap();
        g.leave("deploy").unwrap();
        // Pinned groups persist when empty.
        assert_eq!(g.members("deploy").unwrap(), Vec::<String>::new());
        assert!(base.join("@deploy").is_dir());
    }

    #[test]
    fn mixed_member_kinds_coexist() {
        // ADR-170: a members list can hold a claude session UUID and a
        // human username side by side; neither manager disturbs the
        // other's entry.
        let base = tempdir_like();
        let claude = Groups::new(&base, "2f2632d7-3f59-4c24-9aae-000000000000");
        let human = Groups::new(&base, "aaron");
        claude.join("deploy", false).unwrap();
        human.join("deploy", false).unwrap();
        let members = claude.members("deploy").unwrap();
        assert_eq!(members.len(), 2);
        assert!(members.contains(&"aaron".to_string()));

        human.leave("deploy").unwrap();
        let members = claude.members("deploy").unwrap();
        assert_eq!(members, vec!["2f2632d7-3f59-4c24-9aae-000000000000"]);
    }

    #[test]
    fn load_groups_missing_yaml_is_empty() {
        let base = tempdir_like();
        assert!(load_groups(&base).is_empty());
    }

    #[test]
    fn cleanup_drops_stale_members_and_dissolves_empty_unpinned() {
        let base = tempdir_like();
        let g = Groups::new(&base, "dead-sess");
        g.join("temp", false).unwrap();
        // Everyone is stale → member removed, empty unpinned group
        // dissolved, dir gone.
        g.cleanup_stale_with(|_| false, attend_presence::heartbeat::DEFAULT_GRACE);
        assert!(g.members("temp").is_none());
        assert!(!base.join("@temp").exists());
    }

    #[test]
    fn cleanup_keeps_live_members() {
        let base = tempdir_like();
        let g = Groups::new(&base, "live-sess");
        g.join("deploy", false).unwrap();
        g.cleanup_stale_with(|_| true, attend_presence::heartbeat::DEFAULT_GRACE);
        assert_eq!(g.members("deploy").unwrap(), vec!["live-sess"]);
        assert!(base.join("@deploy").is_dir());
    }

    #[test]
    fn cleanup_sweeps_aged_orphan_dir() {
        let base = tempdir_like();
        fs::create_dir_all(base.join("@orphan")).unwrap();
        // Zero grace: any positive age qualifies — the dir was
        // created a moment ago, which is already "older than zero".
        std::thread::sleep(std::time::Duration::from_millis(5));
        Groups::new(&base, "x").cleanup_stale_with(|_| true, std::time::Duration::ZERO);
        assert!(!base.join("@orphan").exists());
    }

    #[test]
    fn cleanup_spares_fresh_orphan_dir() {
        // A just-created dir (e.g. a join mid-flight) survives the
        // sweep under the real grace window.
        let base = tempdir_like();
        fs::create_dir_all(base.join("@fresh")).unwrap();
        Groups::new(&base, "x").cleanup_stale_with(|_| true, attend_presence::heartbeat::DEFAULT_GRACE);
        assert!(base.join("@fresh").is_dir());
    }

    #[test]
    fn cleanup_spares_yaml_listed_dir_regardless_of_age() {
        let base = tempdir_like();
        let g = Groups::new(&base, "live-sess");
        g.join("deploy", false).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        g.cleanup_stale_with(|_| true, std::time::Duration::ZERO);
        assert!(base.join("@deploy").is_dir());
    }

    #[test]
    fn rename_member_moves_every_membership() {
        let base = tempdir_like();
        Groups::new(&base, "old").join("a", false).unwrap();
        Groups::new(&base, "old").join("b", false).unwrap();
        Groups::new(&base, "other").join("b", false).unwrap();
        Groups::new(&base, "x").rename_member("old", "new");
        assert_eq!(Groups::new(&base, "new").joined_group_names().len(), 2);
        assert!(Groups::new(&base, "old").joined_group_names().is_empty());
        assert_eq!(Groups::new(&base, "x").members("b").unwrap().len(), 2);
    }

    #[test]
    fn concurrent_joins_and_sweeps_lose_nothing() {
        // #955, #960: each write loads, edits and saves the whole file;
        // unlocked, a write saving what it loaded before another's save
        // drops that one. A sweep runs beside the joiners, as attend's
        // cleanup does beside attend-chat's joins.
        let base = tempdir_like();
        let joining = std::sync::atomic::AtomicBool::new(true);
        std::thread::scope(|s| {
            s.spawn(|| {
                while joining.load(std::sync::atomic::Ordering::Relaxed) {
                    Groups::new(&base, "x").cleanup_stale_with(|_| true, Duration::ZERO);
                }
            });
            let joiners: Vec<_> = (0..8)
                .map(|t| {
                    let base = &base;
                    s.spawn(move || {
                        for i in 0..20 {
                            Groups::new(base, &format!("m{t}")).join(&format!("c{t}-{i}"), false).unwrap();
                        }
                    })
                })
                .collect();
            for j in joiners {
                j.join().unwrap();
            }
            joining.store(false, std::sync::atomic::Ordering::Relaxed);
        });
        let state = load_groups(&base);
        assert_eq!(state.len(), 160, "every join's channel is in the file");
        let swept: Vec<_> = state.keys().filter(|n| !base.join(format!("@{n}")).is_dir()).collect();
        assert!(swept.is_empty(), "the sweep removed joined channels' dirs: {swept:?}");
    }

    #[test]
    fn a_bounded_write_is_refused_while_another_holds_the_lock() {
        let base = tempdir_like();
        let held = Lock::acquire(&base.join("_groups.yaml")).unwrap();
        let g = Groups::new(&base, "m").waiting_at_most(Duration::from_millis(30));
        assert_eq!(g.join("deploy", false), Err(BUSY.to_string()));
        assert!(load_groups(&base).is_empty(), "the refused join wrote nothing");
        drop(held);
        assert_eq!(g.join("deploy", false), Ok(()));
    }

    #[test]
    fn cleanup_sweeps_a_reserved_name_dir_as_an_orphan() {
        // `open` and `broadcast` are reserved, so no group of that name
        // can exist: an `@open/` dir is an orphan like any other. No
        // migration reads it any more (ADR-506).
        let base = tempdir_like();
        fs::create_dir_all(base.join("@open")).unwrap();
        fs::write(base.join("@open").join("a.signal"), "from|p|/x|hi\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        Groups::new(&base, "x").cleanup_stale_with(|_| true, std::time::Duration::ZERO);
        assert!(!base.join("@open").exists());
    }
}

#[cfg(test)]
mod kick_tests {
    use super::*;

    fn base(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("attend-groups-kick-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn kick_removes_target_and_keeps_others() {
        let b = base("basic");
        Groups::new(&b, "agent-1").join("deploy", false).unwrap();
        Groups::new(&b, "agent-2").join("deploy", false).unwrap();

        let op = Groups::new(&b, "operator");
        op.kick("deploy", "agent-1").unwrap();

        let members = op.members("deploy").expect("group survives");
        assert!(!members.contains(&"agent-1".to_string()));
        assert!(members.contains(&"agent-2".to_string()));
        fs::remove_dir_all(&b).ok();
    }

    #[test]
    fn kick_reports_unknown_group_and_non_member() {
        let b = base("errors");
        Groups::new(&b, "agent-1").join("deploy", false).unwrap();
        let op = Groups::new(&b, "operator");
        assert!(op.kick("ghost", "agent-1").unwrap_err().contains("unknown group"));
        assert!(op.kick("deploy", "stranger").unwrap_err().contains("not a member"));
        fs::remove_dir_all(&b).ok();
    }

    #[test]
    fn kicking_last_member_cleans_up_unpinned_group() {
        let b = base("cleanup");
        Groups::new(&b, "agent-1").join("temp", false).unwrap();
        let op = Groups::new(&b, "operator");
        op.kick("temp", "agent-1").unwrap();
        assert!(op.members("temp").is_none(), "empty unpinned group is removed");
        assert!(!b.join("@temp").exists());
        fs::remove_dir_all(&b).ok();
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    fn base(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "attend-groups-404-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// #404 golden extension: description round-trips through the yaml
    /// wire format, and pre-#404 files (no description key) parse to
    /// `None` untouched.
    #[test]
    fn description_round_trips_and_absent_is_none() {
        let mut state = HashMap::new();
        state.insert(
            "deploy".to_string(),
            GroupEntry {
                pinned: true,
                members: vec!["m1".into()],
                description: Some("release coordination".into()),
            },
        );
        let yaml = serialize_groups_yaml(&state);
        assert!(yaml.contains("  description: release coordination\n"), "got: {yaml}");
        let back = parse_groups_yaml(&yaml);
        assert_eq!(back["deploy"].description.as_deref(), Some("release coordination"));

        let legacy = "old:\n  pinned: false\n  members:\n    - m1\n";
        let parsed = parse_groups_yaml(legacy);
        assert_eq!(parsed["old"].description, None);
        assert_eq!(parsed["old"].members, vec!["m1".to_string()]);
    }

    #[test]
    fn create_is_pinned_empty_and_refuses_duplicates() {
        let b = base("create");
        let g = Groups::new(&b, "operator");
        g.create("plans", Some("  roadmap talk  ")).unwrap();
        let state = load_groups(&b);
        assert!(state["plans"].pinned, "explicit create must survive empty");
        assert!(state["plans"].members.is_empty());
        assert_eq!(state["plans"].description.as_deref(), Some("roadmap talk"));
        assert!(b.join("@plans").is_dir());
        assert!(g.create("plans", None).unwrap_err().contains("already exists"));
        assert!(g.create("open", None).is_err(), "reserved names stay reserved");
        fs::remove_dir_all(&b).ok();
    }

    /// A name whose `@name/` an earlier channel left behind, its messages
    /// still in it and no entry naming it, is refused by create and by a
    /// join that would make the channel: a new channel there would open on
    /// the old one's messages. Dissolve takes the leftover
    /// away, entry or none, and the name is free again. A leftover holding
    /// no message is no refusal.
    #[test]
    fn create_refuses_a_leftover_directory_holding_messages() {
        let b = base("leftover");
        let g = Groups::new(&b, "operator");
        let dir = b.join("@old");
        fs::create_dir_all(&dir).unwrap();
        for i in 0..12 {
            fs::write(dir.join(format!("m{i}.signal")), "x").unwrap();
        }
        fs::write(dir.join("alpha.invite"), "by: x\n").unwrap();
        assert_eq!(g.create("old", None).unwrap_err(), "#old has 12 messages left on disk from an earlier channel; dissolve it first (attend channel dissolve old)");
        assert_eq!(g.join("old", false).unwrap_err(), "#old has 12 messages left on disk from an earlier channel; dissolve it first (attend channel dissolve old)", "a join would make the channel too");
        assert!(!load_groups(&b).contains_key("old"), "nothing recorded");
        // A channel with an entry is joined whatever its folder holds.
        g.join("live", false).unwrap();
        fs::write(b.join("@live/m.signal"), "x").unwrap();
        Groups::new(&b, "second").join("live", false).unwrap();
        fs::create_dir_all(b.join("@one")).unwrap();
        fs::write(b.join("@one/m.signal"), "x").unwrap();
        assert_eq!(g.create("one", None).unwrap_err(), "#one has 1 message left on disk from an earlier channel; dissolve it first (attend channel dissolve one)");
        assert_eq!(g.dissolve("old").unwrap(), Vec::<String>::new(), "no entry, no members");
        assert!(!dir.exists(), "the leftover is gone");
        g.create("old", None).unwrap();
        fs::create_dir_all(b.join("@bare")).unwrap();
        g.create("bare", None).unwrap();
        fs::remove_dir_all(&b).ok();
    }

    #[test]
    fn set_description_updates_clears_and_rejects_multiline() {
        let b = base("desc");
        let g = Groups::new(&b, "operator");
        g.create("x", None).unwrap();
        g.set_description("x", "first").unwrap();
        assert_eq!(load_groups(&b)["x"].description.as_deref(), Some("first"));
        g.set_description("x", "  ").unwrap();
        assert_eq!(load_groups(&b)["x"].description, None);
        assert!(g.set_description("x", "a\nb").unwrap_err().contains("single line"));
        assert!(g.set_description("ghost", "d").unwrap_err().contains("unknown group"));
        fs::remove_dir_all(&b).ok();
    }

    /// #406: the operator power flows one way — an agent-acting Groups
    /// may not remove a human member; humans may remove anyone; agents
    /// may still remove agents (peer coordination).
    #[test]
    fn kick_guard_blocks_agent_removing_human() {
        let b = base("guard");
        let human = "aaron";
        let agent = "a62473ce-6310-421e-9964-7bb3c9a752d4";
        Groups::new(&b, human).join("ops", false).unwrap();
        Groups::new(&b, agent).join("ops", false).unwrap();

        let acting_agent = Groups::new(&b, agent);
        assert!(
            acting_agent.kick("ops", human).unwrap_err().contains("cannot kick a human"),
            "agent must not remove a human member"
        );
        // pid-fallback identities count as agents too — the guard must
        // not be dodgeable by running unresolved.
        assert!(
            Groups::new(&b, "pid-4242").kick("ops", human).is_err(),
            "unresolved agent must not remove a human member"
        );
        // Agent → agent stays allowed; human → agent stays allowed.
        assert!(acting_agent.kick("ops", agent).is_ok());
        Groups::new(&b, agent).join("ops", false).unwrap();
        assert!(Groups::new(&b, human).kick("ops", agent).is_ok());
        fs::remove_dir_all(&b).ok();
    }

    #[test]
    fn member_kind_discriminator_shapes() {
        assert!(is_agent_member("a62473ce-6310-421e-9964-7bb3c9a752d4"));
        assert!(is_agent_member("pid-1234"));
        assert!(!is_agent_member("aaron"));
        assert!(!is_agent_member("aaron-b"));
        // Close-but-wrong UUID shapes are not agents.
        assert!(!is_agent_member("a62473ce-6310-421e-9964"));
        assert!(!is_agent_member("zzzzzzzz-6310-421e-9964-7bb3c9a752d4"));
    }
}

#[cfg(test)]
mod injection_tests {
    use super::*;

    fn base(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("attend-groups-407-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// PR #407 blocking regression: a newline-bearing description must
    /// be rejected by EVERY write path — the reviewer reproduced a
    /// yaml injection minting a fabricated pinned group with an
    /// arbitrary member through `create`.
    #[test]
    fn create_and_describe_reject_yaml_injection() {
        let b = base("inject");
        let g = Groups::new(&b, "operator");
        let evil = "x\nevil:\n  pinned: true\n  members:\n    - fake-member";
        assert!(g.create("foo", Some(evil)).unwrap_err().contains("single line"));
        assert!(load_groups(&b).is_empty(), "nothing may be written on rejection");

        g.create("foo", None).unwrap();
        assert!(g.set_description("foo", evil).unwrap_err().contains("single line"));
        let state = load_groups(&b);
        assert!(!state.contains_key("evil"), "no fabricated group");
        assert_eq!(state["foo"].description, None);
        fs::remove_dir_all(&b).ok();
    }

    #[test]
    fn group_names_reject_wire_hazard_characters() {
        assert!(validate_group_name("#x").unwrap_err().contains("# or :"));
        assert!(validate_group_name("a:b").unwrap_err().contains("# or :"));
        assert!(validate_group_name("plain-name").is_ok());
    }
}

#[cfg(test)]
mod invite_tests {
    use super::*;

    #[test]
    fn an_invitation_is_taken_once_by_its_invitee() {
        let base = std::env::temp_dir().join(format!("attend-groups-invite-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        Groups::new(&base, "").invite("topic", "sess-a").unwrap();
        assert!(!Groups::new(&base, "sess-b").take_invite("topic"), "not b's");
        assert!(Groups::new(&base, "sess-a").take_invite("topic"));
        assert!(!Groups::new(&base, "sess-a").take_invite("topic"), "spent");
        assert!(Groups::new(&base, "").invite("open", "sess-a").is_err(), "a reserved name");
        assert!(Groups::new(&base, "").invite("topic", "../x").is_err());
        let _ = fs::remove_dir_all(&base);
    }
}

/// A test directory under the build's target directory, removed with its
/// contents when dropped, so test scratch stays out of the system temp dir.
#[cfg(test)]
pub(crate) mod scratch {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub(crate) struct Scratch(PathBuf);

    impl std::ops::Deref for Scratch {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A fresh directory named for `tag`. The test binary runs from
    /// `<target>/<profile>/deps/`, so its grandparent is the profile dir.
    pub(crate) fn dir(tag: &str) -> Scratch {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let exe = std::env::current_exe().unwrap();
        let profile = exe.parent().and_then(Path::parent).unwrap();
        let p = profile.join("test-scratch").join(format!(
            "attend-groups-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Scratch(p)
    }
}

#[cfg(test)]
mod record_tests {
    use super::*;
    use std::time::SystemTime;

    #[test]
    fn an_invitation_records_its_inviter_and_time() {
        let base = scratch::dir("invite-record");
        Groups::new(&base, "aaron").invite("topic", "sess-a").unwrap();
        let invites = Groups::new(&base, "x").invites("topic");
        assert_eq!(invites.len(), 1);
        let i = &invites[0];
        assert_eq!((i.channel.as_str(), i.member.as_str(), i.by.as_deref()), ("topic", "sess-a", Some("aaron")));
        let age = SystemTime::now().duration_since(i.at.expect("a time")).unwrap_or_default();
        assert!(age < Duration::from_secs(5), "written just now: {age:?}");
        assert_eq!(load_invites(&base)["topic"], invites, "the read-only listing agrees");
        assert!(Groups::new(&base, "x").is_invited("topic", "sess-a"));
        assert!(!Groups::new(&base, "x").is_invited("topic", "sess-b"));
    }

    #[test]
    fn an_empty_invitation_file_reads_with_no_inviter_or_time() {
        let base = scratch::dir("invite-legacy");
        fs::create_dir_all(base.join("@topic")).unwrap();
        fs::write(base.join("@topic").join("sess-a.invite"), b"").unwrap();
        let invites = Groups::new(&base, "x").invites("topic");
        assert_eq!(invites, vec![Invite { channel: "topic".into(), member: "sess-a".into(), by: None, at: None }]);
    }

    #[test]
    fn inviting_again_rewrites_the_one_record() {
        let base = scratch::dir("invite-again");
        Groups::new(&base, "aaron").invite("topic", "sess-a").unwrap();
        Groups::new(&base, "bob").invite("topic", "sess-a").unwrap();
        let invites = Groups::new(&base, "x").invites("topic");
        assert_eq!(invites.len(), 1);
        assert_eq!(invites[0].by.as_deref(), Some("bob"));
    }

    #[test]
    fn a_join_by_any_path_clears_the_joiners_invitation() {
        let base = scratch::dir("invite-join");
        Groups::new(&base, "aaron").invite("topic", "sess-a").unwrap();
        Groups::new(&base, "aaron").invite("topic", "sess-b").unwrap();
        // `Groups::join` is what a scene activation and the chat's /join
        // call, without `take_invite`.
        Groups::new(&base, "sess-a").join("topic", false).unwrap();
        let left: Vec<String> = Groups::new(&base, "x").invites("topic").into_iter().map(|i| i.member).collect();
        assert_eq!(left, vec!["sess-b"]);
    }

    #[test]
    fn the_sweep_drops_the_records_of_sessions_that_exited() {
        let base = scratch::dir("invite-prune");
        let g = Groups::new(&base, "aaron");
        g.create("topic", None).unwrap();
        g.invite("topic", "sess-gone").unwrap();
        g.invite("topic", "sess-here").unwrap();
        g.brief("topic", "sess-gone").unwrap();
        g.prune_records_with(|m| m == "sess-gone");
        let left: Vec<String> = g.invites("topic").into_iter().map(|i| i.member).collect();
        assert_eq!(left, vec!["sess-here"]);
        assert!(briefings(&base, "sess-gone").is_empty());
        g.create("other", None).unwrap();
        g.brief("other", "sess-briefed").unwrap();
        // What keeps an exited session's enrollment record from the prune.
        let mut named: Vec<String> = g.recorded_members().into_iter().collect();
        named.sort();
        assert_eq!(named, vec!["sess-briefed", "sess-here"]);
    }

    #[test]
    fn a_new_session_id_carries_its_invitations_and_briefings() {
        let base = scratch::dir("invite-rename");
        let g = Groups::new(&base, "aaron");
        g.create("topic", None).unwrap();
        g.create("other", None).unwrap();
        g.invite("topic", "old").unwrap();
        g.brief("other", "old").unwrap();
        g.rename_member("old", "new");
        let invites = g.invites("topic");
        assert_eq!(invites.len(), 1);
        assert_eq!((invites[0].member.as_str(), invites[0].by.as_deref()), ("new", Some("aaron")));
        assert_eq!(briefings(&base, "new").into_iter().map(|(c, _)| c).collect::<Vec<_>>(), vec!["other"]);
        assert!(briefings(&base, "old").is_empty());
    }

    #[test]
    fn an_add_enrolls_the_member_and_spends_its_invitation() {
        let base = scratch::dir("add");
        let g = Groups::new(&base, "aaron");
        g.create("topic", None).unwrap();
        g.invite("topic", "sess-a").unwrap();
        g.add("topic", "sess-a", || Ok(())).unwrap();
        assert_eq!(g.members("topic").unwrap(), vec!["sess-a"]);
        assert!(g.invites("topic").is_empty());
        assert_eq!(g.add("topic", "sess-a", || Ok(())), Err(AddError::AlreadyMember("topic".into())));
    }

    /// The add's own step runs only for an add that will be written, and
    /// one that fails writes no membership.
    #[test]
    fn an_adds_admit_step_runs_only_when_it_takes() {
        let base = scratch::dir("add-admit");
        let g = Groups::new(&base, "aaron");
        g.create("topic", None).unwrap();
        let ran = std::cell::Cell::new(0);
        let admit = || {
            ran.set(ran.get() + 1);
            Ok(())
        };
        assert_eq!(g.add("ghost", "sess-a", admit), Err(AddError::UnknownChannel("ghost".into())));
        assert_eq!(ran.get(), 0, "a refused add admits no one");
        let failed = g.add("topic", "sess-a", || Err("enrolling failed".into()));
        assert_eq!(failed, Err(AddError::Failed("enrolling failed".into())));
        assert!(g.members("topic").unwrap().is_empty(), "a failed admit writes no membership");
        g.add("topic", "sess-a", admit).unwrap();
        assert_eq!(ran.get(), 1);
        assert_eq!(g.add("topic", "sess-a", admit), Err(AddError::AlreadyMember("topic".into())));
        assert_eq!(ran.get(), 1, "a member already in is not admitted again");
    }

    #[test]
    fn an_add_refuses_the_base_channel_and_unknown_ones() {
        let base = scratch::dir("add-refuse");
        let g = Groups::new(&base, "aaron");
        assert!(g.add("open", "sess-a", || Ok(())).is_err());
        assert_eq!(g.add("ghost", "sess-a", || Ok(())), Err(AddError::UnknownChannel("ghost".into())));
        assert!(g.add("ghost", "../x", || Ok(())).is_err());
        assert!(load_groups(&base).is_empty(), "a refused add writes nothing");
        assert!(!base.join("@ghost").exists());
    }

    #[test]
    fn leaving_drops_the_members_briefing() {
        let base = scratch::dir("brief-leave");
        let g = Groups::new(&base, "aaron");
        g.create("topic", None).unwrap();
        g.add("topic", "sess-a", || Ok(())).unwrap();
        g.brief("topic", "sess-a").unwrap();
        assert_eq!(briefings(&base, "sess-a").len(), 1);
        Groups::new(&base, "sess-a").leave("topic").unwrap();
        assert!(briefings(&base, "sess-a").is_empty());
    }
}
