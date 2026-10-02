//! One message through its whole life: sent, delivered once by the
//! Stop-hook drain, recorded in the session's seen-set, never delivered
//! again, and then released to `/purge` by the consumer consult.
//!
//! Written before the #701 restructuring and kept green through it: the
//! delivery semantics it pins are the ones the consolidation must not move.
//! The receiving session joins a channel first, which enrols it (#720).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const SESSION: &str = "lifecycle-session";

fn fixture() -> PathBuf {
    let d = std::env::temp_dir().join(format!("attend-lifecycle-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join(".claude").join("sessions")).unwrap();
    d
}

/// A session record naming this test process: every attend run from here
/// resolves to [`SESSION`], whose ancestry includes this pid.
fn become_session(home: &Path) {
    let pid = std::process::id();
    std::fs::write(
        home.join(".claude").join("sessions").join(format!("{pid}.json")),
        format!(r#"{{"pid":{pid},"sessionId":"{SESSION}","cwd":"/tmp/lifecycle-project"}}"#),
    )
    .unwrap();
}

fn attend(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_attend"))
        .args(args)
        .env("HOME", home)
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("USER", "lifecycle-human")
        .env_remove("CLAUDE_SESSION_ID")
        .output()
        .expect("run attend")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn ok(o: &Output) {
    assert!(o.status.success(), "stderr: {}", String::from_utf8_lossy(&o.stderr));
}

fn signal_files(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| n.ends_with(".signal"))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn send_deliver_once_seen_then_purgeable() {
    let home = fixture();
    let cache = home.join(".cache").join("attend");
    let broadcast = cache.join("signals").join("_broadcast");

    // Send. No session record yet, so the sender is the human at a terminal
    // and the message is someone else's when the session drains.
    ok(&attend(&home, &["send", "lifecycle hello"]));
    let files = signal_files(&broadcast);
    assert_eq!(files.len(), 1, "one signal on #open: {files:?}");
    let key = files[0].clone();

    // The receiving session enrols by joining a channel.
    become_session(&home);
    ok(&attend(&home, &["join", "lifecycle"]));

    // Deliver: the drain shows it once and records it.
    let first = attend(&home, &["inbox", "--drain", "--format", "plain"]);
    ok(&first);
    let out = stdout(&first);
    assert!(out.contains("lifecycle hello"), "first drain must deliver: {out}");
    assert!(out.contains("1 message(s) drained"), "{out}");

    // Seen-set: the session's state names the signal by its filename.
    let state = std::fs::read_to_string(cache.join("state").join(format!("{SESSION}.state")))
        .expect("the drain writes the session's state");
    assert!(state.contains(&format!("seen_signal: {key}")), "seen-set: {state}");

    // Once: the next drain has nothing for it.
    let second = attend(&home, &["inbox", "--drain", "--format", "plain"]);
    ok(&second);
    let out = stdout(&second);
    assert!(!out.contains("lifecycle hello"), "second drain must not redeliver: {out}");
    assert!(out.contains("no pending messages"), "{out}");

    // Purge consult: every live consumer of #open has seen it, so `/purge`
    // may remove it. The consult reads the same cache this process points at.
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CACHE_HOME", home.join(".cache"));
    let consumers = attend_chat::consumers::live_consumer_seen_sets(&cache.join("signals"), None);
    assert!(!consumers.is_empty(), "the draining session is a live consumer");
    assert!(
        consumers.iter().all(|seen| seen.contains(&key)),
        "every live consumer has consumed {key}"
    );
    // The signal itself is durable: reading never deletes it.
    assert_eq!(signal_files(&broadcast), vec![key]);

    std::fs::remove_dir_all(&home).ok();
}
