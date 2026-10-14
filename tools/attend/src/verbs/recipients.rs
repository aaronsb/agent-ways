//! Who reads the bus: the live agent sessions, the ids each reads as, and
//! the ids the caller reads as. `delete` and `tombstone` both ask here
//! (ADR-404 D3e).

use std::collections::HashSet;

/// The ids the caller reads as: a Claude Code session's id and the ids it
/// had before each `/clear`, or outside a session the member id of the
/// person at the terminal.
pub(crate) fn caller_ids() -> Vec<String> {
    let me = crate::util::current_session();
    if me.resolved() {
        attend_presence::enrollment::reading_ids(&me)
    } else {
        vec![agent_identity::sanitize_id_component(&agent_identity::signal::user_name())]
    }
}

/// Every agent session that may still read the bus, and whether it is
/// enrolled. The candidates are the enrolled sessions, the agents with a
/// heartbeat, and the Claude Code sessions whose recorded process runs on
/// this host, listening or not. A candidate stays unless it has gone (`attend_presence::gone`): a
/// running session while it runs, an enrolled one until it has gone, a
/// heartbeat with no record behind it while the heartbeat is fresh. A
/// person in attend-chat heartbeats under their member id and is no
/// candidate.
pub(crate) fn live_sessions() -> Vec<(String, bool)> {
    use attend_presence::enrollment;
    let enrolled: HashSet<String> =
        enrollment::recorded_ids().into_iter().filter(|id| !enrollment::sources(id).is_empty()).collect();
    let running: HashSet<String> = attend_presence::running_sessions().into_iter().collect();
    let beating = std::fs::read_dir(attend_presence::heartbeat::heartbeat_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|id| !id.starts_with('.') && !id.ends_with(".lock") && attend_groups::is_agent_member(id));
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
pub(crate) fn reading_ids_of(id: &str) -> Vec<String> {
    attend_presence::enrollment::reading_ids(&identity_of(id, attend_presence::session::origin_path(id)))
}

/// A resolved identity for another session.
pub(crate) fn identity_of(id: &str, origin: Option<String>) -> attend_presence::session::SessionIdentity {
    attend_presence::session::SessionIdentity {
        session_id: id.to_string(),
        origin_path: origin.unwrap_or_default(),
        session_resolved: true,
        origin_resolved: true,
        claude_pid: None,
    }
}

