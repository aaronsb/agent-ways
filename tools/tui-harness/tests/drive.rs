//! Drive real tmux sessions: launch, send keys, capture text and pixels,
//! stop. Each test skips when tmux is missing (fails under
//! `TUI_HARNESS_REQUIRE_TMUX=1`) and cleans up through a `Drop` guard.

mod common;

use std::process::Command;
use std::time::Duration;

use common::{fixture, tmux_or_skip, Scratch};
use tui_harness::{sgr::BASIC, Harness, LaunchOptions, Renderer};

const WAIT: Duration = Duration::from_secs(10);

#[test]
fn drives_fixture_through_tmux() {
    if !tmux_or_skip("drives_fixture_through_tmux") {
        return;
    }
    let mut scratch = Scratch::new("drive");
    let name = scratch.name("it");
    let harness = Harness::new(&scratch.root);
    let opts = LaunchOptions {
        cols: 40,
        rows: 6,
        ..LaunchOptions::default()
    };
    let cmd = [
        "bash".to_string(),
        fixture("drive.sh").display().to_string(),
    ];
    let session = harness.launch(&name, &opts, &cmd).expect("launch");

    let before = session.wait_for("press a key", WAIT).unwrap();
    assert!(before.starts_with("RED plain"), "pane was:\n{before}");
    // No status or border line eats a row: the pane has every requested row.
    assert_eq!(before.lines().count(), 6, "pane was:\n{before}");

    session.send(&["x"]).unwrap();
    session.wait_for("type a line", WAIT).unwrap();

    // A `;` at the end of an argument is tmux's command separator; it must
    // still reach the pane as a typed character, alone or after text.
    session.send(&["-l", "a;b;"]).unwrap();
    session.send(&[";"]).unwrap();
    session.send(&["Enter"]).unwrap();
    let after = session.wait_for("line:[", WAIT).unwrap();
    assert!(after.contains("got:x"), "pane was:\n{after}");
    assert!(after.contains("line:[a;b;;]"), "pane was:\n{after}");

    let ansi = session.text(true).unwrap();
    assert!(
        ansi.contains('\x1b'),
        "capture lost its SGR escapes: {ansi:?}"
    );

    // The four blue-background spaces sit at columns 10..14 of row 0.
    let renderer = Renderer::without_fonts(8, 16);
    let img = session.capture_image_with(&renderer).unwrap();
    assert_eq!(img.dimensions(), (40 * 8, 6 * 16));
    assert_eq!(img.get_pixel(11 * 8 + 4, 8).0, BASIC[4]);

    let shot = session.shot(Some(&scratch.path("fixture.png"))).unwrap();
    assert!(shot.is_file());

    assert!(harness.list().unwrap().iter().any(|s| s.name == name));
    harness.session(&name).unwrap().down().unwrap();
    assert!(harness.list().unwrap().is_empty());
}

/// Launch through the CLI with `TUI_PROBE` and the working directory set
/// on the caller, and return what the pane printed.
fn launch_probe(scratch: &mut Scratch, base: &str, probe: &str) -> (String, String) {
    let name = scratch.name(base);
    let cwd = scratch.path(base);
    std::fs::create_dir_all(&cwd).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_tui-harness"))
        .arg("--dir")
        .arg(&scratch.root)
        .args(["launch", &name, "--cols", "80", "--rows", "4", "--"])
        .args([
            "bash",
            "-c",
            "echo \"probe=[$TUI_PROBE] cwd=[$(pwd -P)]\"; sleep 60",
        ])
        .env("TUI_PROBE", probe)
        .current_dir(&cwd)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "launch failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = Harness::new(&scratch.root)
        .session(&name)
        .unwrap()
        .wait_for("probe=", WAIT)
        .unwrap();
    (text, cwd.canonicalize().unwrap().display().to_string())
}

#[test]
fn each_session_gets_its_callers_env_and_cwd() {
    if !tmux_or_skip("each_session_gets_its_callers_env_and_cwd") {
        return;
    }
    let mut scratch = Scratch::new("env");
    // The first launch may start the server; the second must not inherit
    // the first caller's environment from it.
    let (one, cwd_one) = launch_probe(&mut scratch, "env-one", "one");
    let (two, cwd_two) = launch_probe(&mut scratch, "env-two", "two");
    assert!(one.contains("probe=[one]"), "pane was:\n{one}");
    assert!(two.contains("probe=[two]"), "pane was:\n{two}");
    assert!(
        one.contains(&format!("cwd=[{cwd_one}]")),
        "pane was:\n{one}"
    );
    assert!(
        two.contains(&format!("cwd=[{cwd_two}]")),
        "pane was:\n{two}"
    );
}

#[test]
fn down_never_kills_another_roots_session() {
    if !tmux_or_skip("down_never_kills_another_roots_session") {
        return;
    }
    let mut scratch = Scratch::new("roots");
    let name = scratch.name("shared");
    let a = Harness::new(scratch.path("a"));
    let b = Harness::new(scratch.path("b"));
    let cmd = ["sleep".to_string(), "60".to_string()];
    let session = a.launch(&name, &LaunchOptions::default(), &cmd).unwrap();

    // Root B holds stale state under the same name, as after a crash.
    let stale = b.sessions_dir().join(&name);
    std::fs::create_dir_all(&stale).unwrap();
    std::fs::copy(a.sessions_dir().join(&name).join("env"), stale.join("env")).unwrap();

    let _ = b.session(&name).unwrap().down();
    assert!(session.alive(), "root B's down killed root A's session");
    assert!(!stale.exists(), "root B's stale state should be removed");
    a.session(&name).unwrap().down().unwrap();
    assert!(!session.alive());
}

#[test]
fn orphans_are_listed_and_pruned_per_root() {
    if !tmux_or_skip("orphans_are_listed_and_pruned_per_root") {
        return;
    }
    let mut scratch = Scratch::new("orphans");
    let mine = scratch.name("orphan-mine");
    let theirs = scratch.name("orphan-theirs");
    let a = Harness::new(scratch.path("a"));
    let b = Harness::new(scratch.path("b"));
    let cmd = ["sleep".to_string(), "60".to_string()];
    let s_mine = a.launch(&mine, &LaunchOptions::default(), &cmd).unwrap();
    let s_theirs = b.launch(&theirs, &LaunchOptions::default(), &cmd).unwrap();

    // Lose both state directories, as a crash between launch and down would.
    std::fs::remove_dir_all(a.sessions_dir().join(&mine)).unwrap();
    std::fs::remove_dir_all(b.sessions_dir().join(&theirs)).unwrap();

    let orphans: Vec<String> = a.orphans().into_iter().map(|o| o.tmux_name).collect();
    assert!(orphans.contains(&format!("tui-{mine}")), "{orphans:?}");
    assert!(!orphans.contains(&format!("tui-{theirs}")), "{orphans:?}");

    let (killed, _) = a.prune().unwrap();
    assert_eq!(killed, vec![format!("tui-{mine}")]);
    assert!(!s_mine.alive());
    assert!(s_theirs.alive(), "root A's prune killed root B's orphan");
}
