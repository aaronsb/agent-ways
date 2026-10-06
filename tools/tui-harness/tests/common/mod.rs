//! Shared helpers for the tmux-backed integration tests.

use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Whether this test should run. Skips with a message when tmux is absent,
/// unless `TUI_HARNESS_REQUIRE_TMUX=1`, in which case a missing tmux fails.
pub fn tmux_or_skip(test: &str) -> bool {
    if tui_harness::tmux_available() {
        return true;
    }
    if std::env::var("TUI_HARNESS_REQUIRE_TMUX").is_ok_and(|v| v == "1") {
        panic!("TUI_HARNESS_REQUIRE_TMUX=1 but tmux is not installed");
    }
    eprintln!("SKIPPED {test}: tmux is not installed");
    false
}

/// A pane capture with its soft wraps undone. The pane is a fixed number of
/// columns wide, so a long path (a deep checkout, a long `TMPDIR`) wraps
/// across rows; assertions on a path must not depend on it fitting one.
pub fn unwrapped(pane: &str) -> String {
    pane.replace('\n', "")
}

/// A temporary state root plus the tmux sessions a test made. `Drop` kills
/// the sessions and removes the root, best effort and without asserting, so
/// a failing test still cleans up and its own panic is the one reported.
pub struct Scratch {
    pub root: PathBuf,
    sessions: Vec<String>,
}

impl Scratch {
    pub fn new(tag: &str) -> Scratch {
        let root = std::env::temp_dir().join(format!("tui-harness-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        Scratch {
            root,
            sessions: Vec::new(),
        }
    }

    /// A session name unique to this process, registered for cleanup.
    pub fn name(&mut self, base: &str) -> String {
        let name = format!("{base}-{}", std::process::id());
        self.sessions.push(format!("tui-{name}"));
        name
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        for s in &self.sessions {
            let _ = Command::new("tmux")
                .args([
                    "-L",
                    tui_harness::session::TMUX_SOCKET,
                    "kill-session",
                    "-t",
                ])
                .arg(format!("={s}"))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[allow(dead_code)] // not every test binary uses it
pub fn fixture(name: &str) -> PathBuf {
    // Read at run time: a test binary reused from another checkout keeps the
    // path it was built at.
    std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_else(|| env!("CARGO_MANIFEST_DIR").into()))
        .join("tests/fixtures")
        .join(name)
}
