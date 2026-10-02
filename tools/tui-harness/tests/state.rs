//! State-directory hygiene that needs no tmux: an interrupted launch must
//! not leave the caller's environment on disk where nothing can find it.

use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, SystemTime};

fn cli(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tui-harness"))
        .arg("--dir")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
}

/// Backdate a file past the window a launch in progress may hold it.
fn age(path: &Path) {
    let old = SystemTime::now() - Duration::from_secs(3600);
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(old)
        .unwrap();
}

#[test]
fn an_interrupted_launch_is_listed_and_pruned() {
    let root = std::env::temp_dir().join(format!("tui-harness-state-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);

    // Killed before writing anything but the environment file.
    let bare = root.join("sessions/kk-bare-unused");
    std::fs::create_dir_all(&bare).unwrap();
    std::fs::write(bare.join("environ"), b"SECRET=1\0").unwrap();
    age(&bare.join("environ"));

    // Killed after writing its metadata, with no session ever started.
    let meta = root.join("sessions/kk-meta-unused");
    std::fs::create_dir_all(&meta).unwrap();
    std::fs::write(
        meta.join("env"),
        "TMUX_NAME=tui-kk-meta-unused\nCOLS=80\nROWS=24\nFONT=JetBrains Mono\nFONT_SIZE=14\nCMD=sleep 1\n",
    )
    .unwrap();
    std::fs::write(meta.join("environ"), b"SECRET=1\0").unwrap();
    age(&meta.join("environ"));

    let ls = cli(&root, &["ls"]);
    let ls_out = String::from_utf8_lossy(&ls.stdout).into_owned();
    let found = |name: &str, state: &str| {
        ls_out
            .lines()
            .any(|l| l.starts_with(name) && l.contains(state))
    };
    assert!(found("kk-bare-unused", "stale"), "ls was:\n{ls_out}");
    // An environment file still on disk marks the launch as interrupted.
    assert!(found("kk-meta-unused", "stale"), "ls was:\n{ls_out}");

    let prune = cli(&root, &["prune"]);
    let prune_out = String::from_utf8_lossy(&prune.stdout).into_owned();
    assert!(prune.status.success(), "prune: {prune_out}");
    assert!(!bare.exists(), "prune left {}: {prune_out}", bare.display());
    assert!(!meta.exists(), "prune left {}: {prune_out}", meta.display());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_environment_that_cannot_round_trip_is_refused() {
    use tui_harness::{Harness, LaunchOptions};
    let root = std::env::temp_dir().join(format!("tui-harness-nul-{}", std::process::id()));
    let h = Harness::new(&root);
    let cmd = [
        "/bin/sh".to_string(),
        "-c".to_string(),
        "exit 0".to_string(),
    ];
    let cases: [(&str, &str); 4] = [
        ("NULV", "before\0INJECTED=yes"),
        ("NU\0LK", "v"),
        ("", "v"),
        ("A=B", "v"),
    ];
    for (k, v) in cases {
        let opts = LaunchOptions {
            env: vec![(k.to_string(), v.to_string())],
            ..LaunchOptions::default()
        };
        let err = h
            .launch("nul-check", &opts, &cmd)
            .expect_err(&format!("{k:?}={v:?} was accepted"));
        let msg = format!("{err:#}");
        assert!(msg.contains("environment variable"), "{k:?}={v:?}: {msg}");
        assert!(
            !root.join("sessions/nul-check").exists(),
            "state left for {k:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}
