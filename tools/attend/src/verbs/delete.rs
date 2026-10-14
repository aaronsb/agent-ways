//! `delete`: mark the messages no live recipient still has to read
//! (ADR-404 D3e).
//!
//! A message is deletable when it is in the requested scope, older than the
//! requested cutoff, older than [`attend_presence::heartbeat::DEFAULT_GRACE`],
//! and pending for no live recipient. A delete writes a mark for each
//! deletable message (`attend_groups::tombstones`) and removes no file;
//! every reader skips a marked message, and `attend message cleanup`
//! removes it later.
//!
//! A live recipient is a session `attend_presence::alive` counts live that
//! is enrolled or keeps a heartbeat. What it holds is the drain's own scan,
//! `scan_pending(receive_dirs, seen, reading_ids, baselining)`, the count
//! `attend message unread` reports for it. A person in attend-chat keeps no
//! seen-set and holds nothing.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use attend_groups::tombstones::{self, Marks};
use attend_groups::{Groups, ReceiveDir, Room};
use serde::Serialize;

use crate::util::signals_base;

/// Which messages a delete looks at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// `#open`.
    Open,
    /// One named channel; `open` names `#open`.
    Channel(String),
    /// The direct-message thread between the caller and this session: what
    /// it sent the caller and what the caller sent it, in any tray.
    Dm(String),
    /// Every room: `#open`, each channel, each project tray and person tray.
    Everywhere,
}

/// How old a message must be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OlderThan {
    /// No cutoff of its own: the rule decides.
    All,
    /// Older than the earliest start of any live session.
    Current,
    /// Sent before this instant: local midnight today, a local date's
    /// midnight, or now less a duration.
    Before(SystemTime),
}

impl OlderThan {
    /// Read `current`, `today`, `all`, a duration such as `90m`, `2h30m`,
    /// `3d` or `1w`, or a local date `YYYY-MM-DD`, at `now`.
    pub fn parse(text: &str, now: SystemTime) -> Option<Self> {
        match text.trim() {
            "all" => Some(OlderThan::All),
            "current" => Some(OlderThan::Current),
            "today" => Some(OlderThan::Before(agent_fmt::when::local_day_start(now))),
            t => {
                if let Some(date) = parse_date(t) {
                    return Some(OlderThan::Before(date));
                }
                parse_duration(t).and_then(|d| now.checked_sub(d)).map(OlderThan::Before)
            }
        }
    }
}

/// `YYYY-MM-DD` to its local midnight.
fn parse_date(t: &str) -> Option<SystemTime> {
    let mut parts = t.split('-');
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return None;
    }
    agent_fmt::when::local_date_start(y.parse().ok()?, m.parse().ok()?, d.parse().ok()?)
}

/// A run of `<n><unit>` pairs, the units `s`, `m`, `h`, `d` and `w`.
fn parse_duration(t: &str) -> Option<Duration> {
    let mut total: u64 = 0;
    let mut digits = String::new();
    for c in t.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        let unit = match c {
            's' => 1,
            'm' => 60,
            'h' => 3_600,
            'd' => 86_400,
            'w' => 604_800,
            _ => return None,
        };
        let n: u64 = digits.parse().ok()?;
        total = total.checked_add(n.checked_mul(unit)?)?;
        digits.clear();
    }
    (digits.is_empty() && total > 0).then(|| Duration::from_secs(total))
}

/// One delete.
#[derive(Debug, Clone)]
pub struct DeleteRequest {
    pub scope: Scope,
    /// Only messages this session or person sent.
    pub from: Option<String>,
    pub older_than: OlderThan,
    /// Report what would be marked and mark nothing.
    pub dry_run: bool,
}

/// What a delete did.
#[derive(Debug, Clone, Serialize)]
pub struct Deleted {
    /// The scope, as `#open`, `#<channel>`, `dm <session>` or `everywhere`.
    pub scope: String,
    pub dry_run: bool,
    /// How many messages it marked, or would mark on a dry run.
    pub marked: usize,
    /// The batch its marks belong to, which `--undo` names. `None` on a dry
    /// run or when nothing was marked.
    pub batch: Option<String>,
    /// The marked messages' ids.
    pub ids: Vec<String>,
    /// Each live recipient that held a message the rest of the rule allowed.
    pub held: Vec<Held>,
}

/// One live recipient's share of what a delete kept.
#[derive(Debug, Clone, Serialize)]
pub struct Held {
    pub session: String,
    pub name: String,
    /// The messages it still has to read that the delete kept.
    pub kept: usize,
}

