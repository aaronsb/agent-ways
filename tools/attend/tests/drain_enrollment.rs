//! The Stop-hook drain delivers only to a session enrolled in attend (#720).
//!
//! A session is enrolled when it ran `attend run` (it holds a slot in its
//! project's instance registry) or joined a channel. For any other session
//! the drain is a silent no-op: it delivers nothing, from any tray, and
//! writes nothing, so the session does not look alive to peers or to
//! `/purge`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

mod common;

const ORIGIN: &str = "/tmp/drain-enrollment-project";

fn session(tag: &str) -> String {
    format!("drain-{tag}")
}

/// An isolated HOME with a session record naming this test process, and one
/// message from another session waiting in each tray the drain can read:
/// `#open`, the project's own tray, and the `@side` channel.
fn fixture(tag: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!("attend-drain-enrol-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    let sessions = home.join(".claude").join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let pid = std::process::id();
    std::fs::write(
        sessions.join(format!("{pid}.json")),
        format!(r#"{{"pid":{pid},"sessionId":"{}","cwd":"{ORIGIN}"}}"#, session(tag)),
    )
    .unwrap();

    let signals = signals(&home);
    let trays = [
        ("_broadcast".to_string(), "open-msg"),
        (claude_sessions::attend_key(ORIGIN), "project-msg"),
        ("@side".to_string(), "channel-msg"),
    ];
    for (tray, body) in trays {
        let dir = signals.join(&tray);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("other-session-1-{body}.signal")),
            format!("claude:other-session|elsewhere|/tmp/elsewhere|{body}\n"),
        )
        .unwrap();
    }
    home
}

fn cache(home: &Path) -> PathBuf {
    home.join(".cache").join("attend")
}

fn signals(home: &Path) -> PathBuf {
    cache(home).join("signals")
}

fn attend(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_attend"))
        .args(args)
        .env("HOME", home)
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env_remove("CLAUDE_SESSION_ID")
        .stdin(Stdio::null())
        .output()
        .expect("run attend")
}

fn drain(home: &Path, format: &str) -> String {
    let out = attend(home, &["inbox", "--drain", "--format", format]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn heartbeat(home: &Path, tag: &str) -> PathBuf {
    cache(home).join("heartbeat").join(session(tag))
}

fn state_file(home: &Path, tag: &str) -> PathBuf {
    cache(home).join("state").join(format!("{}.state", session(tag)))
}

#[test]
fn an_unenrolled_session_drains_nothing_and_writes_no_heartbeat() {
    let home = fixture("unenrolled");

    let plain = drain(&home, "plain");
    for body in ["open-msg", "project-msg", "channel-msg"] {
        assert!(!plain.contains(body), "delivered {body} to an unenrolled session: {plain}");
    }
    assert!(plain.is_empty(), "the no-op is silent: {plain:?}");
    assert_eq!(drain(&home, "hook"), "", "the hook form says nothing either");

    assert!(!heartbeat(&home, "unenrolled").exists(), "an unenrolled drain must not look alive");
    assert!(!state_file(&home, "unenrolled").exists(), "an unenrolled drain records no consumption");
    std::fs::remove_dir_all(&home).ok();
}

#[test]
fn a_session_in_a_channel_drains_every_tray_as_before() {
    let home = fixture("joined");
    assert!(attend(&home, &["join", "side"]).status.success());

    let plain = drain(&home, "plain");
    for body in ["open-msg", "project-msg", "channel-msg"] {
        assert!(plain.contains(body), "missing {body}: {plain}");
    }
    assert!(plain.contains("3 message(s) drained"), "{plain}");
    assert!(heartbeat(&home, "joined").exists(), "an enrolled drain keeps the session alive");
    assert!(state_file(&home, "joined").exists());
    assert!(drain(&home, "plain").contains("no pending messages"));
    std::fs::remove_dir_all(&home).ok();
}

#[test]
fn a_session_that_ran_attend_drains_every_tray_as_before() {
    // `attend run` enrolls the session; once it has stopped, the drain is
    // the conduit and delivers everything the session receives.
    let f = common::Fixture::new("drain-ran");
    {
        let run = f.run();
        run.wait_for("enrollment by attend run", std::time::Duration::from_secs(20), |_| {
            f.marker(&f.sid).exists()
        });
    }
    let tray = f.project_tray();
    f.put("_broadcast", "other-1-open-msg", "open-msg", std::time::Duration::ZERO);
    f.put(&tray, "other-1-project-msg", "project-msg", std::time::Duration::ZERO);
    f.put("@side", "other-1-channel-msg", "channel-msg", std::time::Duration::ZERO);

    let plain = f.drain("plain");
    // Not in `@side`, so the channel tray is not read.
    for body in ["open-msg", "project-msg"] {
        assert!(plain.contains(body), "missing {body}: {plain}");
    }
    assert!(!plain.contains("channel-msg"), "{plain}");
    assert!(f.heartbeat(&f.sid).exists());
}
