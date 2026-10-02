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
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home)
        .join(".cache")
        .join("attend")
        .join("signals")
}

/// Where keepwarm's per-session arm file and ledger live (ADR-182).
/// Attend-owned state: the CLI verbs are the only sanctioned readers.
pub(crate) fn keepwarm_dir() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home)
        .join(".cache")
        .join("attend")
        .join("keepwarm")
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
    attend_session::find_own_session_id(std::process::id())
}

/// The cwd this session is *about* — the session record's origin path
/// (issue #378), falling back to the process cwd only when no Claude
/// session owns this process. Every subcommand that means "my
/// project" (send identity, tray scan, status, registration) resolves
/// through here, so a stray shell `cd` can no longer put the session
/// on the bus as a different persona.
pub(crate) fn own_origin_cwd() -> String {
    attend_session::identity().origin_path
}

/// Whether the session enrolled in attend (#720): it ran `attend run`,
/// which gives it a slot in its project's instance registry, or it joined
/// a channel. Enrollment is what the Stop-hook drain delivers to; it covers
/// `#open` and the project tray as well as the joined channels.
pub(crate) fn enrolled(ident: &attend_session::SessionIdentity, groups: &groups::Groups) -> bool {
    attend_instances::Registry::new()
        .lookup(&ident.origin_path, &ident.session_id)
        .is_some()
        || !groups.my_groups().is_empty()
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
