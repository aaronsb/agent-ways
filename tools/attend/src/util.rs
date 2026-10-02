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

/// The name of a project's signal tray: Claude Code's project slug
/// (`claude_sessions::project_slug`), so tray creation and the
/// project-liveness lookup that reaps trays can never disagree on a path's
/// encoded name (ADR-136 Decision 3).
pub(crate) fn encode_project(path: &str) -> String {
    claude_sessions::project_slug(path)
}

/// The names of a project's own tray to read: [`encode_project`], then the
/// name attend gave it before adopting Claude Code's slug, when that
/// differs. The old name is read for one release (ADR-504); sends go to
/// the new one only.
pub(crate) fn own_tray_names(path: &str) -> Vec<String> {
    let current = encode_project(path);
    let legacy = claude_sessions::legacy_attend_name(path);
    if legacy == current {
        vec![current]
    } else {
        vec![current, legacy]
    }
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
