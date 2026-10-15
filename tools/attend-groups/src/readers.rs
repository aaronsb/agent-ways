//! Who reads the bus: every agent session that may still read it, and the
//! ids each reads as (ADR-404 D3e). attend's delete and tombstone ask here,
//! and so does attend-chat before it offers Tombstone…, so the offer and
//! the verb hold one rule.

use std::collections::HashSet;

/// Every agent session that may still read the bus, and whether it is
/// enrolled. The candidates are the enrolled sessions, the agents with a
/// heartbeat, and the Claude Code sessions whose recorded process runs on
/// this host, listening or not. A candidate stays unless it has gone (`attend_presence::gone`): a
/// running session while it runs, an enrolled one until it has gone, a
/// heartbeat with no record behind it while the heartbeat is fresh. A
/// person in attend-chat heartbeats under their member id and is no
/// candidate.
pub fn live_sessions() -> Vec<(String, bool)> {
    use attend_presence::enrollment;
    let enrolled: HashSet<String> =
        enrollment::recorded_ids().into_iter().filter(|id| !enrollment::sources(id).is_empty()).collect();
    let running: HashSet<String> = attend_presence::running_sessions().into_iter().collect();
    let beating = std::fs::read_dir(attend_presence::heartbeat::heartbeat_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|id| !id.starts_with('.') && !id.ends_with(".lock") && crate::is_agent_member(id));
    let mut ids: Vec<String> =
        enrolled.iter().chain(&running).cloned().chain(beating).collect::<HashSet<_>>().into_iter().collect();
    ids.sort();
    ids.into_iter()
        .filter(|id| {
            running.contains(id)
                || attend_presence::alive(id, &HashSet::new())
                || (enrolled.contains(id) && !attend_presence::gone(id))
        })
        .map(|id| {
            let listening = enrolled.contains(&id);
            (id, listening)
        })
        .collect()
}

/// The ids live session `id` reads as: its own and those it had before
/// each `/clear`.
pub fn reading_ids_of(id: &str) -> Vec<String> {
    attend_presence::enrollment::reading_ids(&identity_of(id, attend_presence::session::origin_path(id)))
}

/// A resolved identity for another session.
pub fn identity_of(id: &str, origin: Option<String>) -> attend_presence::session::SessionIdentity {
    attend_presence::session::SessionIdentity {
        session_id: id.to_string(),
        origin_path: origin.unwrap_or_default(),
        session_resolved: true,
        origin_resolved: true,
        claude_pid: None,
    }
}

/// The live session `session` is read as: itself while it is live, or a
/// live session that had it as an id before a `/clear`. attend refuses to
/// tombstone either (`ReadAs`, `NotExited`), so a session with one is not
/// offered Tombstone….
pub fn read_as_live(session: &str) -> Option<String> {
    live_sessions().into_iter().map(|(live, _)| live).find(|live| live == session || reading_ids_of(live).iter().any(|id| id == session))
}
