//! The interactive screens in a real terminal: tui-harness launches `ways`
//! in a private tmux pane at 80x25, sends keys, and reads the pane. The
//! golden frames are checked headless in the unit tests; this catches a
//! screen that fails to start, draw, or take keys on a terminal.
//!
//! Each test skips when tmux or the `tui-harness` binary is missing, and
//! fails instead under `TUI_HARNESS_REQUIRE_TMUX=1` (CI). Build the binary
//! first: `cargo build --manifest-path tools/Cargo.toml -p tui-harness`.
//! `tools/ways-cli/shots/verify.sh` takes PNGs of every state.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use tui_harness::{session::helper_binary, tmux_available, Harness, LaunchOptions, Session};

const WAIT: Duration = Duration::from_secs(15);
const SESSION: &str = "bbbbbbbb-0000-4000-8000-000000000002";

fn required() -> bool {
    std::env::var("TUI_HARNESS_REQUIRE_TMUX").is_ok_and(|v| v == "1")
}

/// Whether the harness can run here; a skip names what is missing.
fn harness_or_skip(test: &str) -> bool {
    let missing = if !tmux_available() {
        "tmux is not installed"
    } else if !helper_binary(None).is_ok_and(|p| p.is_file()) {
        "the tui-harness binary is not built (cargo build -p tui-harness)"
    } else {
        return true;
    };
    assert!(!required(), "TUI_HARNESS_REQUIRE_TMUX=1 but {missing}");
    eprintln!("SKIPPED {test}: {missing}");
    false
}

/// A fixture HOME with one project and two sessions in its event log, a
/// harness root, and the tmux sessions to kill when the test ends, even on
/// a panic.
struct Fx {
    root: PathBuf,
    harness: Harness,
    names: Vec<String>,
}

impl Fx {
    fn new(tag: &str) -> Fx {
        let root = std::env::temp_dir().join(format!("ways-tui-smoke-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        for d in [".config", "shop/.git", ".local/state/agent-ways", ".cache", ".local/share"] {
            std::fs::create_dir_all(home.join(d)).unwrap();
        }
        let proj = home.join("shop").display().to_string();
        // Recent days, so the reports' default window holds them.
        let now = agent_fmt::when::now_secs();
        let mut log = String::new();
        for (id, ago) in [("aaaaaaaa-0000-4000-8000-000000000001", 2), (SESSION, 1)] {
            let at = |secs: u64| agent_fmt::when::utc_iso(now - ago * 86_400 + secs);
            log += &format!("{{\"ts\":\"{}\",\"event\":\"session_start\",\"session\":\"{id}\",\"project\":\"{proj}\"}}\n", at(0));
            log += &format!("{{\"ts\":\"{}\",\"event\":\"way_fired\",\"session\":\"{id}\",\"project\":\"{proj}\",\"way\":\"softwaredev/code/testing\",\"trigger\":\"keyword\"}}\n", at(1));
            log += &format!("{{\"ts\":\"{}\",\"event\":\"way_fired\",\"session\":\"{id}\",\"project\":\"{proj}\",\"way\":\"softwaredev/docs/adr\",\"trigger\":\"file\"}}\n", at(60));
        }
        std::fs::write(home.join(".local/state/agent-ways/events.jsonl"), log).unwrap();
        let harness = Harness::new(root.join("th"));
        Fx { root, harness, names: Vec::new() }
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    /// `ways <args>` at 80x25 in the fixture, as session `base`.
    fn launch(&mut self, base: &str, args: &[&str]) -> Session {
        let name = format!("{base}-{}", std::process::id());
        self.names.push(name.clone());
        let home = self.home();
        let env = vec![
            ("PATH".into(), "/usr/bin:/bin".into()),
            ("HOME".into(), home.display().to_string()),
            ("XDG_CONFIG_HOME".into(), home.join(".config").display().to_string()),
            ("XDG_DATA_HOME".into(), home.join(".local/share").display().to_string()),
            ("XDG_STATE_HOME".into(), home.join(".local/state").display().to_string()),
            ("XDG_CACHE_HOME".into(), home.join(".cache").display().to_string()),
            ("XDG_RUNTIME_DIR".into(), self.root.join("run").display().to_string()),
            ("CLAUDE_PROJECT_DIR".into(), home.join("shop").display().to_string()),
            ("LANG".into(), "C.UTF-8".into()),
        ];
        std::fs::create_dir_all(self.root.join("run")).unwrap();
        let opts = LaunchOptions { cols: 80, rows: 25, env, cwd: Some(home.join("shop")), ..LaunchOptions::default() };
        let mut cmd = vec![env!("CARGO_BIN_EXE_ways").to_string()];
        cmd.extend(args.iter().map(|a| a.to_string()));
        self.harness.launch(&name, &opts, &cmd).expect("launch")
    }

    fn project(&self) -> String {
        self.home().join("shop").display().to_string()
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        for n in &self.names {
            let _ = Command::new("tmux")
                .args(["-L", tui_harness::session::TMUX_SOCKET, "kill-session", "-t"])
                .arg(format!("=tui-{n}"))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The session picker lists both sessions; Enter opens the selected one's
/// timeline; a digit switches to the stats tab.
#[test]
fn replay_picker_timeline_and_stats() {
    if !harness_or_skip("replay_picker_timeline_and_stats") {
        return;
    }
    let mut fx = Fx::new("replay");
    let proj = fx.project();
    let s = fx.launch("replay", &["session", "replay", "--project", &proj]);

    let pane = s.wait_for("2 sessions in", WAIT).unwrap();
    assert!(pane.contains("1 sessions") && pane.contains("2 timeline"), "tabs missing:\n{pane}");
    assert!(pane.contains("bbbbbbbb-000") && pane.contains("aaaaaaaa-000"), "a session is missing:\n{pane}");
    assert_eq!(pane.lines().count(), 25, "pane was:\n{pane}");

    s.send(&["Enter"]).unwrap();
    let pane = s.wait_for(" at epoch ", WAIT).unwrap();
    assert!(pane.contains("Session bbbbbbbb-0000"), "timeline opened another session:\n{pane}");
    assert!(pane.contains("/code/testing"), "timeline lacks the first fired way:\n{pane}");
    assert!(pane.contains("replay"), "status line missing:\n{pane}");

    s.send(&["5"]).unwrap();
    let pane = s.wait_for("how they fired", WAIT).unwrap();
    assert!(pane.contains("softwaredev/code/testing"), "stats lacks a way:\n{pane}");
}

/// The matching tab of the settings screens: the tree, the detail pane,
/// and an edit that marks one value changed without writing it.
#[test]
fn settings_matching_edit() {
    if !harness_or_skip("settings_matching_edit") {
        return;
    }
    let mut fx = Fx::new("settings");
    let proj = fx.project();
    let s = fx.launch("settings", &["settings", "matching", "--project", &proj]);

    let pane = s.wait_for("keyword_floor_probability", WAIT).unwrap();
    // At 80 columns the eight tab names are cut to fit, the shown one too.
    assert!(pane.contains("2 match") && pane.contains("detail"), "pane was:\n{pane}");

    s.send(&["Down", "Down", "e", "C-u"]).unwrap();
    s.send(&["-l", "0.2"]).unwrap();
    s.send(&["Enter"]).unwrap();
    let pane = s.wait_for("1 changed", WAIT).unwrap();
    assert!(pane.contains("was      0.15"), "detail lacks the old value:\n{pane}");
    assert!(pane.contains("unsaved in matching"), "pane was:\n{pane}");
    assert!(!fx.home().join(".config/agent-ways/config.yaml").exists(), "an edit wrote the config before apply");
}
