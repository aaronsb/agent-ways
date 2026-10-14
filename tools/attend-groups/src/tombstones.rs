//! The mark store under `signals/_tombstones/` (ADR-404 D3e).
//!
//! A delete marks a message instead of removing its file, and every reader
//! skips what is marked. `attend message cleanup` removes a marked message's
//! file, and its mark, once the mark has aged past its limits.
//!
//! Layout:
//! - `<session-id>`: a session tombstone, an empty file whose mtime is
//!   when it was written. Every reader skips what that session sent up to
//!   then; what it sends later, back under `--resume`, shows. `attend
//!   message tombstone` writes it for a session that has exited, and
//!   `--undo` removes it.
//! - `messages/<key>`: one mark per message. The key is the message's
//!   seen-set key, its filename, which is unique across rooms. The file's
//!   mtime is the mark time and its content the id of the batch that wrote
//!   it. A message already marked keeps its first mark.
//! - `batches/<batch-id>`: the keys one delete marked, one per line, so
//!   `attend message delete --undo` can take that delete back. Batch ids
//!   are the mark time in milliseconds, zero-padded, then the writer's pid,
//!   so the newest batch sorts last.
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

/// Tombstone session `id`. Idempotent.
pub fn mark_session(base: &Path, id: &str) -> io::Result<()> {
    if !valid_session_id(id) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("`{id}` is not a session id")));
    }
    fs::create_dir_all(dir(base))?;
    fs::OpenOptions::new().write(true).create(true).truncate(false).open(dir(base).join(id)).map(|_| ())
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
/// reader never finds a mark without its batch id.
pub fn mark_messages(base: &Path, keys: &[String]) -> io::Result<Option<(String, Vec<String>)>> {
    let marks = messages_dir(base);
    let mut fresh: Vec<String> = keys.iter().filter(|k| valid_key(k) && !marks.join(k).exists()).cloned().collect();
    fresh.sort();
    fresh.dedup();
    if fresh.is_empty() {
        return Ok(None);
    }
    let millis = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let batch = format!("{millis:015}-{}", std::process::id());
    fs::create_dir_all(&marks)?;
    let list: String = fresh.iter().map(|k| format!("{k}\n")).collect();
    agent_settings::writer::write_atomic(&batches_dir(base).join(&batch), list)?;
    let staged = marks.join(format!(".{batch}.tmp"));
    fs::write(&staged, &batch)?;
    let mut marked = Vec::new();
    let mut failed = None;
    for key in fresh {
        match fs::hard_link(&staged, marks.join(&key)) {
            Ok(()) => marked.push(key),
            // Another delete marked it in between: its mark stands.
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => {
                failed = Some(e);
                break;
            }
        }
    }
    fs::remove_file(&staged).ok();
    match failed {
        Some(e) => Err(e),
        None => Ok(Some((batch, marked))),
    }
}

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
        mark_session(&b, dead).unwrap();
        mark_session(&b, dead).unwrap();
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
}
