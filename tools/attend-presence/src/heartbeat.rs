//! Per-session liveness heartbeat (ADR-129).
//!
//! Each running attend touches `<cache>/attend/heartbeat/<session-id>`
//! (see [`crate::cache`])
//! on every tick. The file's mtime is the last_seen timestamp — there
//! is no body, no parsing, no schema. Consumers read mtime and compare
//! against a grace window:
//!
//! - `attend_groups::member_alive` gates channel membership
//!   cleanup on heartbeat freshness.
//! - `chip::known_identities` (attend-chat) filters signal-derived
//!   chips so dead peers stop polluting the legend after reload.
//!
//! Why a sidecar file rather than a field on `_groups.yaml`: the only
//! writer for a given session is that session itself, so per-session
//! files have zero write contention. The yaml gets touched far less
//! often, which matters because peers read it during routing.
//!
//! The grace window must be larger than the longest plausible attend
//! tick gap — `DEFAULT_GRACE` (90s) is 3× the base sensor interval, so
//! a single skipped poll does not evict a session.

use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

#[cfg(unix)]
use std::os::unix::io::AsRawFd;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;

/// Default grace window. A session whose heartbeat is older than this
/// is considered stale. Sized to 3× attend's base sensor interval so a
/// single missed tick does not flip liveness.
pub const DEFAULT_GRACE: Duration = Duration::from_secs(90);

/// Directory holding all heartbeat files for the current user.
pub fn heartbeat_dir() -> PathBuf {
    crate::cache::dir().join("heartbeat")
}

/// Path to the heartbeat file for a given session id.
pub fn heartbeat_path(session_id: &str) -> PathBuf {
    heartbeat_dir().join(session_id)
}

/// Touch the heartbeat for this session — create the file if absent,
/// update mtime if present. Best-effort; callers typically discard
/// the result because a missed heartbeat tick is recoverable on the
/// next pass.
pub fn touch(session_id: &str) -> io::Result<()> {
    let dir = heartbeat_dir();
    fs::create_dir_all(&dir)?;
    let path = heartbeat_path(session_id);
    // OpenOptions truncate-write of zero bytes is the simplest portable
    // mtime bump: opening with `write(true)` updates mtime even when
    // the body is empty.
    use std::fs::OpenOptions;
    use std::io::Write;
    let mut f = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&path)?;
    f.write_all(b"")?;
    Ok(())
}

/// Read the last-seen `SystemTime` for this session, or `None` when
/// the heartbeat file is missing or its mtime cannot be read.
pub fn last_seen(session_id: &str) -> Option<SystemTime> {
    fs::metadata(heartbeat_path(session_id))
        .ok()
        .and_then(|m| m.modified().ok())
}

/// Whether the session's heartbeat is within `grace`. False when the
/// file is missing (no attend ever touched it) or the mtime is older
/// than `grace`.
///
/// A future-dated mtime (clock skew, restored backup) is treated as
/// fresh — defensively erring on the side of "the session is alive"
/// rather than evicting a session because a filesystem reported a
/// time we cannot trust.
pub fn is_fresh(session_id: &str, grace: Duration) -> bool {
    match last_seen(session_id) {
        Some(t) => match SystemTime::now().duration_since(t) {
            Ok(age) => age < grace,
            Err(_) => true,
        },
        None => false,
    }
}

