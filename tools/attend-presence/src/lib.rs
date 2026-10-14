//! Presence on the attend mesh: who this session is, and whether a session
//! is alive.
//!
//! - [`session`]: the canonical identity tuple `(sessionId ∩ origin_path)`
//!   (issue #378, ADR-171).
//! - [`heartbeat`]: the per-session liveness file (ADR-129) and the
//!   duplicate-attend lock.
//! - [`alive`]: the one predicate for "is this member live".
//! - [`enrollment`]: whether a session enrolled in attend (#720).
//! - [`cache`]: attend's cache root, under `XDG_CACHE_HOME`.
//! - [`process`]: a pid's parent and command line.
//!
//! Formerly the `attend-session` and `attend-heartbeat` crates (#701,
//! ADR-505), which every consumer depended on together.

pub mod cache;
pub mod enrollment;
pub mod heartbeat;
pub mod process;
pub mod session;

/// Serializes the tests that point `HOME` or `XDG_CACHE_HOME` elsewhere.
#[cfg(test)]
pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Whether `member_id` is a live member of the mesh: a Claude session in
/// `running` (the ids of live Claude processes, from
/// `sensor_peers::PeerSensor::live_session_ids`), any member whose
/// heartbeat is within [`heartbeat::DEFAULT_GRACE`], or a listening session
/// whose Claude Code process still runs on this host
/// ([`enrollment::process_is_live`]). The heartbeat arm covers humans
/// (attend-chat heartbeats the username while open) and sessions on another
/// host; the process arms cover a running Claude session whose heartbeat
/// lapsed, between attend's ticks or through a turn longer than the grace.
/// Pass an empty set where no process scan is available.
pub fn alive(member_id: &str, running: &std::collections::HashSet<String>) -> bool {
    running.contains(member_id)
        || heartbeat::is_fresh(member_id, heartbeat::DEFAULT_GRACE)
        || enrollment::process_is_live(member_id)
}

/// How much later than its record's `startedAt` a session's process may
/// have started and still be the record's: Claude Code writes the record
/// once it is up, and a `/clear` rewrites it later still, so only a
/// process that started well after the record is a later one.
const RECORD_SLACK: std::time::Duration = std::time::Duration::from_secs(60);

/// Whether the process a session record names still runs: a process holds
/// its pid and, where the record and this host both give a start time,
/// started no later than the record's `startedAt` and [`RECORD_SLACK`]. A
/// process that started later was given the pid after the record's exited.
fn record_process_runs(pid: u32, started_at_ms: Option<u64>) -> bool {
    if !process::is_running(pid) {
        return false;
    }
    let recorded = started_at_ms.map(|ms| std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms));
    match (recorded, process::started_at(pid)) {
        (Some(recorded), Some(began)) => began <= recorded + RECORD_SLACK,
        _ => true,
    }
}

/// The ids of the Claude Code sessions whose recorded pid a process holds on
/// this host, listening or not. A pid given to a later process counts too:
/// for a caller that keeps what a session may still read, that errs on the
/// side of keeping.
pub fn running_sessions() -> Vec<String> {
    let mut ids: Vec<String> = session::sessions_dirs()
        .iter()
        .flat_map(|d| claude_sessions::read_session_records(d))
        .filter(|r| process::is_running(r.pid))
        .map(|r| r.session_id)
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

/// Whether `member_id` is a Claude Code session whose process this host saw
/// exit: it is not [`alive`], and either the session records naming it all
/// name processes that no longer run ([`record_process_runs`]), or, with no
/// such record, its enrollment names a process of this host that has
/// exited. A session this host knows nothing of
/// (one on another host, or one that never listened) has not exited. A
/// quiet session that has not exited may still read what is sent to it
/// later; one that has exited never will.
pub fn has_exited(member_id: &str) -> bool {
    if alive(member_id, &std::collections::HashSet::new()) {
        return false;
    }
    let records: Vec<(u32, Option<u64>)> = session::sessions_dirs()
        .iter()
        .flat_map(|d| claude_sessions::read_session_records(d))
        .filter(|r| r.session_id == member_id)
        .map(|r| (r.pid, r.started_at_ms))
        .collect();
    if !records.is_empty() {
        return !records.iter().any(|(pid, started)| record_process_runs(*pid, *started));
    }
    enrollment::process_has_exited(member_id)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    /// `HOME` and `XDG_CACHE_HOME` in a fresh directory, and no
    /// `CLAUDE_CONFIG_DIR`, for `f`.
    fn with_home(f: impl FnOnce(&Path)) {
        let _g = crate::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join(format!("attend-presence-lib-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(root.join(".claude/sessions")).unwrap();
        let saved: Vec<_> = ["HOME", "XDG_CACHE_HOME", "CLAUDE_CONFIG_DIR"].iter().map(|k| (*k, std::env::var_os(k))).collect();
        std::env::set_var("HOME", &root);
        std::env::set_var("XDG_CACHE_HOME", root.join("cache"));
        std::env::remove_var("CLAUDE_CONFIG_DIR");
        f(&root);
        for (k, v) in saved {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
        std::fs::remove_dir_all(&root).ok();
    }

    fn record(home: &Path, name: &str, pid: u32, session: &str, started: Option<SystemTime>) {
        let started = started.map(|t| format!(r#","startedAt":{}"#, t.duration_since(UNIX_EPOCH).unwrap().as_millis())).unwrap_or_default();
        std::fs::write(home.join(".claude/sessions").join(format!("{name}.json")), format!(r#"{{"pid":{pid},"sessionId":"{session}","cwd":"/w"{started}}}"#)).unwrap();
    }

    // Review of #998, finding 3: a session record's pid is judged with the
    // record's start time, so a pid given to a later process does not keep
    // an exited session alive.
    #[test]
    fn a_session_record_is_judged_by_its_pid_and_start_time() {
        with_home(|home| {
            let pid = std::process::id();
            record(home, "a", pid, "current", Some(SystemTime::now()));
            assert!(!crate::has_exited("current"), "this process started before its record");
            record(home, "b", pid, "reused", Some(SystemTime::now() - Duration::from_secs(24 * 3600)));
            assert!(crate::has_exited("reused"), "the record's process started a day ago; this pid's process is later");
            record(home, "c", pid, "undated", None);
            assert!(!crate::has_exited("undated"), "a record with no start time is judged by its pid");
        });
    }
}
