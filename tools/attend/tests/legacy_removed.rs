//! Removed attend surface stays removed (#692, ADR-505).
//!
//! `send --broadcast` was a no-op and `--focus` an alias of `--channel`. Both
//! are trailing-argument text to clap, so a bare removal would send the flag as
//! message body; the binary must refuse instead. `attend focus` was a
//! deprecated alias of the channel verbs and must not parse. The flat verbs
//! ADR-413 item 5 kept as aliases for one release are gone too (#982): each
//! is a usage error and writes nothing.

use std::path::PathBuf;
use std::process::{Command, Output};

fn fixture(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("attend-legacy-removed-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    // A session record for this test process: attend resolves its session by
    // walking its ancestors, so every invocation from here is the same member.
    let sessions = d.join(".claude").join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let pid = std::process::id();
    std::fs::write(
        sessions.join(format!("{pid}.json")),
        format!(r#"{{"pid":{pid},"sessionId":"test-session-{tag}","cwd":"/tmp/fixture-{tag}"}}"#),
    )
    .unwrap();
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
    let (out, home) = attend("bcast", &["message", "send", "--broadcast", "hello"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "stderr: {err}");
    assert!(err.contains("--broadcast"), "stderr: {err}");
    assert_eq!(signals_written(&home), 0, "nothing may be written");
}

#[test]
fn send_focus_is_rejected_and_sends_nothing() {
    let (out, home) = attend("focus", &["message", "send", "--focus", "deploy", "hello"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "stderr: {err}");
    assert!(err.contains("--focus"), "stderr: {err}");
    assert_eq!(signals_written(&home), 0, "nothing may be written");
}

// `attend message reply` is compiled out without the peers sensor.
#[cfg(feature = "sensor-peers")]
#[test]
fn reply_broadcast_is_rejected_by_the_guard_and_writes_nothing() {
    let (out, home) = attend("rbcast", &["message", "reply", "--broadcast", "hello"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "stderr: {err}");
    assert!(err.contains("`--broadcast` is not a flag"), "stderr: {err}");
    assert_eq!(signals_written(&home), 0, "nothing may be written");

    // Control: seed a last-inbound from a person in #open so a plain reply can
    // succeed in this fixture (a direct message to them), and show it writes
    // exactly one signal where the guarded one wrote none.
    let state = home.join("cache").join("attend").join("state");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("test-session-rbcast.last-inbound"), "peer-1-1\n#open\nfrom external:peer@kitty\n").unwrap();
    let ok = Command::new(env!("CARGO_BIN_EXE_attend"))
        .args(["message", "reply", "hello"])
        .env("HOME", &home)
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .output()
        .expect("run attend");
    assert!(ok.status.success(), "stderr: {}", String::from_utf8_lossy(&ok.stderr));
    assert_eq!(signals_written(&home), 1, "a plain reply writes one signal");

    // Control: with no flag a fresh fixture fails for a different reason
    // (no inbound to thread against), so the guard text above is the guard's.
    let (ctl, _h) = attend("rctl", &["message", "reply", "hello"]);
    let ctl_err = String::from_utf8_lossy(&ctl.stderr);
    assert!(ctl_err.contains("no prior inbound"), "stderr: {ctl_err}");
    assert!(!ctl_err.contains("is not a flag"), "stderr: {ctl_err}");
}

#[test]
fn positive_control_a_plain_send_writes_one_signal_inside_the_fixture() {
    let (out, home) = attend("plain", &["message", "send", "hello"]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(signals_written(&home), 1, "the fixture must see exactly one signal");
}

#[test]
fn a_typo_d_flag_is_refused_not_broadcast() {
    let (out, home) = attend("typo", &["message", "send", "--chanel", "deploy", "hi"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "stderr: {err}");
    assert!(err.contains("`--chanel` is not a flag"), "stderr: {err}");
    assert_eq!(signals_written(&home), 0);
}

#[test]
fn double_dash_sends_a_flag_shaped_message_as_text() {
    let (out, home) = attend("dashdash", &["message", "send", "--", "--broadcast", "was", "removed"]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(signals_written(&home), 1);
}

#[test]
fn joined_view_is_read_only() {
    // A stale group file (member with no heartbeat) is what `cleanup_stale`
    // would rewrite. The read-only `--joined` view must leave it byte-identical.
    let home = fixture("joined");
    let signals = home.join("cache").join("attend").join("signals");
    std::fs::create_dir_all(&signals).unwrap();
    let groups = signals.join("_groups.yaml");
    std::fs::write(&groups, "ghost:\n  pinned: false\n  members:\n    - dead-member\n").unwrap();
    let before = std::fs::read_to_string(&groups).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_attend"))
        .args(["channel", "--joined"])
        .env("HOME", &home)
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .output()
        .expect("run attend");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(std::fs::read_to_string(&groups).unwrap(), before);
    assert!(String::from_utf8_lossy(&out.stdout).contains("project only"));

    // Control: the full listing does clean up, so the file is a fair probe.
    let full = Command::new(env!("CARGO_BIN_EXE_attend"))
        .args(["channel"])
        .env("HOME", &home)
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .output()
        .expect("run attend");
    assert!(full.status.success());
    assert_ne!(
        std::fs::read_to_string(&groups).unwrap_or_default(),
        before,
        "`attend channel` should have pruned the stale member"
    );
}

#[test]
fn rooms_scene_is_an_error_and_leaves_membership_alone() {
    // transition: removed by #717 (ADR-506)
    let home = fixture("rooms");
    let cfg = home.join("config").join("attend");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(cfg.join("scenes.yaml"), "workroom:\n  rooms: [deploy]\n").unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_attend"))
            .args(args)
            .env("HOME", &home)
            .env("XDG_CACHE_HOME", home.join("cache"))
            .env("XDG_CONFIG_HOME", home.join("config"))
            .output()
            .expect("run attend")
    };
    assert!(run(&["channel", "join", "keepme"]).status.success());
    let before = String::from_utf8_lossy(&run(&["channel", "--joined"]).stdout).to_string();
    assert!(before.contains("#keepme"), "{before}");

    let out = run(&["channel", "scene", "workroom"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "stderr: {err}");
    assert!(err.contains("`rooms:` was renamed `channels:`"), "stderr: {err}");
    let after = String::from_utf8_lossy(&run(&["channel", "--joined"]).stdout).to_string();
    assert_eq!(before, after, "membership must be unchanged");

    let listed = run(&["channel", "scenes"]);
    assert!(!listed.status.success());
}

#[test]
fn channels_pin_and_unpin_round_trip() {
    let home = fixture("pin");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_attend"))
            .args(args)
            .env("HOME", &home)
            .env("XDG_CACHE_HOME", home.join("cache"))
            .env("XDG_CONFIG_HOME", home.join("config"))
            .output()
            .expect("run attend")
    };
    assert!(run(&["channel", "join", "ops"]).status.success());
    assert!(run(&["channel", "pin", "ops"]).status.success());
    let pinned = String::from_utf8_lossy(&run(&["channel", "--joined"]).stdout).to_string();
    assert!(pinned.contains("#ops") && pinned.contains("yes"), "{pinned}");
    assert!(run(&["channel", "unpin", "ops"]).status.success());
    let unpinned = String::from_utf8_lossy(&run(&["channel", "--joined"]).stdout).to_string();
    assert!(unpinned.contains("#ops") && unpinned.contains("no"), "{unpinned}");
}

#[test]
fn focus_subcommand_is_rejected() {
    let (out, _home) = attend("focussub", &["focus", "on", "deploy"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "stderr: {err}");
    assert!(err.contains("unrecognized subcommand"), "stderr: {err}");
}

/// The flat verbs ADR-413 moved under a group, kept as hidden aliases for one
/// release and removed by #982.
const FLAT_VERBS: &[&str] = &[
    "send", "reply", "inbox", "unread", "cleanup", "join", "leave", "channels", "dissolve", "scene", "scenes", "peers",
    "whoami", "status", "sensors", "permissions", "tune",
];

#[test]
fn each_removed_flat_verb_is_a_usage_error_and_writes_nothing() {
    for verb in FLAT_VERBS {
        let (out, home) = attend(&format!("flat-{verb}"), &[verb, "hi"]);
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "`attend {verb} hi` stderr: {err}");
        assert!(err.contains("unrecognized subcommand"), "`attend {verb} hi` stderr: {err}");
        assert!(!err.contains("is now `attend"), "`attend {verb}` prints no alias notice: {err}");
        assert_eq!(signals_written(&home), 0, "`attend {verb} hi` wrote a signal");
    }
}

#[test]
fn removed_flat_verbs_are_not_in_the_help() {
    let (out, _home) = attend("flat-help", &["--help"]);
    let help = String::from_utf8_lossy(&out.stdout);
    for verb in FLAT_VERBS {
        assert!(!help.lines().any(|l| l.trim_start().starts_with(&format!("{verb} "))), "{verb} listed:\n{help}");
    }
}
