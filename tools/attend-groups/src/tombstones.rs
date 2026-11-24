// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The mark store under `signals/_tombstones/` (ADR-404 D3e).
//!
//! A delete marks a message instead of removing its file, and every reader
//! skips what is marked. `attend message cleanup` removes a marked message's
//! file, and its mark, once the mark has aged past its limits.
//!
//! Layout:
//! - `<session-id>`: a session tombstone, a file whose mtime is when it
//!   was written. Every reader skips what that session sent up to then;
//!   what it sends later, back under `--resume`, shows. `attend message
//!   tombstone` writes it for a session that has exited, and `--undo`
//!   removes it. A tombstone a batch wrote holds the batch's id; one a
//!   single tombstone wrote is empty.
//! - `messages/<key>`: one mark per message. The key is the message's
//!   seen-set key, its filename, which is unique across rooms. The file's
//!   mtime is the mark time and its content the id of the batch that wrote
//!   it. A message already marked keeps its first mark.
//! - `batches/<batch-id>`: the keys one delete marked, one per line, so
//!   `attend message delete --undo` can take that delete back. Batch ids
//!   are the mark time in milliseconds, zero-padded, then the writer's pid,
//!   so the newest batch sorts last.
//! - `session-batches/<batch-id>`: the sessions one `attend message
//!   tombstone --ended` tombstoned, one per line, so `--undo` can take that
//!   batch back. The ids have the form of the delete batches' ids. A line
//!   holds the session id alone when the batch wrote a new tombstone. When
//!   the batch moved a tombstone already there, the line also holds the
//!   earlier cut in milliseconds and the batch that owned the earlier
//!   tombstone, or `-` for none. An undo restores only the tombstones that
//!   still hold its batch's id, so a tombstone of the same session written
//!   after the batch stands.
//!
//! The directory name starts with `_`, so the project-liveness sweep treats
//! it as a shared room and reads only `.signal` files in it, of which it
//! holds none.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// The mark store's directory under the signals base.
pub const TOMBSTONES_DIR: &str = "_tombstones";

const MESSAGES: &str = "messages";
const BATCHES: &str = "batches";
const SESSION_BATCHES: &str = "session-batches";

/// The mark store under signals base `base`.
pub fn dir(base: &Path) -> PathBuf {
    base.join(TOMBSTONES_DIR)
}

fn messages_dir(base: &Path) -> PathBuf {
    dir(base).join(MESSAGES)
}

fn batches_dir(base: &Path) -> PathBuf {
    dir(base).join(BATCHES)
}

fn session_batches_dir(base: &Path) -> PathBuf {
    dir(base).join(SESSION_BATCHES)
}

/// A new batch id: the time in milliseconds, zero-padded, then this
/// process's id, so the newest batch sorts last.
fn new_batch_id() -> String {
    let millis = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    format!("{millis:015}-{}", std::process::id())
}

/// The names of the files directly in `dir`, without dotfiles.
fn file_names(dir: &Path) -> impl Iterator<Item = String> {
    fs::read_dir(dir).into_iter().flatten().flatten().filter_map(|e| {
        let name = e.file_name().to_str()?.to_string();
        (!name.starts_with('.') && e.file_type().ok()?.is_file()).then_some(name)
    })
}

/// What the readers skip, read once per scan.
#[derive(Debug, Default, Clone)]
pub struct Marks {
    messages: HashSet<String>,
    /// Each tombstoned session, with when it was tombstoned.
    sessions: HashMap<String, SystemTime>,
}

impl Marks {
    /// The marks under signals base `base`. Empty when there is no store.
    pub fn load(base: &Path) -> Self {
        let sessions = file_names(&dir(base))
            .filter_map(|id| Some((id.clone(), fs::metadata(dir(base).join(&id)).and_then(|m| m.modified()).ok()?)))
            .collect();
        Marks { messages: file_names(&messages_dir(base)).collect(), sessions }
    }

    /// Whether a reader skips the message with seen-set key `key`.
    pub fn hides(&self, key: &str) -> bool {
        self.messages.contains(key)
    }

    /// Whether a reader skips a message `sender` sent at `sent`: the
    /// sender's canonical id is tombstoned, and the message is no newer
    /// than the tombstone.
    pub fn hides_sender(&self, sender: &str, sent: SystemTime) -> bool {
        self.sessions.get(sender).is_some_and(|at| sent <= *at)
    }
}

