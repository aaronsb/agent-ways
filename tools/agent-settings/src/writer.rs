//! The one settings writer (ADR-503 §6), generalised from the `ways-core`
//! targets writer: a lock file beside the target, the edit made on the text by
//! [`crate::yaml_edit`] and verified, and a temporary file renamed into place.
//! Keys the edit does not name, comments and order stay as they were.

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
/// dies releases it. The holder removes the file before releasing it; a waiter
/// that then gets the lock on the removed file sees the path no longer names
/// it and tries again, so two holders never overlap.
pub struct Lock {
    path: PathBuf,
    _file: File,
}

impl Lock {
    pub fn acquire(target: &Path) -> io::Result<Lock> {
        let path = lock_path(target);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        loop {
            let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path)?;
            file.lock()?;
            if same_file(&file, &path) {
                return Ok(Lock { path, _file: file });
            }
        }
    }
}

#[cfg(unix)]
fn same_file(file: &File, path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (file.metadata(), std::fs::metadata(path)) {
        (Ok(a), Ok(b)) => a.ino() == b.ino() && a.dev() == b.dev(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn same_file(_file: &File, _path: &Path) -> bool {
    true
}

impl Drop for Lock {
    fn drop(&mut self) {
        // Removed while held; the OS lock goes with the handle right after.
        #[cfg(unix)]
        let _ = std::fs::remove_file(&self.path);
    }
}

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Write `body` to `path` through a temporary file in the same directory and
/// a rename, so a reader never sees half a file. The temporary name is unique
/// to the process and the call; it is removed when the write fails.
pub fn write_atomic(path: &Path, body: &str) -> io::Result<()> {
    let dir = match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    };
    std::fs::create_dir_all(&dir)?;
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.{}.{}.tmp", std::process::id(), TMP_SEQ.fetch_add(1, Ordering::Relaxed)));
    let result = std::fs::write(&tmp, body).and_then(|_| std::fs::rename(&tmp, path));
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
    let _lock = Lock::acquire(path).map_err(|e| WriteError::Lock(path.to_path_buf(), e))?;
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
    write_atomic(path, &doc.text()).map_err(|e| WriteError::Write(path.to_path_buf(), e))?;
    Ok((out, true))
}

/// Create `path` with `body` only when no file is there (`config init`).
/// Returns false when one already exists.
pub fn create_new(path: &Path, body: &str) -> Result<bool, WriteError> {
    let _lock = Lock::acquire(path).map_err(|e| WriteError::Lock(path.to_path_buf(), e))?;
    if path.exists() {
        return Ok(false);
    }
    write_atomic(path, body).map_err(|e| WriteError::Write(path.to_path_buf(), e))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        std::fs::remove_dir_all(&dir).ok();
    }
}
