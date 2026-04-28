//! Per-session liveness heartbeat (ADR-129).
//!
//! Each running attend touches `~/.cache/attend/heartbeat/<session-id>`
//! on every tick. The file's mtime is the last_seen timestamp — there
//! is no body, no parsing, no schema. Consumers read mtime and compare
//! against a grace window:
//!
//! - `groups::session_alive` (attend) gates focus-group membership
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

/// Default grace window. A session whose heartbeat is older than this
/// is considered stale. Sized to 3× attend's base sensor interval so a
/// single missed tick does not flip liveness.
pub const DEFAULT_GRACE: Duration = Duration::from_secs(90);

/// Directory holding all heartbeat files for the current user.
pub fn heartbeat_dir() -> PathBuf {
    home_dir()
        .join(".cache")
        .join("attend")
        .join("heartbeat")
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

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::Duration;

    // `$HOME` is process-global. cargo runs tests in parallel by
    // default, so without serialization one test's tempdir overrides
    // another's mid-run. The mutex makes `with_home` the only writer
    // at a time. Held across the whole closure body so every read
    // and write inside sees a consistent `$HOME`.
    static HOME_LOCK: Mutex<()> = Mutex::new(());

    fn with_home<F: FnOnce(&PathBuf)>(f: F) {
        let _guard = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let home = std::env::temp_dir().join(format!(
            "attend-hb-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&home).unwrap();
        let prev = std::env::var("HOME").ok();
        std::env::set_var("HOME", &home);
        f(&home);
        match prev {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
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
