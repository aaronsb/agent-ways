//! Presence on the attend mesh: who this session is, and whether a session
//! is alive.
//!
//! - [`session`]: the canonical identity tuple `(sessionId ∩ origin_path)`
//!   (issue #378, ADR-171).
//! - [`heartbeat`]: the per-session liveness file (ADR-129) and the
//!   duplicate-attend lock.
//! - [`alive`]: the one predicate for "is this member live".
//! - [`cache`]: attend's cache root, under `XDG_CACHE_HOME`.
//! - [`process`]: a pid's parent and command line.
//!
//! Formerly the `attend-session` and `attend-heartbeat` crates (#701,
//! ADR-505), which every consumer depended on together.

pub mod cache;
pub mod heartbeat;
pub mod process;
pub mod session;

/// Serializes the tests that point `HOME` or `XDG_CACHE_HOME` elsewhere.
#[cfg(test)]
pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Whether `member_id` is a live member of the mesh: a Claude session in
/// `running` (the ids of live Claude processes, from
/// `sensor_peers::PeerSensor::live_session_ids`), or any member whose
/// heartbeat is within [`heartbeat::DEFAULT_GRACE`]. The heartbeat arm
/// covers humans (attend-chat heartbeats the username while open) and
/// drain-only sessions; the process arm covers a running Claude session
/// whose attend is between ticks. Pass an empty set where no process scan
/// is available, and only heartbeats count.
pub fn alive(member_id: &str, running: &std::collections::HashSet<String>) -> bool {
    running.contains(member_id) || heartbeat::is_fresh(member_id, heartbeat::DEFAULT_GRACE)
}
