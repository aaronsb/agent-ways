//! Removed attend surface stays removed (#692, ADR-505).
//!
//! `send --broadcast` was a no-op and `--focus` an alias of `--channel`. Both
//! are trailing-argument text to clap, so a bare removal would send the flag as
//! message body; the binary must refuse instead. `attend focus` was a
//! deprecated alias of the channel verbs and must not parse.

use std::path::PathBuf;
use std::process::{Command, Output};

fn fixture(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("attend-legacy-removed-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn attend(tag: &str, args: &[&str]) -> (Output, PathBuf) {
    let home = fixture(tag);
    let out = Command::new(env!("CARGO_BIN_EXE_attend"))
        .args(args)
        .env("HOME", &home)
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env_remove("CLAUDE_SESSION_ID")
        .output()
        .expect("run attend");
    (out, home)
}

fn signals_written(home: &std::path::Path) -> usize {
    fn count(dir: &std::path::Path) -> usize {
        std::fs::read_dir(dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| {
                        let p = e.path();
                        if p.is_dir() {
                            count(&p)
                        } else if p.extension().is_some_and(|x| x == "signal") {
                            1
                        } else {
                            0
                        }
                    })
                    .sum()
            })
            .unwrap_or(0)
    }
    count(home)
}

#[test]
fn send_broadcast_is_rejected_and_sends_nothing() {
    let (out, home) = attend("bcast", &["send", "--broadcast", "hello"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "stderr: {err}");
    assert!(err.contains("--broadcast"), "stderr: {err}");
    assert_eq!(signals_written(&home), 0, "nothing may be written");
}

#[test]
fn send_focus_is_rejected_and_sends_nothing() {
    let (out, home) = attend("focus", &["send", "--focus", "deploy", "hello"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "stderr: {err}");
    assert!(err.contains("--focus"), "stderr: {err}");
    assert_eq!(signals_written(&home), 0, "nothing may be written");
}

#[test]
fn reply_broadcast_is_rejected() {
    let (out, _home) = attend("rbcast", &["reply", "--broadcast", "hello"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "stderr: {err}");
    assert!(err.contains("--broadcast"), "stderr: {err}");
}

#[test]
fn focus_subcommand_is_rejected() {
    let (out, _home) = attend("focussub", &["focus", "on", "deploy"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "stderr: {err}");
    assert!(err.contains("unrecognized subcommand"), "stderr: {err}");
}
