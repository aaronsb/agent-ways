//! How launched commands get their environment and directory, and how roots
//! stay out of each other's sessions. Each test skips when tmux is missing
//! (fails under `TUI_HARNESS_REQUIRE_TMUX=1`).

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use common::{tmux_or_skip, Scratch};
use tui_harness::session::TMUX_SOCKET;
use tui_harness::{Harness, LaunchOptions};

const WAIT: Duration = Duration::from_secs(10);

fn cli(root: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_tui-harness"));
    c.arg("--dir").arg(root);
    c
}

fn ok(out: &Output) {
    assert!(
        out.status.success(),
        "exit {:?}\nstdout: {}\nstderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A command that prints a marker and then waits on stdin, using only the
/// shell's builtins, so it needs nothing on PATH.
fn marker_cmd(marker: &str) -> Vec<String> {
    vec![
        "/bin/sh".into(),
        "-c".into(),
        format!("echo {marker}; echo \"cwd=[$(pwd -P)]\"; read -r line"),
    ]
}

fn which(prog: &str) -> PathBuf {
    let out = Command::new("sh")
        .args(["-c", &format!("command -v {prog}")])
        .output()
        .unwrap();
    PathBuf::from(String::from_utf8_lossy(&out.stdout).trim())
}

#[test]
fn a_bash_readonly_name_in_the_environment_launches() {
    if !tmux_or_skip("a_bash_readonly_name_in_the_environment_launches") {
        return;
    }
    let mut scratch = Scratch::new("uid");
    let name = scratch.name("uid");
    let out = cli(&scratch.root)
        .args(["launch", &name, "--cols", "60", "--rows", "4", "--"])
        .args(marker_cmd("uid-ok"))
        .env("UID", "1000")
        .env("SHELLOPTS", "braceexpand")
        .output()
        .unwrap();
    ok(&out);
    Harness::new(&scratch.root)
        .session(&name)
        .unwrap()
        .wait_for("uid-ok", WAIT)
        .unwrap();
}

#[test]
fn a_path_without_rm_launches() {
    if !tmux_or_skip("a_path_without_rm_launches") {
        return;
    }
    let mut scratch = Scratch::new("norm");
    let name = scratch.name("norm");
    // A PATH holding only what launching itself needs.
    let bin = scratch.path("fakebin");
    std::fs::create_dir_all(&bin).unwrap();
    for prog in ["tmux", "setsid"] {
        let real = which(prog);
        if real.as_os_str().is_empty() {
            continue;
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, bin.join(prog)).unwrap();
    }
    let out = cli(&scratch.root)
        .args(["launch", &name, "--cols", "60", "--rows", "4", "--"])
        .args(marker_cmd("norm-ok"))
        .env("PATH", &bin)
        .output()
        .unwrap();
    ok(&out);
    Harness::new(&scratch.root)
        .session(&name)
        .unwrap()
        .wait_for("norm-ok", WAIT)
        .unwrap();
}

#[test]
fn a_cwd_with_tmux_style_syntax_is_entered_exactly() {
    if !tmux_or_skip("a_cwd_with_tmux_style_syntax_is_entered_exactly") {
        return;
    }
    let mut scratch = Scratch::new("cwd");
    let harness = Harness::new(scratch.path("root"));
    for (i, dir) in ["c#[x]", "c##[y]", "c#{session_name}", "c;"]
        .iter()
        .enumerate()
    {
        let name = scratch.name(&format!("cwd{i}"));
        let cwd = scratch.path(dir);
        std::fs::create_dir_all(&cwd).unwrap();
        let opts = LaunchOptions {
            cols: 120,
            rows: 4,
            cwd: Some(cwd.clone()),
            ..LaunchOptions::default()
        };
        let s = harness.launch(&name, &opts, &marker_cmd("cwd-ok")).unwrap();
        let text = s.wait_for("cwd=[", WAIT).unwrap();
        let want = format!("cwd=[{}]", cwd.canonicalize().unwrap().display());
        assert!(
            text.replace('\n', "").contains(&want),
            "{dir}: pane was:\n{text}"
        );
    }
}

#[test]
fn a_command_that_cannot_start_fails_the_launch() {
    if !tmux_or_skip("a_command_that_cannot_start_fails_the_launch") {
        return;
    }
    let mut scratch = Scratch::new("nostart");
    let name = scratch.name("nostart");
    let h = Harness::new(&scratch.root);
    let cmd = ["/no/such/program".to_string(), "arg".to_string()];
    let err = h
        .launch(&name, &LaunchOptions::default(), &cmd)
        .unwrap_err();
    assert!(format!("{err:#}").contains("cannot start"), "{err:#}");
    assert!(!h.sessions_dir().join(&name).exists(), "state left behind");
    // A missing program given as one word fails the same way, by name or
    // by path; a real shell string still runs through the shell.
    for word in ["nosuchprog_xyz", "/no/such/path/prog"] {
        let n = scratch.name(&format!("word{}", word.len()));
        let err = h
            .launch(&n, &LaunchOptions::default(), &[word.to_string()])
            .expect_err(&format!("{word} launched"));
        assert!(
            format!("{err:#}").contains("cannot start"),
            "{word}: {err:#}"
        );
    }
    let shell = scratch.name("shellstr");
    let s = h
        .launch(
            &shell,
            &LaunchOptions::default(),
            &["echo shell-ok; read -r x".to_string()],
        )
        .unwrap();
    s.wait_for("shell-ok", WAIT).unwrap();
    // A command that runs and exits at once is not a failure.
    let quick = scratch.name("quick");
    let cmd = [
        "/bin/sh".to_string(),
        "-c".to_string(),
        "exit 0".to_string(),
    ];
    h.launch(&quick, &LaunchOptions::default(), &cmd).unwrap();
}

#[test]
fn send_text_and_shot_refuse_another_roots_session() {
    if !tmux_or_skip("send_text_and_shot_refuse_another_roots_session") {
        return;
    }
    let mut scratch = Scratch::new("foreign");
    let name = scratch.name("foreign");
    let a = Harness::new(scratch.path("a"));
    let b = Harness::new(scratch.path("b"));
    let theirs = b
        .launch(&name, &LaunchOptions::default(), &marker_cmd("b-ok"))
        .unwrap();
    theirs.wait_for("b-ok", WAIT).unwrap();

    // Root A holds stale state under the same name.
    let stale = a.sessions_dir().join(&name);
    std::fs::create_dir_all(&stale).unwrap();
    std::fs::copy(b.sessions_dir().join(&name).join("env"), stale.join("env")).unwrap();
    let mine = a.session(&name).unwrap();

    assert!(
        mine.send(&["-l", "FROM_A"]).is_err(),
        "send reached root B's session"
    );
    assert!(mine.text(false).is_err(), "text read root B's session");
    assert!(
        mine.shot(Some(&scratch.path("x.png"))).is_err(),
        "shot read root B's session"
    );
    std::thread::sleep(Duration::from_millis(200));
    assert!(!theirs.text(false).unwrap().contains("FROM_A"));
}

#[test]
fn send_flags_cannot_retarget_another_session() {
    if !tmux_or_skip("send_flags_cannot_retarget_another_session") {
        return;
    }
    let mut scratch = Scratch::new("retarget");
    let attacker = scratch.name("attacker");
    let victim = scratch.name("victim");
    let h = Harness::new(&scratch.root);
    let a = h
        .launch(&attacker, &LaunchOptions::default(), &marker_cmd("a-ok"))
        .unwrap();
    let v = h
        .launch(&victim, &LaunchOptions::default(), &marker_cmd("v-ok"))
        .unwrap();
    a.wait_for("a-ok", WAIT).unwrap();
    v.wait_for("v-ok", WAIT).unwrap();

    let target = format!("=tui-{victim}:");
    let _ = a.send(&["--", "-t", &target, "-l", "HIJACK"]);
    let _ = a.send(&["-t", &target, "-l", "HIJACK"]);
    let _ = a.send(&["-l", "-t", &target, "HIJACK"]);
    std::thread::sleep(Duration::from_millis(300));
    let victim_text = v.text(false).unwrap();
    assert!(
        !victim_text.contains("HIJACK"),
        "victim pane:\n{victim_text}"
    );

    // Literal text may begin with a dash.
    a.send(&["-l", "-dash-text"]).unwrap();
    a.wait_for("-dash-text", WAIT).unwrap();
}

fn raw_tmux(args: &[&str]) {
    let _ = Command::new("tmux")
        .args(["-L", TMUX_SOCKET, "-f", "/dev/null"])
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn alive(tmux_name: &str) -> bool {
    Command::new("tmux")
        .args([
            "-L",
            TMUX_SOCKET,
            "has-session",
            "-t",
            &format!("={tmux_name}"),
        ])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[test]
fn prune_and_down_all_never_kill_untagged_sessions() {
    if !tmux_or_skip("prune_and_down_all_never_kill_untagged_sessions") {
        return;
    }
    let mut scratch = Scratch::new("untagged");
    let raw = scratch.name("untagged-raw");
    let unset = scratch.name("untagged-unset");
    let h = Harness::new(&scratch.root);

    // A tui-* session nobody tagged, as from a manual tmux command.
    raw_tmux(&[
        "new-session",
        "-d",
        "-s",
        &format!("tui-{raw}"),
        "sleep",
        "60",
    ]);
    // One of this root's sessions whose tag was unset.
    let s = h
        .launch(&unset, &LaunchOptions::default(), &marker_cmd("u-ok"))
        .unwrap();
    raw_tmux(&[
        "set-option",
        "-u",
        "-t",
        &format!("=tui-{unset}:"),
        "@tui_harness_root",
    ]);

    h.prune(false).unwrap();
    assert!(
        alive(&format!("tui-{raw}")),
        "prune killed an untagged session"
    );
    h.down_all().unwrap();
    assert!(
        alive(&format!("tui-{raw}")),
        "down --all killed an untagged session"
    );
    assert!(s.alive(), "down --all killed this root's untagged session");

    let out = cli(&scratch.root).arg("ls").output().unwrap();
    ok(&out);
    let ls = String::from_utf8_lossy(&out.stdout);
    assert!(
        ls.lines()
            .any(|l| l.starts_with(&raw) && l.contains("untagged")),
        "ls was:\n{ls}"
    );

    // Only an explicit opt-in kills them.
    let out = cli(&scratch.root)
        .args(["prune", "--untagged"])
        .output()
        .unwrap();
    ok(&out);
    assert!(!alive(&format!("tui-{raw}")));
    assert!(!s.alive());
}

/// A PATH whose `setsid` waits before starting tmux, so a launch stays in
/// progress (state written, no session yet) long enough to interfere with.
fn slow_setsid_path(scratch: &Scratch) -> std::ffi::OsString {
    let bin = scratch.path("slowbin");
    std::fs::create_dir_all(&bin).unwrap();
    let real = which("setsid");
    let real = if real.as_os_str().is_empty() {
        // No setsid (macOS): the harness falls back to tmux directly, so
        // slow tmux instead, for the launch's first call only.
        which("tmux")
    } else {
        real
    };
    let name = if which("setsid").as_os_str().is_empty() {
        "tmux"
    } else {
        "setsid"
    };
    let shim = bin.join(name);
    std::fs::write(
        &shim,
        format!("#!/bin/sh\nsleep 2\nexec '{}' \"$@\"\n", real.display()),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    std::env::join_paths(paths).unwrap()
}

/// Start a CLI launch that will sit in progress, and wait until its state
/// is on disk.
fn launch_in_progress(scratch: &mut Scratch, base: &str) -> (String, std::process::Child) {
    let name = scratch.name(base);
    let child = cli(&scratch.root)
        .args(["launch", &name, "--cols", "60", "--rows", "4", "--"])
        .args(marker_cmd("inprog-ok"))
        .env("PATH", slow_setsid_path(scratch))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let environ = scratch.root.join("sessions").join(&name).join("environ");
    let deadline = std::time::Instant::now() + WAIT;
    while !environ.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "launch never wrote its state"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    (name, child)
}

fn assert_launch_survived(scratch: &Scratch, name: &str, child: std::process::Child) {
    let out = child.wait_with_output().unwrap();
    ok(&out);
    let s = Harness::new(&scratch.root).session(name).unwrap();
    s.wait_for("inprog-ok", WAIT).unwrap();
}

#[test]
fn prune_spares_a_launch_in_progress() {
    if !tmux_or_skip("prune_spares_a_launch_in_progress") {
        return;
    }
    let mut scratch = Scratch::new("inprog-prune");
    let (name, child) = launch_in_progress(&mut scratch, "inprog-prune");
    let report = Harness::new(&scratch.root).prune(false).unwrap();
    assert!(
        report.removed.is_empty(),
        "prune removed {:?}",
        report.removed
    );
    assert_launch_survived(&scratch, &name, child);
}

#[test]
fn down_all_spares_a_launch_in_progress() {
    if !tmux_or_skip("down_all_spares_a_launch_in_progress") {
        return;
    }
    let mut scratch = Scratch::new("inprog-down");
    let (name, child) = launch_in_progress(&mut scratch, "inprog-down");
    let downed = Harness::new(&scratch.root).down_all().unwrap();
    assert!(downed.is_empty(), "down --all touched {downed:?}");
    assert_launch_survived(&scratch, &name, child);
}

#[test]
fn a_launch_whose_state_is_removed_midway_fails() {
    if !tmux_or_skip("a_launch_whose_state_is_removed_midway_fails") {
        return;
    }
    let mut scratch = Scratch::new("inprog-gone");
    let (name, child) = launch_in_progress(&mut scratch, "inprog-gone");
    // An explicit down of the name removes the state at once.
    Harness::new(&scratch.root).down(&name).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(!out.status.success(), "the launch reported success");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("removed while it launched"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !alive(&format!("tui-{name}")),
        "the session was left running"
    );
}