/// Whether `id` can be tombstoned: an agent's session id
/// ([`crate::is_agent_member`]). A person's member id cannot be, so no
/// delete of an agent's can silence the operator.
pub fn valid_session_id(id: &str) -> bool {
    crate::is_agent_member(id)
}

/// Tombstone session `id` as of now. A tombstone already there moves to
/// now, so the cut takes in what the session sent since, and no batch owns
/// it any more: a batch's undo leaves it. Returns whether one was there.
pub fn mark_session(base: &Path, id: &str) -> io::Result<bool> {
    if !valid_session_id(id) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("`{id}` is not a session id")));
    }
    fs::create_dir_all(dir(base))?;
    let path = dir(base).join(id);
    let existed = path.is_file();
    let file = fs::OpenOptions::new().write(true).create(true).truncate(true).open(&path)?;
    file.set_modified(SystemTime::now())?;
    Ok(existed)
}

/// What a session's tombstone held before a batch moved it: its cut, and
/// the batch that owned it, if one did.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Prior {
    at: SystemTime,
    owner: Option<String>,
}

impl Prior {
    /// The tombstone of session `id` as it stands, if there is one.
    fn of(base: &Path, id: &str) -> Option<Prior> {
        let at = fs::metadata(dir(base).join(id)).and_then(|m| m.modified()).ok()?;
        Some(Prior { at, owner: session_owner(base, id) })
    }

    /// The record line's tail: the cut in milliseconds, then the owner or
    /// `-`.
    fn line(&self) -> String {
        let millis = self.at.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        format!("{millis} {}", self.owner.as_deref().unwrap_or("-"))
    }

    /// A record line's tail, as [`Prior::line`] writes it.
    fn parse(millis: &str, owner: Option<&str>) -> Option<Prior> {
        let at = SystemTime::UNIX_EPOCH + Duration::from_millis(millis.parse().ok()?);
        Some(Prior { at, owner: owner.filter(|o| *o != "-" && valid_batch_id(o)).map(str::to_string) })
    }
}

/// The batch whose id session `id`'s tombstone holds, if a batch wrote it.
fn session_owner(base: &Path, id: &str) -> Option<String> {
    fs::read_to_string(dir(base).join(id)).ok().map(|b| b.trim().to_string()).filter(|b| !b.is_empty())
}

/// Put session `id`'s tombstone back as `prior` says it was, or remove it
/// when there was none.
fn restore_session(base: &Path, id: &str, prior: Option<&Prior>) -> io::Result<()> {
    let path = dir(base).join(id);
    match prior {
        None => match fs::remove_file(&path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        },
        Some(p) => write_session(&path, p.owner.as_deref().unwrap_or_default(), p.at),
    }
}

/// Write the tombstone at `path` holding `owner`, its cut at `at`.
fn write_session(path: &Path, owner: &str, at: SystemTime) -> io::Result<()> {
    fs::write(path, owner)?;
    fs::File::options().write(true).open(path)?.set_modified(at)
}

/// Tombstone sessions `ids` as of now, as one batch. An id that is not an
/// agent's session id is left out. Returns the batch id and the sessions
/// it tombstoned; with none it writes nothing and returns `None`. The
/// batch's record is written before its tombstones. When a tombstone
/// cannot be written, the ones written before it are put back as they were
/// and the record is removed before the error returns, so no record lists
/// a tombstone that was never there.
pub fn mark_sessions(base: &Path, ids: &[String]) -> io::Result<Option<(String, Vec<String>)>> {
    let mut ids: Vec<String> = ids.iter().filter(|id| valid_session_id(id)).cloned().collect();
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        return Ok(None);
    }
    let batch = new_batch_id();
    let priors: Vec<Option<Prior>> = ids.iter().map(|id| Prior::of(base, id)).collect();
    let record: String = ids
        .iter()
        .zip(&priors)
        .map(|(id, prior)| match prior {
            Some(p) => format!("{id} {}\n", p.line()),
            None => format!("{id}\n"),
        })
        .collect();
    let record_path = session_batches_dir(base).join(&batch);
    agent_settings::writer::write_atomic(&record_path, record)?;
    let now = SystemTime::now();
    for (n, id) in ids.iter().enumerate() {
        if let Err(e) = write_session(&dir(base).join(id), &batch, now) {
            for (done, prior) in ids.iter().zip(&priors).take(n + 1) {
                restore_session(base, done, prior.as_ref()).ok();
            }
            fs::remove_file(&record_path).ok();
            return Err(e);
        }
    }
    Ok(Some((batch, ids)))
}

