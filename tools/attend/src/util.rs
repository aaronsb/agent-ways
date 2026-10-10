//! Shared helpers reused across the `cmd::*` subcommand modules.
//!
//! These were free functions in the top of `main.rs` before the
//! dispatcher split (issue #51). Addressing-layer concerns — the
//! signals base path, project-name encoding, own-session resolution,
//! and the `Groups` builder that joins the two — collect here so every
//! command module can import them from a single place instead of
//! reaching into `main`.

use crate::groups;

pub(crate) fn signals_base() -> std::path::PathBuf {
    attend_presence::cache::signals_dir()
}

/// Where keepwarm's per-session arm file and ledger live (ADR-182).
/// Attend-owned state: the CLI verbs are the only sanctioned readers.
pub(crate) fn keepwarm_dir() -> std::path::PathBuf {
    attend_presence::cache::dir().join("keepwarm")
}

/// Claude Code's per-project data dir. A project is "live" iff its
/// encoded-cwd subdir exists here; message-tray lifetime is bound to it
/// (ADR-136) rather than to a wall-clock age.
pub(crate) fn projects_base() -> std::path::PathBuf {
    claude_sessions::ClaudeDir::user().projects_dir()
}

/// The name of a project's signal tray, `claude_sessions::attend_key`: the
/// project slug, so the project-liveness lookup that reaps trays reads the
/// project directory from it (ADR-136 Decision 3), and a hash of the full
/// path, so two projects with one slug never share a tray.
pub(crate) fn encode_project(path: &str) -> String {
    claude_sessions::attend_key(path)
}

/// Delegate to the canonical identity derivation (issue #378). No
/// longer gated on the sensor-peers feature — a minimal attend build
/// used to degrade own-identity to `pid-<pid>`, which polluted
/// `_groups.yaml` member ids.
pub(crate) fn own_session_id() -> Option<String> {
    attend_presence::session::find_own_session_id(std::process::id())
}

/// The cwd this session is *about* — the session record's origin path
/// (issue #378), falling back to the process cwd only when no Claude
/// session owns this process. Every subcommand that means "my
/// project" (send identity, tray scan, status, registration) resolves
/// through here, so a stray shell `cd` can no longer put the session
/// on the bus as a different persona.
pub(crate) fn own_origin_cwd() -> String {
    attend_presence::session::identity().origin_path
}

/// Move everything attend keeps under session id `old` to `new`, for a
/// session whose id changed under the same Claude Code process (`/clear`):
/// the seen-set and cold-start mark, the enrollment record, the registry
/// slot with its instance name, channel memberships and the last-inbound
/// record. The old heartbeat is cleared. Idempotent: a move already done,
/// or done in part, completes.
pub(crate) fn move_session(old: &str, new: &str, origin: &str) {
    let old_store = attend_state::StateStore::new(Some(old.to_string()));
    if let Some(snapshot) = old_store.load() {
        attend_state::StateStore::new(Some(new.to_string())).checkpoint(&snapshot);
        old_store.clear();
    }
    attend_presence::enrollment::carry(old, new).ok();
    attend_instances::Registry::new().rename(origin, old, new).ok();
    groups::Groups::new(&signals_base(), new).rename_member(old, new);
    let state_dir = attend_presence::cache::state_dir();
    std::fs::rename(state_dir.join(format!("{old}.last-inbound")), state_dir.join(format!("{new}.last-inbound"))).ok();
    attend_presence::heartbeat::clear(old).ok();
}

/// Record that this session enrolled by joining a channel (#720). Only a
/// resolved session enrolls: the drain never runs for any other.
pub(crate) fn enroll_by_join() {
    let ident = attend_presence::session::identity();
    if ident.resolved() {
        attend_presence::enrollment::enroll(&ident.session_id, attend_presence::enrollment::Source::Join).ok();
    }
}

/// Before this session joins channel `name`: mark the channel's old
/// messages seen, as the cold start does for every room (ADR-136
/// Decision 2, ADR-172's cold-start rule), so joining a channel late does
/// not deliver its history. Messages younger than the fresh window stay
/// live. A channel the session is already in is left alone: what it has
/// not consumed there is still owed to it. Returns how many were marked;
/// only a resolved session keeps a seen-set, so any other marks nothing.
pub(crate) fn baseline_joined_room(groups: &groups::Groups, name: &str) -> usize {
    let ident = attend_presence::session::identity();
    if !ident.resolved() || groups.my_groups().iter().any(|(n, _)| n == name) {
        return 0;
    }
    let backlog = attend_state::cold_start::room_backlog(&groups.group_dir(name));
    let n = backlog.len();
    attend_state::StateStore::new(Some(ident.session_id)).mark_seen(backlog);
    n
}

/// What a join prints after its line for the backlog it did not hand over.
pub(crate) fn backlog_note(n: usize) -> String {
    match n {
        0 => String::new(),
        1 => " (1 earlier message not shown; attend inbox)".to_string(),
        n => format!(" ({n} earlier messages not shown; attend inbox)"),
    }
}

/// After this session's channels changed: a session in no channel any more
/// withdraws its join enrollment. One that also ran `attend run` stays
/// enrolled through that.
pub(crate) fn settle_join_enrollment(groups: &groups::Groups) {
    let ident = attend_presence::session::identity();
    if ident.resolved() && groups.my_groups().is_empty() {
        attend_presence::enrollment::withdraw(&ident.session_id, attend_presence::enrollment::Source::Join).ok();
    }
}

pub(crate) fn count_signals(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("signal"))
                .count()
        })
        .unwrap_or(0)
}

pub(crate) fn get_groups() -> groups::Groups {
    let session_id = own_session_id().unwrap_or_else(|| format!("pid-{}", std::process::id()));
    groups::Groups::new(&signals_base(), &session_id)
}
