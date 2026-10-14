//! Who reads the bus: the live agent sessions, the ids each reads as, and
//! the ids the caller reads as. `delete` and `tombstone` both ask here
//! (ADR-404 D3e).

use std::collections::HashSet;
use std::time::Duration;

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
/// this host, listening or not. A candidate stays unless [`gone`]: a
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
                || (enrolled.contains(id) && !gone(id))
        })
        .map(|id| {
            let listening = enrolled.contains(&id);
            (id, listening)
        })
        .collect()
}

/// How long a session this host cannot judge, with no session record here,
/// holds its mail after its last heartbeat. Its process may run on another
/// host, or under a host name this one no longer has, so only its
/// heartbeat speaks for it (ADR-406 item 12); the window is the one after
/// which cleanup removes a marked message.
pub(crate) const UNJUDGED_HOLD: Duration = Duration::from_secs(crate::cmd::cleanup::MARK_REAP_DAYS * 86_400);

/// Whether session `id` has gone: this host saw its process exit
/// (`attend_presence::has_exited`), or no session record here names it
/// and its last heartbeat, if any, is older than [`UNJUDGED_HOLD`].
pub(crate) fn gone(id: &str) -> bool {
    if attend_presence::has_exited(id) {
        return true;
    }
    if attend_presence::alive(id, &HashSet::new()) || recorded_here(id) {
        return false;
    }
    attend_presence::heartbeat::last_seen(id)
        .is_none_or(|t| std::time::SystemTime::now().duration_since(t).unwrap_or_default() >= UNJUDGED_HOLD)
}

/// Whether a Claude Code session record on this host names `id`.
fn recorded_here(id: &str) -> bool {
    attend_presence::session::sessions_dirs()
        .iter()
        .any(|d| claude_sessions::read_session_records(d).iter().any(|r| r.session_id == id))
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

