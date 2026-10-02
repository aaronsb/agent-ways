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

/// Record that this session enrolled by joining a channel (#720). Only a
/// resolved session enrolls: the drain never runs for any other.
pub(crate) fn enroll_by_join() {
    let ident = attend_presence::session::identity();
    if ident.resolved() {
        attend_presence::enrollment::enroll(&ident.session_id, attend_presence::enrollment::Source::Join).ok();
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
