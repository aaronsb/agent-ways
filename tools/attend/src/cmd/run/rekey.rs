//! Following a session id that changes under a running `attend run`.
//!
//! Claude Code's `/clear` gives the running process a new session id and
//! rewrites its session record. The old process does two things: it shows
//! every message line the disclosure cooldown still holds, so nothing it
//! has marked seen is lost, and it checkpoints under the old id. Then it
//! re-executes with [`MOVED_FROM`] naming the old id. The new process
//! resolves the new id and moves the session's state across at startup
//! (`crate::util::move_session`), before it reads any of it. A crash before
//! the exec leaves everything on the old id, which is consistent, and the
//! moves are idempotent, so a crash during them is repaired by the next
//! start.

use super::tick::{collect_snapshot, flush_message_lane, reexec};
use crate::sensors::SensorSlot;
use crate::{emit, state};

/// The env var a re-executed run reads: the session id it moved from.
pub(super) const MOVED_FROM: &str = "ATTEND_SESSION_MOVED_FROM";

/// The session id this process belongs to now, when it resolves and
/// differs from `current`. Reads the session records afresh: the
/// process-wide identity is memoized at startup.
pub(super) fn changed_id(current: &str) -> Option<String> {
    let now = attend_presence::session::identity_for_pid(std::process::id());
    (now.resolved() && now.session_id != current).then_some(now.session_id)
}

/// Hand over to a process running under the new id. Returns only if the
/// exec failed; the caller then stops trying for this id.
pub(super) fn hand_over(old: &str, new: &str, slots: &mut [SensorSlot], store: &state::StateStore) {
    emit::log(&format!("session id changed ({old} → {new}); restarting under the new id"));
    flush_message_lane(slots);
    store.checkpoint(&collect_snapshot(slots));
    reexec(MOVED_FROM, old);
}

/// At startup: when this run was re-executed after an id change, move the
/// old id's state to `new`. Returns the old id for the banner.
pub(super) fn complete_move(new: &str, origin: &str) -> Option<String> {
    let old = std::env::var(MOVED_FROM).ok()?;
    std::env::remove_var(MOVED_FROM);
    if old != new && !old.is_empty() {
        crate::util::move_session(&old, new, origin);
    }
    Some(old)
}