/// What `--undo` took back.
#[derive(Debug, Clone, Serialize)]
pub struct Undone {
    pub batch: String,
    /// How many marks it removed.
    pub unmarked: usize,
}

/// Why a delete marked nothing.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeleteError {
    /// No channel has this name.
    NoChannel { name: String },
    /// `--undo` found no such batch.
    NoBatch { batch: Option<String> },
    /// The mark store could not be written.
    Write { error: String },
}

impl std::fmt::Display for DeleteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeleteError::NoChannel { name } => write!(f, "no channel named #{name}"),
            DeleteError::NoBatch { batch: Some(b) } => write!(f, "no delete batch {b} to undo"),
            DeleteError::NoBatch { batch: None } => write!(f, "no delete to undo"),
            DeleteError::Write { error } => write!(f, "could not write the marks: {error}"),
        }
    }
}

/// Mark what `req` makes deletable.
pub fn delete(req: &DeleteRequest) -> Result<Deleted, DeleteError> {
    let base = signals_base();
    let recipients = live_recipients(&base);
    delete_in(&base, req, &caller_ids(), &recipients, SystemTime::now())
}

/// The ids the caller reads as: a Claude Code session's id and the ids it
/// had before each `/clear`, or outside a session the member id of the
/// person at the terminal.
pub(crate) fn caller_ids() -> Vec<String> {
    let me = crate::util::current_session();
    if me.resolved() {
        attend_presence::enrollment::reading_ids(&me)
    } else {
        vec![agent_identity::sanitize_id_component(&agent_identity::signal::user_name())]
    }
}

/// Which messages in the scope's rooms a delete may take, by sender and
/// addressee.
struct Filter<'a> {
    from: Option<&'a str>,
    /// The other party of a `--dm` thread, and the caller's ids.
    thread: Option<(&'a str, &'a [String])>,
}

impl Filter<'_> {
    fn is_open(&self) -> bool {
        self.from.is_none() && self.thread.is_none()
    }

    /// Whether the signal at `path` passes. One that cannot be read or
    /// parsed names no sender and passes no filter.
    fn admits(&self, path: &Path) -> bool {
        if self.is_open() {
            return true;
        }
        let Ok(content) = agent_identity::signal::read_signal_file(path) else { return false };
        let Some(sig) = agent_identity::parse_signal(agent_identity::wire_line(&content)) else { return false };
        let sender = sig.sender_id();
        if self.from.is_some_and(|f| f != sender) {
            return false;
        }
        match self.thread {
            None => true,
            Some((other, caller)) => {
                let to_caller = caller.iter().any(|c| sig.addressed_to(c));
                let by_caller = caller.contains(&sender);
                (sender == other && to_caller) || (by_caller && sig.addressed_to(other))
            }
        }
    }
}

/// Remove the marks of batch `batch`, or of the newest batch.
pub fn undo(batch: Option<&str>) -> Result<Undone, DeleteError> {
    match tombstones::undo(&signals_base(), batch) {
        Ok(Some(u)) => Ok(Undone { batch: u.batch, unmarked: u.unmarked.len() }),
        Ok(None) => Err(DeleteError::NoBatch { batch: batch.map(str::to_string) }),
        Err(e) => Err(DeleteError::Write { error: e.to_string() }),
    }
}

/// A live recipient: its id, when its session started, and the seen-set
/// keys of what it has pending.
pub(crate) struct Recipient {
    pub(crate) id: String,
    pub(crate) started: Option<SystemTime>,
    pub(crate) pending: HashSet<String>,
}

/// Every live agent session, and whether it is enrolled. Candidates are
/// the sessions with a heartbeat or an enrollment record; a heartbeat with
/// no record counts only for an agent's id, since a person in attend-chat
/// heartbeats under their member id.
pub(crate) fn live_sessions() -> Vec<(String, bool)> {
    use attend_presence::enrollment;
    let enrolled: HashSet<String> =
        enrollment::recorded_ids().into_iter().filter(|id| !enrollment::sources(id).is_empty()).collect();
    let beating = std::fs::read_dir(attend_presence::heartbeat::heartbeat_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|id| !id.starts_with('.') && !id.ends_with(".lock") && attend_groups::is_agent_member(id));
    let mut ids: Vec<String> = enrolled.iter().cloned().chain(beating).collect::<HashSet<_>>().into_iter().collect();
    ids.sort();
    ids.into_iter()
        .filter(|id| attend_presence::alive(id, &HashSet::new()))
        .map(|id| {
            let listening = enrolled.contains(&id);
            (id, listening)
        })
        .collect()
}