/// The session batches on record, oldest first.
pub fn session_batches(base: &Path) -> Vec<String> {
    let mut ids: Vec<String> = file_names(&session_batches_dir(base)).filter(|n| valid_batch_id(n)).collect();
    ids.sort();
    ids
}

/// What [`undo_sessions`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionUndo {
    Undone {
        batch: String,
        /// The sessions whose tombstones it removed, or moved back to the
        /// cut they had before the batch.
        restored: Vec<String>,
        /// The sessions it left as they are: tombstoned again since the
        /// batch, or no longer tombstoned.
        kept: Vec<String>,
    },
    /// No batch by that id, or none at all.
    NoBatch,
}

/// Take back session batch `batch`, or the newest when `None`. Each
/// tombstone that still holds the batch's id is removed, or moved back to
/// the cut and the owner it had before the batch, and the batch's record
/// is removed. A tombstone written since, by a single tombstone or another
/// batch, stays.
pub fn undo_sessions(base: &Path, batch: Option<&str>) -> io::Result<SessionUndo> {
    let batch = match batch {
        Some(b) if valid_batch_id(b) => b.to_string(),
        Some(_) => return Ok(SessionUndo::NoBatch),
        None => match session_batches(base).pop() {
            Some(b) => b,
            None => return Ok(SessionUndo::NoBatch),
        },
    };
    let record_path = session_batches_dir(base).join(&batch);
    let record = match fs::read_to_string(&record_path) {
        Ok(r) => r,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(SessionUndo::NoBatch),
        Err(e) => return Err(e),
    };
    let (mut restored, mut kept) = (Vec::new(), Vec::new());
    for line in record.lines() {
        let mut parts = line.split_whitespace();
        let Some(id) = parts.next().filter(|id| valid_session_id(id)) else { continue };
        let prior = parts.next().and_then(|millis| Prior::parse(millis, parts.next()));
        if session_owner(base, id).as_deref() == Some(batch.as_str()) {
            restore_session(base, id, prior.as_ref())?;
            restored.push(id.to_string());
        } else {
            kept.push(id.to_string());
        }
    }
    fs::remove_file(&record_path)?;
    Ok(SessionUndo::Undone { batch, restored, kept })
}

/// Remove session `id`'s tombstone. Returns whether there was one.
pub fn unmark_session(base: &Path, id: &str) -> io::Result<bool> {
    if !valid_session_id(id) {
        return Ok(false);
    }
    match fs::remove_file(dir(base).join(id)) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// The tombstoned sessions, sorted.
pub fn sessions(base: &Path) -> Vec<String> {
    let mut ids: Vec<String> = file_names(&dir(base)).collect();
    ids.sort();
    ids
}

/// Whether `key` can name a mark file: a plain filename.
fn valid_key(key: &str) -> bool {
    !key.is_empty() && !key.starts_with(['.', '#']) && !key.contains(['/', '\\', '\0', ' '])
}

/// Whether `id` is in the form [`mark_messages`] gives a batch.
pub fn valid_batch_id(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit() || b == b'-')
}

/// When batch `id` was written, from the milliseconds that lead it.
pub fn batch_time(id: &str) -> Option<SystemTime> {
    let millis: u64 = id.split('-').next()?.parse().ok()?;
    Some(SystemTime::UNIX_EPOCH + Duration::from_millis(millis))
}

/// How long a batch record stays as a stub once cleanup has removed every
/// message it marked, so `--undo` can say what became of it.
pub const STUB_KEPT: Duration = Duration::from_secs(30 * 86_400);

/// The first line of a stub: `reaped <unix seconds>`.
const STUB_PREFIX: &str = "reaped ";