/// Remove the heartbeat file for a session — call on clean shutdown
/// so a stopped attend does not appear fresh until grace expires.
/// Best-effort; absence of the file is not an error.
pub fn clear(session_id: &str) -> io::Result<()> {
    match fs::remove_file(heartbeat_path(session_id)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Holder for an exclusive process-level lock on a session. While the
/// value is alive the OS guarantees no other process holds the lock
/// for the same session id. Drop the value (or the process exits) and
/// the OS releases automatically.
///
/// Implementation differs by platform:
/// - Unix: non-blocking `flock(LOCK_EX | LOCK_NB)` on the heartbeat
///   file's fd.
/// - Windows: exclusive `share_mode(0)` on a sibling `<id>.lock` file,
///   so the heartbeat file itself remains freely openable by `touch`.
///
/// In both cases we keep the `File` open for the lock's lifetime, and
/// even a panicking attend releases on process exit.
pub struct SessionLock {
    _file: fs::File,
}

/// Path to the lock sidecar file (Windows). Kept separate from the
/// heartbeat file because Windows exclusive sharing applies across
/// every handle to the path — including the lock-holder's own
/// future `touch()` opens — so locking the heartbeat directly would
/// break the regular mtime bump.
#[cfg(windows)]
fn lock_path(session_id: &str) -> PathBuf {
    heartbeat_dir().join(format!("{session_id}.lock"))
}

/// Try to acquire the exclusive process lock for a session. Returns
/// `Ok(Some(_))` when this process owns the lock, `Ok(None)` when
/// another process already holds it (caller should exit cleanly), or
/// `Err` for IO failures opening the file.
///
/// This is the duplicate-attend guard. The orphan / re-launched
/// attend case (parent shell killed, child reparented to init,
/// then a new attend started) is exactly what file locking is
/// designed for: the orphan still holds the OS lock, so the new
/// attend's non-blocking attempt fails fast instead of silently
/// double-running.
///
/// Self-reload via `exec()` (Unix) keeps the same PID. Rust's standard
/// library opens files with `O_CLOEXEC`, so the lock's descriptor closes
/// at `exec()` and the kernel releases the lock; the re-executed process
/// acquires it again. A reload that finds it held (`Ok(None)` under
/// `ATTEND_RELOADED_FROM`) proceeds rather than exiting.
///
/// On Windows self-reload spawns a new process rather than exec()ing,
/// so the lock IS released before the child starts; the new process
/// re-acquires cleanly.
#[cfg(unix)]
pub fn try_acquire_session_lock(session_id: &str) -> io::Result<Option<SessionLock>> {
    fs::create_dir_all(heartbeat_dir())?;
    let path = heartbeat_path(session_id);
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)?;
    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if ret == 0 {
        return Ok(Some(SessionLock { _file: file }));
    }
    let err = io::Error::last_os_error();
    if err.raw_os_error() == Some(libc::EWOULDBLOCK) {
        Ok(None)
    } else {
        Err(err)
    }
}

#[cfg(windows)]
pub fn try_acquire_session_lock(session_id: &str) -> io::Result<Option<SessionLock>> {
    fs::create_dir_all(heartbeat_dir())?;
    let path = lock_path(session_id);
    // share_mode(0): exclusive access. A second process attempting
    // to open the same path gets ERROR_SHARING_VIOLATION (raw 32) —
    // which is exactly the duplicate-attend signal we want.
    match fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .share_mode(0)
        .open(&path)
    {
        Ok(file) => Ok(Some(SessionLock { _file: file })),
        Err(e) if e.raw_os_error() == Some(32) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Whether an `attend run` holds the session lock for `session_id`.
/// Unlike [`try_acquire_session_lock`] this never creates the heartbeat
/// file, so asking does not make the session look alive.
pub fn run_is_live(session_id: &str) -> bool {
    #[cfg(unix)]
    {
        let Ok(file) = fs::OpenOptions::new().write(true).open(heartbeat_path(session_id)) else {
            return false;
        };
        let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if ret == 0 {
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
            return false;
        }
        io::Error::last_os_error().raw_os_error() == Some(libc::EWOULDBLOCK)
    }
    #[cfg(windows)]
    {
        // The run holds `<id>.lock` with no sharing, so any other open of
        // it fails with ERROR_SHARING_VIOLATION (32) while the run lives.
        matches!(
            fs::OpenOptions::new().write(true).open(lock_path(session_id)),
            Err(e) if e.raw_os_error() == Some(32)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // `HOME` and `XDG_CACHE_HOME` are process-global, and cargo runs
    // tests in parallel, so every test that points them somewhere holds
    // the crate's one env lock for its whole body.
    fn with_home<F: FnOnce(&PathBuf)>(f: F) {
        let _guard = crate::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let home = std::env::temp_dir().join(format!(
            "attend-hb-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&home).unwrap();
        let prev = (std::env::var_os("HOME"), std::env::var_os("XDG_CACHE_HOME"));
        std::env::set_var("HOME", &home);
        std::env::remove_var("XDG_CACHE_HOME");
        f(&home);
        match prev.0 {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        if let Some(v) = prev.1 {
            std::env::set_var("XDG_CACHE_HOME", v);
        }
        fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn missing_heartbeat_is_not_fresh() {
        with_home(|_| {
            assert!(!is_fresh("nope", DEFAULT_GRACE));
            assert!(last_seen("nope").is_none());
        });
    }

    #[test]
    fn touched_heartbeat_is_fresh() {
        with_home(|_| {
            touch("session-x").unwrap();
            assert!(is_fresh("session-x", DEFAULT_GRACE));
            assert!(last_seen("session-x").is_some());
        });
    }

    #[test]
    fn touch_creates_directory_if_missing() {
        with_home(|home| {
            assert!(!home.join(".cache").join("attend").join("heartbeat").exists());
            touch("session-y").unwrap();
            assert!(heartbeat_path("session-y").exists());
        });
    }

    #[test]
    fn clear_makes_session_stale() {
        with_home(|_| {
            touch("session-z").unwrap();
            assert!(is_fresh("session-z", DEFAULT_GRACE));
            clear("session-z").unwrap();
            assert!(!is_fresh("session-z", DEFAULT_GRACE));
        });
    }

    #[test]
    fn clear_is_idempotent_when_absent() {
        with_home(|_| {
            // Calling clear on a session that never heartbeated is a
            // no-op, not an error — supports clean-shutdown paths
            // that do not know whether they ever touched.
            assert!(clear("never-existed").is_ok());
        });
    }

    #[cfg(unix)]
    #[test]
    fn first_lock_acquires_second_returns_none() {
        with_home(|_| {
            let first = try_acquire_session_lock("sess-lock-a")
                .expect("first acquire io ok")
                .expect("first acquire holds lock");
            // Second concurrent acquire on the same session-id must
            // return Ok(None) — Some(_) would mean both processes
            // think they own the lock, which is the exact bug the
            // duplicate-attend guard is meant to catch.
            let second = try_acquire_session_lock("sess-lock-a")
                .expect("second acquire io ok");
            assert!(second.is_none(), "second acquire must observe first's lock");
            drop(first);
            // Once the first lock drops, a fresh acquire should win.
            let third = try_acquire_session_lock("sess-lock-a")
                .expect("third acquire io ok")
                .expect("third acquire after drop");
            drop(third);
        });
    }

    #[cfg(unix)]
    #[test]
    fn distinct_session_ids_lock_independently() {
        with_home(|_| {
            let a = try_acquire_session_lock("sess-lock-x")
                .expect("a io ok")
                .expect("a holds");
            let b = try_acquire_session_lock("sess-lock-y")
                .expect("b io ok")
                .expect("b holds — different session must not contend");
            drop(a);
            drop(b);
        });
    }

    #[test]
    fn run_is_live_follows_the_lock_and_creates_nothing() {
        with_home(|_| {
            assert!(!run_is_live("sess-live"));
            assert!(!heartbeat_path("sess-live").exists(), "asking must not create a heartbeat");
            let lock = try_acquire_session_lock("sess-live").unwrap().unwrap();
            assert!(run_is_live("sess-live"));
            drop(lock);
            assert!(!run_is_live("sess-live"));
        });
    }

    #[test]
    fn stale_when_grace_is_zero() {
        with_home(|_| {
            touch("s").unwrap();
            // Zero grace: any positive age is stale. SystemTime resolution
            // is fine enough that "right after touch" still has a measurable
            // delta — but use a small sleep to make the assertion robust
            // against zero-elapsed clocks.
            std::thread::sleep(Duration::from_millis(2));
            assert!(!is_fresh("s", Duration::from_secs(0)));
        });
    }
}