/// The ids live session `id` reads as: its own and those it had before
/// each `/clear`.
pub(crate) fn reading_ids_of(id: &str) -> Vec<String> {
    attend_presence::enrollment::reading_ids(&identity_of(id, attend_presence::session::origin_path(id)))
}

/// A resolved identity for another session.
fn identity_of(id: &str, origin: Option<String>) -> attend_presence::session::SessionIdentity {
    attend_presence::session::SessionIdentity {
        session_id: id.to_string(),
        origin_path: origin.unwrap_or_default(),
        session_resolved: true,
        origin_resolved: true,
        claude_pid: None,
    }
}

/// Every live recipient, with what it holds.
pub(crate) fn live_recipients(base: &Path) -> Vec<Recipient> {
    let marks = Marks::load(base);
    live_sessions()
        .into_iter()
        .map(|(id, enrolled)| {
            let pending = pending_for(base, &id, enrolled, &marks);
            let started = attend_presence::session::started_at(&id)
                .or_else(|| attend_presence::enrollment::listening_since(&id));
            Recipient { id, started, pending }
        })
        .collect()
}

/// What session `id` has pending: the drain's scan over its receive set,
/// against its seen-set, as its reading ids, with its cold start while it
/// is not listening or not yet baselined, as `attend message unread`
/// counts it. A session whose project no record names reads every project
/// tray, so the delete keeps whatever it might be owed there.
fn pending_for(base: &Path, id: &str, enrolled: bool, marks: &Marks) -> HashSet<String> {
    let origin = attend_presence::session::origin_path(id);
    let groups = Groups::new(base, id);
    let dirs: Vec<ReceiveDir> = match &origin {
        Some(origin) => groups.receive_dirs(origin),
        None => project_trays(base)
            .into_iter()
            .map(|path| ReceiveDir { path, room: Room::Project })
            .chain(groups.receive_dirs("").into_iter().filter(|d| d.room != Room::Project))
            .collect(),
    };
    let snapshot = attend_state::StateStore::new(Some(id.to_string())).load();
    let baselining = !enrolled || !snapshot.as_ref().is_some_and(|s| s.baselined);
    let seen = snapshot.map(|s| s.seen_signals).unwrap_or_default();
    let ids = attend_presence::enrollment::reading_ids(&identity_of(id, origin));
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    crate::cmd::inbox::scan_pending(&dirs, &seen, &ids, baselining, marks).delivered.into_iter().map(|d| d.key).collect()
}

/// The subdirectories of `dir`, with their names.
fn subdirs(dir: &Path) -> Vec<(PathBuf, String)> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| Some((e.path(), e.file_name().to_str()?.to_string())))
        .collect()
}

/// Every project tray under `base`.
fn project_trays(base: &Path) -> Vec<PathBuf> {
    subdirs(base).into_iter().filter(|(_, n)| !n.starts_with('_') && !n.starts_with('@')).map(|(p, _)| p).collect()
}

/// The person trays under `base`.
fn person_trays(base: &Path) -> Vec<PathBuf> {
    subdirs(&base.join(attend_groups::PEOPLE_DIR)).into_iter().map(|(p, _)| p).collect()
}

/// The directories `scope` covers, and its label.
fn scope_dirs(base: &Path, scope: &Scope) -> Result<(Vec<PathBuf>, String), DeleteError> {
    let open = || base.join(attend_groups::BROADCAST_DIR);
    Ok(match scope {
        Scope::Open => (vec![open()], "#open".to_string()),
        Scope::Channel(name) => {
            let name = name.trim_start_matches('#');
            if name == attend_groups::BASE_CHANNEL {
                return Ok((vec![open()], "#open".to_string()));
            }
            let groups = Groups::new(base, "");
            let dir = groups.group_dir(name);
            if attend_groups::validate_group_name(name).is_err()
                || (!dir.is_dir() && !attend_groups::load_groups(base).contains_key(name))
            {
                return Err(DeleteError::NoChannel { name: name.to_string() });
            }
            (vec![dir], format!("#{name}"))
        }
        Scope::Dm(other) => {
            let mut dirs = project_trays(base);
            dirs.extend(person_trays(base));
            (dirs, format!("dm {other}"))
        }
        Scope::Everywhere => {
            let mut dirs = vec![open()];
            dirs.extend(subdirs(base).into_iter().filter(|(_, n)| n.starts_with('@')).map(|(p, _)| p));
            dirs.extend(project_trays(base));
            dirs.extend(person_trays(base));
            (dirs, "everywhere".to_string())
        }
    })
}

