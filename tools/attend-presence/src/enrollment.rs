//! Whether a session enrolled in attend (#720).
//!
//! A session enrolls by running `attend run` (`/attend`) or by joining a
//! channel. Enrollment is what the Stop-hook drain delivers to, and it is a
//! durable fact about the session, kept apart from liveness: a stale
//! heartbeat, or `cleanup_stale` pruning a channel membership, does not
//! un-enroll a session. Only an explicit opt-out does.
//!
//! The record is `<cache>/enrolled/<session-id>`, one line per way the
//! session enrolled (`run`, `join`). The session is enrolled while the file
//! exists; withdrawing the last way removes it.

use std::fs;
use std::io;
use std::path::PathBuf;

/// A way a session enrolled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// `attend run`.
    Run,
    /// `attend join`, or a scene that joins a channel.
    Join,
}

impl Source {
    fn as_str(self) -> &'static str {
        match self {
            Source::Run => "run",
            Source::Join => "join",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "run" => Some(Source::Run),
            "join" => Some(Source::Join),
            _ => None,
        }
    }
}

fn dir() -> PathBuf {
    crate::cache::dir().join("enrolled")
}

fn path(session_id: &str) -> PathBuf {
    dir().join(session_id)
}

/// Whether `session_id` is enrolled.
pub fn is_enrolled(session_id: &str) -> bool {
    !session_id.is_empty() && path(session_id).is_file()
}

/// The ways `session_id` enrolled, empty when it is not enrolled.
pub fn sources(session_id: &str) -> Vec<Source> {
    fs::read_to_string(path(session_id))
        .map(|s| s.lines().filter_map(Source::parse).collect())
        .unwrap_or_default()
}

fn write(session_id: &str, sources: &[Source]) -> io::Result<()> {
    let p = path(session_id);
    if sources.is_empty() {
        return match fs::remove_file(&p) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    fs::create_dir_all(dir())?;
    let body: String = sources.iter().map(|s| format!("{}\n", s.as_str())).collect();
    let tmp = p.with_extension(format!("tmp.{}", std::process::id()));
    fs::write(&tmp, body)?;
    fs::rename(&tmp, &p)
}

/// Record that `session_id` enrolled by `source`. Idempotent.
pub fn enroll(session_id: &str, source: Source) -> io::Result<()> {
    let mut have = sources(session_id);
    if have.contains(&source) && path(session_id).is_file() {
        return Ok(());
    }
    if !have.contains(&source) {
        have.push(source);
    }
    write(session_id, &have)
}

/// Withdraw one way `session_id` enrolled. The session stays enrolled while
/// another way remains.
pub fn withdraw(session_id: &str, source: Source) -> io::Result<()> {
    let have: Vec<Source> = sources(session_id).into_iter().filter(|s| *s != source).collect();
    write(session_id, &have)
}

/// Move `old`'s enrollment to `new`, for a session whose id changed under a
/// running process (`/clear`). Keeps any way `new` already had.
pub fn carry(old: &str, new: &str) -> io::Result<()> {
    let mut have = sources(new);
    for s in sources(old) {
        if !have.contains(&s) {
            have.push(s);
        }
    }
    write(new, &have)?;
    write(old, &[])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_cache(f: impl FnOnce()) {
        let _g = crate::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join(format!(
            "attend-enrol-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let prev = std::env::var_os("XDG_CACHE_HOME");
        std::env::set_var("XDG_CACHE_HOME", &root);
        f();
        match prev {
            Some(v) => std::env::set_var("XDG_CACHE_HOME", v),
            None => std::env::remove_var("XDG_CACHE_HOME"),
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn enrollment_lasts_until_its_last_way_is_withdrawn() {
        with_cache(|| {
            assert!(!is_enrolled("s"));
            enroll("s", Source::Join).unwrap();
            enroll("s", Source::Run).unwrap();
            enroll("s", Source::Join).unwrap();
            assert_eq!(sources("s"), vec![Source::Join, Source::Run]);
            withdraw("s", Source::Join).unwrap();
            assert!(is_enrolled("s"), "run still holds it");
            withdraw("s", Source::Run).unwrap();
            assert!(!is_enrolled("s"));
            withdraw("s", Source::Run).unwrap();
        });
    }

    #[test]
    fn carry_moves_every_way_to_the_new_id() {
        with_cache(|| {
            enroll("old", Source::Run).unwrap();
            enroll("old", Source::Join).unwrap();
            carry("old", "new").unwrap();
            assert!(!is_enrolled("old"));
            assert_eq!(sources("new"), vec![Source::Run, Source::Join]);
        });
    }
}
