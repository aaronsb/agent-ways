//! The one settings writer (ADR-503 §6), generalised from the `ways-core`
//! targets writer: a lock file beside the target, the edit made on the text by
//! [`crate::yaml_edit`] and verified, and a temporary file synced and renamed
//! into place. Keys the edit does not name, comments and order stay as they
//! were. A settings path that is a symlink (a dotfiles checkout) is followed:
//! the lock, the temporary file and the rename are beside the real file, so
//! the link stays a link, and the file keeps its permissions.

use crate::yaml_edit::{Doc, EditError};
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Why a write did not happen. Every variant leaves the file as it was.
#[derive(Debug)]
pub enum WriteError {
    Lock(PathBuf, io::Error),
    Read(PathBuf, io::Error),
    Edit(PathBuf, EditError),
    Write(PathBuf, io::Error),
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteError::Lock(p, e) => write!(f, "could not lock {}: {e}", p.display()),
            WriteError::Read(p, e) => write!(f, "could not read {}: {e}", p.display()),
            WriteError::Edit(p, e) => write!(f, "{}: {e}", p.display()),
            WriteError::Write(p, e) => write!(f, "could not write {}: {e}", p.display()),
        }
    }
}

impl std::error::Error for WriteError {}

impl From<WriteError> for io::Error {
    fn from(e: WriteError) -> io::Error {
        io::Error::other(e.to_string())
    }
}

/// The lock file beside `target`: `config.yaml` locks `config.yaml.lock`.
pub fn lock_path(target: &Path) -> PathBuf {
    let name = target.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    target.with_file_name(format!("{name}.lock"))
}

/// An exclusive lock on a settings file, held for one read-modify-write.
///
/// The lock is an OS file lock on a file beside the target, so a holder that
/// dies releases it. The lock file is removed on release, and two holders
/// never overlap:
///
/// - **Unix:** the holder unlinks the file while it still holds the lock. A
///   waiter that then gets the lock on the unlinked file sees the path no
///   longer names it (`same_file`) and tries again.
/// - **Windows:** every handle on the lock file is opened without delete
///   sharing, so the file cannot be deleted while anyone has it open. The
///   holder closes its handle, then deletes the file; the delete fails while
///   a waiter holds the file open, and the waiter locks the same file. A
///   file can be deleted only when nobody has it open, and a later opener
///   then creates a new one. So the path never moves under a holder, and
///   `same_file` need not compare files. An open that meets another
///   process's delete in progress gets a sharing violation and retries.
pub struct Lock {
    path: PathBuf,
    file: Option<File>,
    /// Leave the lock file in place on release ([`Lock::acquire_kept`]).
    keep: bool,
}

impl Lock {
    /// Take the lock on `target`, waiting while another holder has it.
    pub fn acquire(target: &Path) -> io::Result<Lock> {
        Self::take(target, true).map(|l| l.expect("a blocking take always locks"))
    }

    /// Take the lock on `target`, waiting, and leave the lock file in place
    /// when released. For a lock that older binaries also take with a plain
    /// open and lock and never unlink: with the file never removed, every
    /// holder locks the same inode, so a waiter from either side cannot end up
    /// on an unlinked file beside a newer holder. The file stays behind.
    pub fn acquire_kept(target: &Path) -> io::Result<Lock> {
        let mut lock = Self::acquire(target)?;
        lock.keep = true;
        Ok(lock)
    }