/// [`delete`] against signals base `base`, for the caller reading as
/// `caller`, with the live recipients given, at `now`.
pub(crate) fn delete_in(
    base: &Path,
    req: &DeleteRequest,
    caller: &[String],
    recipients: &[Recipient],
    now: SystemTime,
) -> Result<Deleted, DeleteError> {
    let (dirs, label) = scope_dirs(base, &req.scope)?;
    let cutoff = match req.older_than {
        OlderThan::All => None,
        OlderThan::Before(t) => Some(t),
        OlderThan::Current => Some(recipients.iter().filter_map(|r| r.started).min().unwrap_or(now)),
    };
    let filter = Filter {
        from: req.from.as_deref(),
        thread: match &req.scope {
            Scope::Dm(other) => Some((other.as_str(), caller)),
            _ => None,
        },
    };
    let marks = Marks::load(base);
    let mut kept = vec![0usize; recipients.len()];
    let mut keys: Vec<String> = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_string) else { continue };
            if !name.ends_with(".signal") {
                continue;
            }
            let key = attend_state::seen_key(&name);
            if marks.hides(&key) {
                continue;
            }
            let Ok(mtime) = entry.metadata().and_then(|m| m.modified()) else { continue };
            let age = now.duration_since(mtime).unwrap_or_default();
            if age <= attend_presence::heartbeat::DEFAULT_GRACE
                || cutoff.is_some_and(|c| mtime >= c)
                || !filter.admits(&entry.path())
            {
                continue;
            }
            let mut held = false;
            for (i, r) in recipients.iter().enumerate() {
                if r.pending.contains(&key) {
                    kept[i] += 1;
                    held = true;
                }
            }
            if !held {
                keys.push(key);
            }
        }
    }
    keys.sort();
    keys.dedup();
    let (batch, marked) = if req.dry_run || keys.is_empty() {
        (None, keys)
    } else {
        match tombstones::mark_messages(base, &keys) {
            Ok(Some((batch, marked))) => (Some(batch), marked),
            Ok(None) => (None, Vec::new()),
            Err(e) => return Err(DeleteError::Write { error: e.to_string() }),
        }
    };
    let held = recipients
        .iter()
        .zip(kept)
        .filter(|(_, n)| *n > 0)
        .map(|(r, kept)| Held { session: r.id.clone(), name: display_name(&r.id), kept })
        .collect();
    Ok(Deleted {
        scope: label,
        dry_run: req.dry_run,
        marked: marked.len(),
        batch,
        ids: marked.iter().map(|k| k.strip_suffix(".signal").unwrap_or(k).to_string()).collect(),
        held,
    })
}

/// The name peers see session `id` by, or its id when no record names its
/// project.
fn display_name(id: &str) -> String {
    match attend_presence::session::origin_path(id) {
        Some(origin) => crate::util::display_name(&identity_of(id, Some(origin))),
        None => id.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_784_732_645;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn older_than_reads_each_form() {
        let now = at(NOW);
        assert_eq!(OlderThan::parse("all", now), Some(OlderThan::All));
        assert_eq!(OlderThan::parse("current", now), Some(OlderThan::Current));
        assert_eq!(OlderThan::parse("2h30m", now), Some(OlderThan::Before(at(NOW - 9_000))));
        assert_eq!(OlderThan::parse("3d", now), Some(OlderThan::Before(at(NOW - 3 * 86_400))));
        assert_eq!(OlderThan::parse("1w", now), Some(OlderThan::Before(at(NOW - 604_800))));
        assert!(matches!(OlderThan::parse("today", now), Some(OlderThan::Before(t)) if t <= now && now.duration_since(t).unwrap() < Duration::from_secs(86_400)));
        assert_eq!(
            OlderThan::parse("2026-07-22", now),
            agent_fmt::when::local_date_start(2026, 7, 22).map(OlderThan::Before)
        );
        for bad in ["", "soon", "2h30", "5x", "2026-7-22", "2026-02-30", "0m"] {
            assert_eq!(OlderThan::parse(bad, now), None, "{bad}");
        }
    }
}
