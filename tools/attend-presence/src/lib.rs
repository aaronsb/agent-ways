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

/// Whether `member_id` is a Claude Code session whose process this host saw
/// exit: it is not [`alive`], and either the session records naming it all
/// name pids no process holds, or, with no such record, its enrollment
/// names a process that has exited. A session this host knows nothing of
/// (one on another host, or one that never listened) has not exited. A
/// quiet session that has not exited may still read what is sent to it
/// later; one that has exited never will.
pub fn has_exited(member_id: &str) -> bool {
    if alive(member_id, &std::collections::HashSet::new()) {
        return false;
    }
    let records: Vec<u32> = session::sessions_dirs()
        .iter()
        .flat_map(|d| claude_sessions::read_session_records(d))
        .filter(|r| r.session_id == member_id)
        .map(|r| r.pid)
        .collect();
    if !records.is_empty() {
        return !records.iter().any(|pid| process::is_running(*pid));
    }
    enrollment::process_has_exited(member_id)
}
