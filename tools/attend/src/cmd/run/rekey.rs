//! Following a session id that changes under a running `attend run`.
//!
//! Claude Code's `/clear` gives the running process a new session id and
//! rewrites its session record. Everything attend keeps is keyed on the id
//! (the registry slot, the enrollment record, the seen-set, channel
//! memberships, the heartbeat), so a run that kept the startup id would
//! leave the drain, which resolves afresh, looking at an unenrolled
//! session. On a change the run moves that state to the new id and
//! re-executes itself, so every part of the loop starts again keyed on the
//! new id.

use super::tick::{collect_snapshot, reexec};
use crate::sensors::SensorSlot;
use crate::util::signals_base;
use crate::{emit, groups, state};

/// The env var a re-executed run reads to announce the move.
pub(super) const MOVED_FROM: &str = "ATTEND_SESSION_MOVED_FROM";

/// The session id this process belongs to now, when it resolves and
/// differs from `current`. Reads the session records afresh: the
/// process-wide identity is memoized at startup.
pub(super) fn changed_id(current: &str) -> Option<String> {
    let now = attend_presence::session::identity_for_pid(std::process::id());
    (now.resolved() && now.session_id != current).then_some(now.session_id)
}

/// Move `old`'s state to `new` and re-execute. Returns only if the exec
/// failed, after which the loop carries on under the old id.
pub(super) fn follow(old: &str, new: &str, origin: &str, slots: &[SensorSlot], store: &state::StateStore) {
    emit::log(&format!("session id changed ({old} → {new}); moving state and restarting"));

    // The seen-set: what this run's sensors hold, unioned with whatever
    // the new id already has on disk.
    let snapshot = collect_snapshot(slots);
    store.checkpoint(&snapshot);
    state::StateStore::new(Some(new.to_string())).checkpoint(&snapshot);
    store.clear();

    attend_presence::enrollment::carry(old, new).ok();
    attend_instances::Registry::new().rename(origin, old, new).ok();
    groups::Groups::new(&signals_base(), new).rename_member(old, new);
    let state_dir = attend_presence::cache::state_dir();
    std::fs::rename(
        state_dir.join(format!("{old}.last-inbound")),
        state_dir.join(format!("{new}.last-inbound")),
    )
    .ok();
    attend_presence::heartbeat::clear(old).ok();

    reexec(MOVED_FROM, &format!("{old} → {new}"));
}
