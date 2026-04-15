//! Shared helpers reused across the `cmd::*` subcommand modules.
//!
//! These were free functions in the top of `main.rs` before the
//! dispatcher split (issue #51). Addressing-layer concerns — the
//! signals base path, project-name encoding, own-session resolution,
//! and the `Groups` builder that joins the two — collect here so every
//! command module can import them from a single place instead of
//! reaching into `main`.

use crate::groups;
#[cfg(feature = "sensor-peers")]
use crate::sensors;

pub(crate) fn signals_base() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home)
        .join(".cache")
        .join("attend")
        .join("signals")
}

/// Encode a project path the same way Claude Code does: '/', '_', '.' → '-'
pub(crate) fn encode_project(path: &str) -> String {
    path.chars()
        .map(|c| match c {
            '/' | '_' | '.' => '-',
            _ => c,
        })
        .collect()
}

/// Delegate to the shared implementation in the peer sensor module.
pub(crate) fn own_session_id() -> Option<String> {
    #[cfg(feature = "sensor-peers")]
    {
        sensors::find_own_session_id(std::process::id())
    }
    #[cfg(not(feature = "sensor-peers"))]
    {
        None
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