/// When cleanup removed the last message of the batch whose record is
/// `list`, if the record is a stub.
fn stub_time(list: &str) -> Option<SystemTime> {
    let secs: u64 = list.lines().next()?.strip_prefix(STUB_PREFIX)?.trim().parse().ok()?;
    Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
}

/// Mark the messages with seen-set keys `keys`, as one batch. Keys already
/// marked keep their mark and are left out of the batch. Returns the batch
/// id and the keys it marked; with nothing new to mark it writes nothing
/// and returns `None`. The batch's list is written before its marks, so an
/// interrupted write leaves no mark that `--undo` cannot find. Each mark
/// is written whole under a temporary name and linked into place, so a
/// reader never finds a mark without its batch id; a new staged file
/// serves each [`LINKS_PER_STAGE`] marks, under any filesystem's cap on
/// links to one file. When a mark cannot be written, the marks already
/// linked and the batch record are removed before the error returns, so
/// no record lists marks that were never there.
pub fn mark_messages(base: &Path, keys: &[String]) -> io::Result<Option<(String, Vec<String>)>> {
    let marks = messages_dir(base);
    let mut fresh: Vec<String> = keys.iter().filter(|k| valid_key(k) && !marks.join(k).exists()).cloned().collect();
    fresh.sort();
    fresh.dedup();
    if fresh.is_empty() {
        return Ok(None);
    }
    let batch = new_batch_id();
    fs::create_dir_all(&marks)?;
    let list: String = fresh.iter().map(|k| format!("{k}\n")).collect();
    agent_settings::writer::write_atomic(&batches_dir(base).join(&batch), list)?;
    let mut marked = Vec::new();
    let linked = (|| {
        for (n, chunk) in fresh.chunks(LINKS_PER_STAGE).enumerate() {
            let staged = marks.join(format!(".{batch}.{n}.tmp"));
            fs::write(&staged, &batch)?;
            let mut result = Ok(());
            for key in chunk {
                match fs::hard_link(&staged, marks.join(key)) {
                    Ok(()) => marked.push(key.clone()),
                    // Another delete marked it in between: its mark stands.
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(e) => {
                        result = Err(e);
                        break;
                    }
                }
            }
            fs::remove_file(&staged).ok();
            result?;
        }
        Ok(())
    })();
    if let Err(e) = linked {
        for key in &marked {
            fs::remove_file(marks.join(key)).ok();
        }
        fs::remove_file(batches_dir(base).join(&batch)).ok();
        return Err(e);
    }
    Ok(Some((batch, marked)))
}

/// How many marks share one staged file before a new one is written.
const LINKS_PER_STAGE: usize = 1_000;

/// Every message mark, with its mark time.
pub fn message_marks(base: &Path) -> HashMap<String, SystemTime> {
    let marks = messages_dir(base);
    file_names(&marks)
        .filter_map(|key| {
            let at = fs::metadata(marks.join(&key)).and_then(|m| m.modified()).ok()?;
            Some((key, at))
        })
        .collect()
}

/// The batch that marked the message with seen-set key `key`, if one did.
pub fn batch_of(base: &Path, key: &str) -> Option<String> {
    if !valid_key(key) {
        return None;
    }
    fs::read_to_string(messages_dir(base).join(key)).ok().map(|b| b.trim().to_string())
}

/// Whether the mark for `key` belongs to `batch`.
fn owns(base: &Path, batch: &str, key: &str) -> bool {
    fs::read_to_string(messages_dir(base).join(key)).is_ok_and(|owner| owner.trim() == batch)
}

/// Remove the marks of messages not in `present`, the keys of every signal
/// file on disk. A batch record left with no mark of its own becomes a stub
/// that says when, kept for [`STUB_KEPT`]; a record younger than
/// `attend_presence::heartbeat::DEFAULT_GRACE` is left alone, since its
/// marks may still be being written. Returns how many marks it removed.
pub fn prune(base: &Path, present: &HashSet<String>) -> usize {
    let marks = messages_dir(base);
    let mut removed = 0;
    for key in file_names(&marks) {
        if !present.contains(&key) && fs::remove_file(marks.join(&key)).is_ok() {
            removed += 1;
        }
    }
    let now = SystemTime::now();
    for batch in batches(base) {
        let record = batches_dir(base).join(&batch);
        let young = fs::metadata(&record)
            .and_then(|m| m.modified())
            .is_ok_and(|at| now.duration_since(at).unwrap_or_default() < attend_presence::heartbeat::DEFAULT_GRACE);
        let Ok(list) = fs::read_to_string(&record) else { continue };
        if young {
            continue;
        }
        if let Some(at) = stub_time(&list) {
            if now.duration_since(at).unwrap_or_default() >= STUB_KEPT {
                fs::remove_file(&record).ok();
            }
            continue;
        }
        if !list.lines().filter(|k| valid_key(k)).any(|k| owns(base, &batch, k)) {
            let secs = now.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            agent_settings::writer::write_atomic(&record, format!("{STUB_PREFIX}{secs}\n{list}")).ok();
        }
    }
    removed
}

