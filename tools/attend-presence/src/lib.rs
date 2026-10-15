//! Presence on the attend mesh: who this session is, and whether a session
//! is alive.
//!
//! - [`session`]: the canonical identity tuple `(sessionId ∩ origin_path)`
//!   (issue #378, ADR-171).
//! - [`heartbeat`]: the per-session liveness file (ADR-129) and the
//!   duplicate-attend lock.
//! - [`alive`]: the one predicate for "is this member live", and [`gone`]
//!   for "has this session gone for good".
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
pub(crate) fn record_process_runs(pid: u32, started_at_ms: Option<u64>) -> bool {
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

/// Days a delete's mark stands before cleanup removes the message it marks
/// (ADR-404 D3e), and for as long, after its last heartbeat, a session this
/// host cannot judge holds its mail ([`gone`]).
pub const MARK_REAP_DAYS: u64 = 7;

/// How long a session this host cannot judge, with no session record here,
/// holds its mail after its last heartbeat. Its process may run on another
/// host, or under a host name this one no longer has, so only its
/// heartbeat speaks for it (ADR-406 item 12); the window is the one after
/// which cleanup removes a marked message.
pub const UNJUDGED_HOLD: std::time::Duration = std::time::Duration::from_secs(MARK_REAP_DAYS * 86_400);

/// Whether session `id` has gone: this host saw its process exit
/// ([`has_exited`]), or no session record here names it and its last
/// heartbeat, if any, is older than [`UNJUDGED_HOLD`]. attend's tombstone
/// accepts only a session that has gone, and attend-chat offers Tombstone…
/// for the same sessions (ADR-404 D3e).
pub fn gone(id: &str) -> bool {
    if has_exited(id) {
        return true;
    }
    if alive(id, &std::collections::HashSet::new()) || recorded_here(id) {
        return false;
    }
    heartbeat::last_seen(id).is_none_or(|t| std::time::SystemTime::now().duration_since(t).unwrap_or_default() >= UNJUDGED_HOLD)
}

/// Whether a Claude Code session record on this host names `id`.
fn recorded_here(id: &str) -> bool {
    session::sessions_dirs().iter().any(|d| claude_sessions::read_session_records(d).iter().any(|r| r.session_id == id))
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

    /// The process key an add records for another session is that
    /// session's only while the record's pid is still its Claude Code
    /// process (ADR-404 D1): pid 1 runs, and is no Claude Code process.
    #[test]
    fn an_add_keys_only_a_claude_code_process_its_record_still_names() {
        with_home(|home| {
            record(home, "a", 1, "not-claude", Some(SystemTime::now()));
            assert_eq!(crate::session::claude_key_of("not-claude"), None);
        });
    }

    /// Another session's earlier id is found by the process its record
    /// names, Claude Code or not, as the session finds it from inside.
    #[test]
    fn a_sessions_process_key_is_its_records_whatever_the_process_runs() {
        with_home(|home| {
            let pid = std::process::id();
            record(home, "a", pid, "this-test", Some(SystemTime::now()));
            let key = format!("{pid}-{}@{}", crate::process::start_time(pid).unwrap(), crate::process::host_name());
            assert_eq!(crate::session::process_key_of("this-test"), Some(key));
            assert_eq!(crate::session::claude_key_of("this-test"), None, "this test is no Claude Code process");
            assert_eq!(crate::session::process_key_of("nobody"), None);
        });
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

    /// A session has gone when this host saw it exit, or when nothing here
    /// records it and its heartbeat, if any, is past the hold.
    #[test]
    fn a_session_has_gone_when_it_exited_or_nothing_here_speaks_for_it() {
        with_home(|home| {
            let pid = std::process::id();
            record(home, "a", pid, "running", Some(SystemTime::now()));
            assert!(!crate::gone("running"), "its process runs");
            record(home, "b", pid, "exited", Some(SystemTime::now() - Duration::from_secs(24 * 3600)));
            assert!(crate::gone("exited"), "its record's process exited");
            assert!(crate::gone("never-seen"), "no record, no heartbeat");
            crate::heartbeat::touch("away").unwrap();
            let file = std::fs::File::options().write(true).open(crate::heartbeat::heartbeat_path("away")).unwrap();
            file.set_modified(SystemTime::now() - Duration::from_secs(3600)).unwrap();
            assert!(!crate::gone("away"), "no record here, heartbeat within the hold: another host may hold it");
            file.set_modified(SystemTime::now() - crate::UNJUDGED_HOLD - Duration::from_secs(60)).unwrap();
            assert!(crate::gone("away"), "heartbeat past the hold");
        });
    }

    /// A session whose record held only its exited process stays gone
    /// once the sweep prunes that record: the prune takes its heartbeat
    /// too, so the heartbeat's hold does not speak for it.
    #[test]
    fn a_pruned_record_takes_its_heartbeat_and_the_session_stays_gone() {
        with_home(|_| {
            let mut child = std::process::Command::new("true").spawn().unwrap();
            child.wait().unwrap();
            let dir = crate::enrollment::records_dir();
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("left"), format!("claude {}-1@{}\n", child.id(), crate::process::host_name())).unwrap();
            crate::heartbeat::touch("left").unwrap();
            let file = std::fs::File::options().write(true).open(crate::heartbeat::heartbeat_path("left")).unwrap();
            file.set_modified(SystemTime::now() - Duration::from_secs(3600)).unwrap();
            assert!(crate::gone("left"), "its process exited");
            crate::enrollment::prune_exited(|_| false);
            assert!(!dir.join("left").exists(), "the record is pruned");
            assert!(crate::gone("left"), "and the session is still gone");
        });
    }
}