    /// Take the lock on `target`, waiting at most `limit` for another holder
    /// to let go: `Ok(None)` when it did not. For a writer that must not
    /// block on a holder that hangs, such as one on a screen's own thread.
    pub fn acquire_within(target: &Path, limit: std::time::Duration) -> io::Result<Option<Lock>> {
        let start = std::time::Instant::now();
        loop {
            if let Some(l) = Self::take(target, false)? {
                return Ok(Some(l));
            }
            if start.elapsed() >= limit {
                return Ok(None);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// Take the lock on `target` if it is free: `Ok(None)` while another
    /// holder has it. For a single-instance guard held for a process's life.
    pub fn try_acquire(target: &Path) -> io::Result<Option<Lock>> {
        Self::take(target, false)
    }

    fn take(target: &Path, wait: bool) -> io::Result<Option<Lock>> {
        let path = lock_path(target);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        loop {
            let file = open_lock_file(&path)?;
            if wait {
                file.lock()?;
            } else {
                match file.try_lock() {
                    Ok(()) => {}
                    Err(std::fs::TryLockError::WouldBlock) => return Ok(None),
                    Err(std::fs::TryLockError::Error(e)) => return Err(e),
                }
            }
            if same_file(&file, &path) {
                return Ok(Some(Lock { path, file: Some(file), keep: false }));
            }
        }
    }
}

/// Open (creating) the lock file. On Windows the handle shares read and
/// write but not delete, which is what makes deleting on release safe.
fn open_lock_file(path: &Path) -> io::Result<File> {
    let mut opts = OpenOptions::new();
    opts.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 0x1;
        const FILE_SHARE_WRITE: u32 = 0x2;
        opts.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE);
        // ERROR_ACCESS_DENIED (5) or ERROR_SHARING_VIOLATION (32): another
        // process is deleting the file at this moment. It is brief.
        let mut tries = 0;
        loop {
            match opts.open(path) {
                Err(e) if matches!(e.raw_os_error(), Some(5 | 32)) && tries < 1000 => {
                    tries += 1;
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                r => return r,
            }
        }
    }
    #[cfg(not(windows))]
    opts.open(path)
}

#[cfg(unix)]
fn same_file(file: &File, path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (file.metadata(), std::fs::metadata(path)) {
        (Ok(a), Ok(b)) => a.ino() == b.ino() && a.dev() == b.dev(),
        _ => false,
    }
}

/// Off Unix the path cannot move under a holder (see [`Lock`]).
#[cfg(not(unix))]
fn same_file(_file: &File, _path: &Path) -> bool {
    true
}

impl Drop for Lock {
    fn drop(&mut self) {
        if self.keep {
            drop(self.file.take());
            return;
        }
        #[cfg(unix)]
        {
            // Unlinked while held; the OS lock goes with the handle after.
            let _ = std::fs::remove_file(&self.path);
            drop(self.file.take());
        }
        #[cfg(windows)]
        {
            // The handle (and the lock) first; the delete then fails while a
            // waiter holds the file open, since no handle shares delete.
            drop(self.file.take());
            let _ = std::fs::remove_file(&self.path);
        }
        // Elsewhere the lock file is left in place.
        #[cfg(not(any(unix, windows)))]
        drop(self.file.take());
    }
}

/// The file a settings path names: symlinks followed, a dangling one to the
/// path it points at. A path that is not a link is returned as it is.
pub fn resolve(path: &Path) -> PathBuf {
    let mut cur = path.to_path_buf();
    for _ in 0..40 {
        match std::fs::symlink_metadata(&cur) {
            Ok(m) if m.file_type().is_symlink() => match std::fs::read_link(&cur) {
                Ok(next) => {
                    cur = if next.is_absolute() {
                        next
                    } else {
                        cur.parent().map(|d| d.join(&next)).unwrap_or(next)
                    };
                }
                Err(_) => return cur,
            },
            _ => return cur,
        }
    }
    cur
}

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Write `body` to `path` through a temporary file in the same directory and
/// a rename, so a reader never sees half a file. The temporary file takes the
/// permissions of the file it replaces and is synced to disk before the
/// rename. Its name is unique to the process and the call; it is removed when
/// the write fails. `path` is written as given: callers resolve links first.
pub fn write_atomic(path: &Path, body: impl AsRef<[u8]>) -> io::Result<()> {
    let dir = match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    };
    std::fs::create_dir_all(&dir)?;
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.{}.{}.tmp", std::process::id(), TMP_SEQ.fetch_add(1, Ordering::Relaxed)));
    let perms = std::fs::metadata(path).ok().filter(|m| m.is_file()).map(|m| m.permissions());
    let result = (|| {
        let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        if let Some(p) = perms {
            f.set_permissions(p)?;
        }
        io::Write::write_all(&mut f, body.as_ref())?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)?;
        // The rename is durable once the directory is.
        #[cfg(unix)]
        if let Ok(d) = File::open(&dir) {
            let _ = d.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Read, edit and write one settings file under its lock. `header` is the
/// text a new or empty file starts with. Returns the closure's result and
/// whether the file changed; an edit that changes nothing writes nothing,
/// and a missing file the edit leaves alone is not created.
pub fn edit_file<T>(
    path: &Path,
    header: Option<&str>,
    f: impl FnOnce(&mut Doc) -> Result<T, EditError>,
) -> Result<(T, bool), WriteError> {
    edit_file_within(path, header, None, f)
}

/// [`edit_file`], waiting at most `limit` for the lock when one is given; a
/// holder that keeps it longer fails the write with a `TimedOut` lock error
/// and nothing written.
pub fn edit_file_within<T>(
    path: &Path,
    header: Option<&str>,
    limit: Option<std::time::Duration>,
    f: impl FnOnce(&mut Doc) -> Result<T, EditError>,
) -> Result<(T, bool), WriteError> {
    let path = &resolve(path);
    let lock = match limit {
        None => Lock::acquire(path).map(Some),
        Some(l) => Lock::acquire_within(path, l),
    };
    let _lock = match lock {
        Ok(Some(l)) => l,
        Ok(None) => {
            let e = io::Error::new(io::ErrorKind::TimedOut, "another writer holds it");
            return Err(WriteError::Lock(path.to_path_buf(), e));
        }
        Err(e) => return Err(WriteError::Lock(path.to_path_buf(), e)),
    };
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(WriteError::Read(path.to_path_buf(), e)),
    };
    let start = if text.trim().is_empty() { header.unwrap_or(&text) } else { &text };
    let mut doc = Doc::parse(start).map_err(|e| WriteError::Edit(path.to_path_buf(), e))?;
    let before = doc.value().clone();
    let out = f(&mut doc).map_err(|e| WriteError::Edit(path.to_path_buf(), e))?;
    if doc.value() == &before {
        return Ok((out, false));
    }
    doc.verify().map_err(|e| WriteError::Edit(path.to_path_buf(), e))?;
    write_atomic(path, doc.text()).map_err(|e| WriteError::Write(path.to_path_buf(), e))?;
    Ok((out, true))
}

/// Create `path` with `body` only when nothing is there (`config init`).
/// Returns false when something already is. Never overwrites, by
/// construction rather than by a check: the body is written to a temporary
/// file, synced, and hard-linked to `path`, which the OS refuses when any
/// entry, even a dangling or looping link, holds the name. A writer that
/// takes no lock (an editor) creating the file at the same moment wins, and
/// is kept.
pub fn create_new(path: &Path, body: &str) -> Result<bool, WriteError> {
    let path = &resolve(path);
    let _lock = Lock::acquire(path).map_err(|e| WriteError::Lock(path.to_path_buf(), e))?;
    let dir = match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let err = |e| WriteError::Write(path.to_path_buf(), e);
    std::fs::create_dir_all(&dir).map_err(err)?;
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.{}.{}.new", std::process::id(), TMP_SEQ.fetch_add(1, Ordering::Relaxed)));
    let result = (|| {
        let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        io::Write::write_all(&mut f, body.as_bytes())?;
        f.sync_all()?;
        drop(f);
        match std::fs::hard_link(&tmp, path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(false),
            Err(e) => Err(e),
        }
    })();
    let _ = std::fs::remove_file(&tmp);
    result.map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bounded_write_gives_up_on_a_held_lock_and_writes_nothing() {
        let d = tmp("within");
        let f = d.join("c.yaml");
        std::fs::write(&f, "a: 1\n").unwrap();
        let held = Lock::acquire(&f).unwrap();
        let start = std::time::Instant::now();
        let r = edit_file_within(&f, None, Some(std::time::Duration::from_millis(200)), |doc| doc.set(&["a".into()], &serde_yaml::Value::from(2)));
        assert!(matches!(&r, Err(WriteError::Lock(_, e)) if e.kind() == io::ErrorKind::TimedOut), "{r:?}");
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "a: 1\n");
        drop(held);
        edit_file_within(&f, None, Some(std::time::Duration::from_millis(200)), |doc| doc.set(&["a".into()], &serde_yaml::Value::from(2))).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "a: 2\n");
    }

    /// A kept lock leaves its file in place on release, so a holder that never
    /// unlinks (an older binary) and a newer one always lock the same inode.
    #[test]
    fn a_kept_lock_leaves_its_file_on_release() {
        let d = tmp("kept-lock");
        let target = d.join("way");
        drop(Lock::acquire_kept(&target).unwrap());
        assert!(lock_path(&target).is_file(), "the kept lock file stays");
        drop(Lock::acquire_kept(&target).unwrap());
        assert!(lock_path(&target).is_file());
    }

    #[test]
    fn try_acquire_refuses_while_held_and_frees_on_drop() {
        let d = tmp("try-lock");
        let target = d.join("agent");
        let held = Lock::try_acquire(&target).unwrap().expect("free lock");
        assert!(Lock::try_acquire(&target).unwrap().is_none(), "second holder refused");
        drop(held);
        assert!(Lock::try_acquire(&target).unwrap().is_some(), "free again after drop");
    }

    #[test]
    fn write_atomic_takes_bytes() {
        let d = tmp("bytes");
        let p = d.join("log.jsonl");
        write_atomic(&p, b"{}\n".as_slice()).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{}\n");
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("agent-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn key(s: &str) -> Vec<String> {
        s.split('.').map(str::to_string).collect()
    }

    #[test]
    fn a_set_keeps_every_unrelated_line() {
        let dir = tmp("keep");
        let path = dir.join("config.yaml");
        let src = "# ways configuration\n# language: en\n\nlanguage: es   # mine\ndisabled_domains: [ea]\n\n# matching\nsemantic_fire_probability: 0.5\nrefire_presets:\n  normal: 0.2  # slower\n# end\n";
        std::fs::write(&path, src).unwrap();
        let (_, changed) = edit_file(&path, None, |d| d.set(&key("semantic_fire_probability"), &serde_yaml::from_str("0.4").unwrap())).unwrap();
        assert!(changed);
        let body = std::fs::read_to_string(&path).unwrap();
        assert_eq!(body, src.replace("semantic_fire_probability: 0.5", "semantic_fire_probability: 0.4"));
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(names, vec![std::ffi::OsString::from("config.yaml")], "no lock or temp file left");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_no_op_edit_does_not_create_or_touch_the_file() {
        let dir = tmp("noop");
        let path = dir.join("ways.yaml");
        let (_, changed) = edit_file(&path, Some("# header\n"), |d| d.unset(&key("ways.a"))).unwrap();
        assert!(!changed);
        assert!(!path.exists());
        let (_, changed) = edit_file(&path, Some("# header\n"), |d| d.set(&key("ways.a"), &serde_yaml::Value::Bool(false))).unwrap();
        assert!(changed);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# header\nways:\n  a: false\n");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_unparseable_file_is_left_alone() {
        let dir = tmp("bad");
        let path = dir.join("config.yaml");
        std::fs::write(&path, "a: [\n").unwrap();
        let err = edit_file(&path, None, |d| d.set(&key("b"), &serde_yaml::Value::Bool(true))).unwrap_err();
        assert!(matches!(err, WriteError::Edit(_, EditError::Parse { .. })), "{err}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a: [\n");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn concurrent_writers_never_lose_an_update() {
        let dir = tmp("race");
        let path = dir.join("config.yaml");
        std::fs::write(&path, "# shared\n").unwrap();
        let writers: Vec<_> = (0..8)
            .map(|i| {
                let path = path.clone();
                std::thread::spawn(move || {
                    for j in 0..25 {
                        edit_file(&path, None, |d| {
                            let n = d.get(&key("count")).and_then(|v| v.as_i64()).unwrap_or(0);
                            d.set(&key("count"), &serde_yaml::Value::Number((n + 1).into()))?;
                            d.set(&key(&format!("w{i}")), &serde_yaml::Value::Number(j.into()))
                        })
                        .unwrap();
                    }
                })
            })
            .collect();
        for w in writers {
            w.join().unwrap();
        }
        let doc: serde_yaml::Value = serde_yaml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(doc.get("count").and_then(|v| v.as_i64()), Some(200));
        assert!(std::fs::read_to_string(&path).unwrap().starts_with("# shared\n"));
        let left = std::fs::read_dir(&dir).unwrap().count();
        assert_eq!(left, 1, "only the settings file remains");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_failed_rename_leaves_no_temp_file() {
        let dir = tmp("fail");
        let path = dir.join("agent.yaml");
        std::fs::create_dir_all(path.join("occupied")).unwrap();
        assert!(write_atomic(&path, "x: 1\n").is_err());
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(names, vec![std::ffi::OsString::from("agent.yaml")]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn create_new_never_overwrites() {
        let dir = tmp("init");
        let path = dir.join("config.yaml");
        assert!(create_new(&path, "# a\n").unwrap());
        assert!(!create_new(&path, "# b\n").unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# a\n");
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(names, vec![std::ffi::OsString::from("config.yaml")], "no temporary or lock file left");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Something at the name that `exists()` does not see, here a link to
    /// itself, is never replaced: the create is refused by the OS, not
    /// decided by a check before it.
    #[cfg(unix)]
    #[test]
    fn create_new_never_replaces_an_entry_a_check_would_miss() {
        let dir = tmp("init-loop");
        let path = dir.join("config.yaml");
        std::os::unix::fs::symlink("config.yaml", &path).unwrap();
        assert!(!path.exists(), "a looping link does not exist to a check");
        assert!(!create_new(&path, "# a\n").unwrap());
        assert!(std::fs::symlink_metadata(&path).unwrap().file_type().is_symlink(), "the entry is kept");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_0600_file_is_edited_through_the_link_and_keeps_its_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tmp("link");
        let dots = dir.join("dotfiles");
        let conf = dir.join("config");
        std::fs::create_dir_all(&dots).unwrap();
        std::fs::create_dir_all(&conf).unwrap();
        let real = dots.join("config.yaml");
        std::fs::write(&real, "# mine\nlanguage: es\n").unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
        let link = conf.join("config.yaml");
        std::os::unix::fs::symlink("../dotfiles/config.yaml", &link).unwrap();
        edit_file(&link, None, |d| d.set(&key("near_miss_margin"), &serde_yaml::from_str("0.1").unwrap())).unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "the link stays a link");
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "# mine\nlanguage: es\nnear_miss_margin: 0.1\n");
        assert_eq!(std::fs::metadata(&real).unwrap().permissions().mode() & 0o777, 0o600);
        let beside_link: Vec<_> = std::fs::read_dir(&conf).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(beside_link, vec![std::ffi::OsString::from("config.yaml")], "no lock or temp file beside the link");
        let beside_real: Vec<_> = std::fs::read_dir(&dots).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(beside_real, vec![std::ffi::OsString::from("config.yaml")]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_round_trip_refusal_says_nothing_written_once() {
        let e = WriteError::Edit(PathBuf::from("/x.yaml"), EditError::RoundTrip("why".into()));
        assert_eq!(e.to_string().matches("nothing written").count(), 0, "callers add it once: {e}");
    }
}