/// The batches on record, stubs included, oldest first.
pub fn batches(base: &Path) -> Vec<String> {
    let mut ids: Vec<String> = file_names(&batches_dir(base)).filter(|n| valid_batch_id(n)).collect();
    ids.sort();
    ids
}

/// What [`undo`] took back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Undone {
    pub batch: String,
    /// The keys whose marks it removed.
    pub unmarked: Vec<String>,
    /// The keys whose messages cleanup had already removed.
    pub already_removed: usize,
}

/// What an undo found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Undo {
    Undone(Undone),
    /// Cleanup removed every message the batch marked, at this time.
    Reaped { batch: String, at: SystemTime },
    /// No batch by that id, or none at all.
    NoBatch,
}

/// Remove the marks batch `batch` wrote, or the newest batch's when
/// `None`, and the batch's record. A mark another batch wrote stays. With
/// no batch named and the newest record a stub, nothing is undone: an
/// older delete is taken back only when it is named.
pub fn undo(base: &Path, batch: Option<&str>) -> io::Result<Undo> {
    let batch = match batch {
        Some(b) if valid_batch_id(b) => b.to_string(),
        Some(_) => return Ok(Undo::NoBatch),
        None => match batches(base).pop() {
            Some(b) => b,
            None => return Ok(Undo::NoBatch),
        },
    };
    let record = batches_dir(base).join(&batch);
    let list = match fs::read_to_string(&record) {
        Ok(list) => list,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Undo::NoBatch),
        Err(e) => return Err(e),
    };
    if let Some(at) = stub_time(&list) {
        return Ok(Undo::Reaped { batch, at });
    }
    let mut unmarked = Vec::new();
    let mut already_removed = 0;
    for key in list.lines().filter(|k| valid_key(k)) {
        let mark = messages_dir(base).join(key);
        if owns(base, &batch, key) {
            if fs::remove_file(&mark).is_ok() {
                unmarked.push(key.to_string());
            }
        } else if !mark.exists() {
            already_removed += 1;
        }
    }
    fs::remove_file(&record)?;
    Ok(Undo::Undone(Undone { batch, unmarked, already_removed }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("attend-marks-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn keys(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    fn undone(u: Undo) -> Undone {
        match u {
            Undo::Undone(u) => u,
            other => panic!("expected an undo, got {other:?}"),
        }
    }

    /// Date batch `batch`'s record `ago` back.
    fn age_record(b: &Path, batch: &str, ago: Duration) {
        let f = fs::File::options().write(true).open(batches_dir(b).join(batch)).unwrap();
        f.set_modified(SystemTime::now() - ago).unwrap();
    }

    const OLD: Duration = Duration::from_secs(3_600);

    #[test]
    fn a_batch_marks_what_is_unmarked_and_undo_takes_back_only_its_own() {
        let b = base("undo");
        let (first, marked) = mark_messages(&b, &keys(&["a.signal", "b.signal"])).unwrap().unwrap();
        assert_eq!(marked, keys(&["a.signal", "b.signal"]));
        std::thread::sleep(std::time::Duration::from_millis(2));
        let (second, marked) = mark_messages(&b, &keys(&["b.signal", "c.signal"])).unwrap().unwrap();
        assert_eq!(marked, keys(&["c.signal"]), "b keeps its first mark");
        assert_eq!(batches(&b), vec![first.clone(), second.clone()]);

        let u = undone(undo(&b, None).unwrap());
        assert_eq!(u, Undone { batch: second, unmarked: keys(&["c.signal"]), already_removed: 0 });
        let marks = Marks::load(&b);
        assert!(marks.hides("a.signal") && marks.hides("b.signal") && !marks.hides("c.signal"));

        assert_eq!(undone(undo(&b, Some(&first)).unwrap()).unmarked, keys(&["a.signal", "b.signal"]));
        assert!(!Marks::load(&b).hides("b.signal"));
        assert_eq!(undo(&b, None).unwrap(), Undo::NoBatch, "nothing left to undo");
        fs::remove_dir_all(&b).ok();
    }

    /// Two deletes racing for one message: the batch that lost the race
    /// lists the key, and its undo leaves the winner's mark.
    #[test]
    fn undo_leaves_a_mark_another_batch_won() {
        let b = base("race");
        let (winner, _) = mark_messages(&b, &keys(&["x.signal"])).unwrap().unwrap();
        fs::write(batches_dir(&b).join("000000000000001-1"), "x.signal\n").unwrap();
        let lost = undone(undo(&b, Some("000000000000001-1")).unwrap());
        assert_eq!((lost.unmarked, lost.already_removed), (Vec::<String>::new(), 0));
        assert!(Marks::load(&b).hides("x.signal"));
        assert_eq!(undone(undo(&b, Some(&winner)).unwrap()).unmarked, keys(&["x.signal"]));
        fs::remove_dir_all(&b).ok();
    }

    #[test]
    fn a_session_mark_sits_beside_the_message_marks() {
        let b = base("session");
        mark_messages(&b, &keys(&["a.signal"])).unwrap();
        let dead = "22222222-2222-4222-8222-222222222222";
        assert!(!mark_session(&b, dead).unwrap());
        // Dated an hour back, then tombstoned again: the cut moves to now.
        let path = dir(&b).join(dead);
        fs::File::options().write(true).open(&path).unwrap().set_modified(SystemTime::now() - Duration::from_secs(3_600)).unwrap();
        let ten_minutes_ago = SystemTime::now() - Duration::from_secs(600);
        assert!(!Marks::load(&b).hides_sender(dead, ten_minutes_ago));
        assert!(mark_session(&b, dead).unwrap());
        assert!(Marks::load(&b).hides_sender(dead, ten_minutes_ago), "the cut moved to now");
        for bad in ["messages", "../x", "aaron"] {
            assert!(mark_session(&b, bad).is_err(), "{bad}");
        }
        assert_eq!(sessions(&b), keys(&[dead]));
        let marks = Marks::load(&b);
        let before = SystemTime::now() - std::time::Duration::from_secs(60);
        let after = SystemTime::now() + std::time::Duration::from_secs(60);
        assert!(marks.hides_sender(dead, before) && !marks.hides_sender(dead, after), "the tombstone's time cuts");
        assert!(!marks.hides_sender("a.signal", before) && marks.hides("a.signal"));
        assert!(unmark_session(&b, dead).unwrap());
        assert!(!unmark_session(&b, dead).unwrap());
        assert!(!Marks::load(&b).hides_sender(dead, before));
        fs::remove_dir_all(&b).ok();
    }

    /// Cleanup removed every message of one batch and one of another's: the
    /// first becomes a stub that says so, the second undoes in part.
    #[test]
    fn prune_drops_the_marks_of_messages_gone_and_stubs_their_batches() {
        let b = base("prune");
        let (gone, _) = mark_messages(&b, &keys(&["gone.signal"])).unwrap().unwrap();
        std::thread::sleep(Duration::from_millis(2));
        let (half, _) = mark_messages(&b, &keys(&["here.signal", "also-gone.signal"])).unwrap().unwrap();
        age_record(&b, &gone, OLD);
        age_record(&b, &half, OLD);
        let present: HashSet<String> = keys(&["here.signal", "unmarked.signal"]).into_iter().collect();
        assert_eq!(prune(&b, &present), 2);
        assert_eq!(message_marks(&b).into_keys().collect::<Vec<_>>(), keys(&["here.signal"]));
        assert_eq!(batches(&b), vec![gone.clone(), half.clone()], "the reaped batch stays as a stub");

        assert!(matches!(undo(&b, Some(&gone)).unwrap(), Undo::Reaped { .. }));
        let partial = undone(undo(&b, None).unwrap());
        assert_eq!((partial.unmarked, partial.already_removed), (keys(&["here.signal"]), 1));
        assert!(matches!(undo(&b, None).unwrap(), Undo::Reaped { .. }), "the newest left is the stub: refused");
        fs::remove_dir_all(&b).ok();
    }

    /// A record younger than the grace window may still be writing its marks.
    #[test]
    fn prune_leaves_a_young_batch_alone() {
        let b = base("young");
        let (batch, _) = mark_messages(&b, &keys(&["a.signal"])).unwrap().unwrap();
        fs::remove_file(messages_dir(&b).join("a.signal")).unwrap();
        prune(&b, &HashSet::new());
        assert_eq!(fs::read_to_string(batches_dir(&b).join(&batch)).unwrap(), "a.signal\n", "no stub yet");
        fs::remove_dir_all(&b).ok();
    }

    #[test]
    fn a_stub_goes_after_its_keeping_time() {
        let b = base("stub");
        fs::create_dir_all(batches_dir(&b)).unwrap();
        let old = SystemTime::now() - STUB_KEPT - OLD;
        let secs = old.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();
        fs::write(batches_dir(&b).join("000000000000001-1"), format!("reaped {secs}\na.signal\n")).unwrap();
        age_record(&b, "000000000000001-1", OLD);
        prune(&b, &HashSet::new());
        assert!(batches(&b).is_empty());
        fs::remove_dir_all(&b).ok();
    }

    /// A mark that cannot be written takes back the marks before it and the
    /// batch record, so no undo reports a reap that never happened.
    #[test]
    fn a_failed_mark_leaves_no_batch_behind() {
        let b = base("fail");
        let too_long = format!("{}.signal", "x".repeat(300));
        assert!(mark_messages(&b, &[String::from("a.signal"), too_long]).is_err());
        assert!(message_marks(&b).is_empty(), "the mark already linked is taken back");
        assert!(batches(&b).is_empty(), "and the batch record");
        fs::remove_dir_all(&b).ok();
    }

    /// Past the stage size the marks are links to more than one file.
    #[cfg(unix)]
    #[test]
    fn many_marks_spread_over_staged_files() {
        use std::os::unix::fs::MetadataExt;
        let b = base("many");
        let many: Vec<String> = (0..LINKS_PER_STAGE + 1).map(|i| format!("m-{i:05}.signal")).collect();
        let (batch, marked) = mark_messages(&b, &many).unwrap().unwrap();
        assert_eq!(marked.len(), many.len());
        let inode = |k: &str| fs::metadata(messages_dir(&b).join(k)).unwrap().ino();
        assert_ne!(inode(&many[0]), inode(&many[LINKS_PER_STAGE]));
        assert_eq!(fs::read_to_string(messages_dir(&b).join(&many[LINKS_PER_STAGE])).unwrap(), batch);
        fs::remove_dir_all(&b).ok();
    }

    #[test]
    fn a_mark_holds_its_batch_and_no_staging_file_stays() {
        let b = base("staged");
        let (batch, _) = mark_messages(&b, &keys(&["a.signal", "b.signal"])).unwrap().unwrap();
        assert_eq!(fs::read_to_string(messages_dir(&b).join("b.signal")).unwrap(), batch);
        let names: Vec<_> = fs::read_dir(messages_dir(&b)).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(names.len(), 2, "{names:?}");
        fs::remove_dir_all(&b).ok();
    }

    #[test]
    fn nothing_new_writes_no_batch() {
        let b = base("empty");
        assert_eq!(mark_messages(&b, &[]).unwrap(), None);
        assert_eq!(mark_messages(&b, &keys(&["../x", ".hidden"])).unwrap(), None);
        assert!(batches(&b).is_empty());
        fs::remove_dir_all(&b).ok();
    }

    const S1: &str = "11111111-1111-4111-8111-111111111111";
    const S2: &str = "22222222-2222-4222-8222-222222222222";
    const S3: &str = "33333333-3333-4333-8333-333333333333";
    const S4: &str = "44444444-4444-4444-8444-444444444444";

    /// Date session `id`'s tombstone `ago` back.
    fn age_session(b: &Path, id: &str, ago: Duration) {
        fs::File::options().write(true).open(dir(b).join(id)).unwrap().set_modified(SystemTime::now() - ago).unwrap();
    }

    fn restored(u: SessionUndo) -> (Vec<String>, Vec<String>) {
        match u {
            SessionUndo::Undone { restored, kept, .. } => (restored, kept),
            other => panic!("expected an undo, got {other:?}"),
        }
    }

    /// A batch tombstones its sessions, moving one already there; a later
    /// single tombstone of one of them, and one of another session, stand
    /// through the batch's undo, and the moved one goes back to its cut.
    #[test]
    fn a_session_batch_undo_takes_back_only_what_it_still_owns() {
        let b = base("sessions");
        mark_session(&b, S1).unwrap();
        age_session(&b, S1, OLD);
        let ten_minutes_ago = SystemTime::now() - Duration::from_secs(600);
        let (batch, ids) = mark_sessions(&b, &keys(&[S3, S1, S2, "aaron", S2])).unwrap().unwrap();
        assert_eq!(ids, keys(&[S1, S2, S3]), "a person's id is left out, the rest sorted once");
        assert_eq!(session_batches(&b), vec![batch.clone()]);
        let marks = Marks::load(&b);
        assert!([S1, S2, S3].iter().all(|s| marks.hides_sender(s, ten_minutes_ago)), "S1's cut moved to now");

        mark_session(&b, S3).unwrap();
        mark_session(&b, S4).unwrap();
        let (back, kept) = restored(undo_sessions(&b, None).unwrap());
        assert_eq!((back, kept), (keys(&[S1, S2]), keys(&[S3])));
        assert_eq!(sessions(&b), keys(&[S1, S3, S4]), "S2 was new: removed; the later single tombstones stand");
        let marks = Marks::load(&b);
        assert!(!marks.hides_sender(S1, ten_minutes_ago) && marks.hides_sender(S1, SystemTime::now() - OLD - Duration::from_secs(60)), "S1 is back at its old cut");
        assert_eq!(session_owner(&b, S1), None, "and owned by no batch, as before");
        assert!(session_batches(&b).is_empty());
        assert_eq!(undo_sessions(&b, None).unwrap(), SessionUndo::NoBatch, "nothing left to undo");
        assert_eq!(undo_sessions(&b, Some(&batch)).unwrap(), SessionUndo::NoBatch, "undone once");
        fs::remove_dir_all(&b).ok();
    }

    /// Two batches over one session: undoing the newer gives the tombstone
    /// back to the older, whose undo then removes it.
    #[test]
    fn a_newer_batch_hands_its_session_back_to_the_older() {
        let b = base("nested");
        let (first, _) = mark_sessions(&b, &keys(&[S1])).unwrap().unwrap();
        age_session(&b, S1, OLD);
        std::thread::sleep(Duration::from_millis(2));
        let (second, _) = mark_sessions(&b, &keys(&[S1, S2])).unwrap().unwrap();
        assert_eq!(session_batches(&b), vec![first.clone(), second.clone()]);
        assert_eq!(restored(undo_sessions(&b, None).unwrap()), (keys(&[S1, S2]), vec![]), "the newer batch first");
        assert_eq!(sessions(&b), keys(&[S1]), "S2 was new to the newer batch");
        assert_eq!(session_owner(&b, S1), Some(first.clone()), "S1 goes back to the older batch");
        assert!(!Marks::load(&b).hides_sender(S1, SystemTime::now() - Duration::from_secs(600)), "at the older cut");
        assert_eq!(restored(undo_sessions(&b, None).unwrap()), (keys(&[S1]), vec![]));
        assert!(sessions(&b).is_empty() && session_batches(&b).is_empty());
        fs::remove_dir_all(&b).ok();
    }

    #[test]
    fn no_session_writes_no_session_batch() {
        let b = base("no-sessions");
        assert_eq!(mark_sessions(&b, &keys(&["aaron", "../x"])).unwrap(), None);
        assert!(session_batches(&b).is_empty() && sessions(&b).is_empty());
        assert_eq!(undo_sessions(&b, Some("not a batch")).unwrap(), SessionUndo::NoBatch);
        fs::remove_dir_all(&b).ok();
    }
}
