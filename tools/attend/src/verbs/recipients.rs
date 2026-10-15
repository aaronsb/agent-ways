//! Who reads the bus: the live agent sessions and the ids each reads as
//! (`attend_groups::readers`, which attend-chat's Tombstone… offer asks
//! too), and the ids the caller reads as. `delete` and `tombstone` both ask
//! here (ADR-404 D3e).

/// The ids the caller reads as: a Claude Code session's id and the ids it
/// had before each `/clear`, or outside a session the member id of the
/// person at the terminal.
pub(crate) fn caller_ids() -> Vec<String> {
    let me = crate::util::current_session();
    if me.resolved() {
        attend_presence::enrollment::reading_ids(&me)
    } else {
        person_ids()
    }
}

/// The ids the person at the terminal reads as: their member id, with no
/// walk up the process tree to a Claude Code session (`--as-person`).
pub(crate) fn person_ids() -> Vec<String> {
    vec![agent_identity::sanitize_id_component(&agent_identity::signal::user_name())]
}

pub(crate) use attend_groups::readers::{identity_of, live_sessions, reading_ids_of};

