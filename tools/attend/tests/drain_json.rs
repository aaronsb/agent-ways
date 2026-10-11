//! `attend inbox --drain --json`: the drain for a program such as the attend
//! mod (ADR-406). The messages, the cold start's held-back count and note,
//! and the session's name come in fields of their own, so a reader never
//! mistakes the note or the name for a message.

mod common;
use common::{Fixture, MINUTE};
use serde_json::Value;
use std::time::Duration;

fn drain_json(f: &Fixture) -> Value {
    let out = f.attend(&["inbox", "--drain", "--json"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)))
}

#[test]
fn a_cold_start_with_nothing_pending_has_no_messages_and_names_the_session() {
    let f = Fixture::new("drain-json-empty");
    f.ok(&["listen", "on"]);
    let d = drain_json(&f);
    assert_eq!(d["status"], "drained");
    assert_eq!(d["messages"], serde_json::json!([]));
    assert_eq!(d["held_back"], 0);
    assert!(d["you_are"].as_str().is_some_and(|n| !n.is_empty()), "the cold start names the session: {d}");

    // The plain drain prints the name as a line of its own, which a reader
    // of the text could take for a message; the JSON keeps it apart.
    let again = drain_json(&f);
    assert_eq!(again["messages"], serde_json::json!([]));
    assert!(again["you_are"].is_null(), "only the cold start names the session: {again}");
}

#[test]
fn a_cold_start_with_backlog_gives_messages_note_and_count_apart() {
    let f = Fixture::new("drain-json-backlog");
    f.put(&f.project_tray(), "other-1-1-addressed", "addressed while dormant", 60 * MINUTE);
    f.put("_broadcast", "other-1-2-old", "old open backlog", 10 * MINUTE);
    f.put("_broadcast", "other-1-3-fresh", "fresh open", Duration::ZERO);
    f.ok(&["listen", "on"]);

    let d = drain_json(&f);
    let bodies: Vec<&str> = d["messages"].as_array().unwrap().iter().map(|m| m["message"].as_str().unwrap()).collect();
    assert_eq!(bodies, ["addressed while dormant", "fresh open"], "{d}");
    assert_eq!(d["held_back"], 1);
    assert!(d["note"].as_str().is_some_and(|n| n.contains("held back")), "{d}");
    assert!(d["you_are"].is_string());
    for m in d["messages"].as_array().unwrap() {
        assert!(!m["message"].as_str().unwrap().contains("you are"), "the name is not in a message: {m}");
        assert!(!m["message"].as_str().unwrap().contains("held back"), "the note is not in a message: {m}");
    }
    let first = &d["messages"][0];
    assert_eq!(first["scope"], "project");
    assert_eq!(first["id"], "other-1-1-addressed");
    assert!(first["re"].is_null());

    let again = drain_json(&f);
    assert_eq!(again["messages"], serde_json::json!([]), "drained messages are marked read");
}

#[test]
fn a_session_not_listening_says_so() {
    let f = Fixture::new("drain-json-off");
    f.put("_broadcast", "other-1-1-fresh", "fresh open", Duration::ZERO);
    let d = drain_json(&f);
    assert_eq!(d["status"], "not_listening");
    assert_eq!(d["messages"], serde_json::json!([]));
    assert!(!f.state_file(&f.sid).exists(), "and marks nothing read");
}

#[test]
fn a_threaded_message_carries_its_parent() {
    let f = Fixture::new("drain-json-re");
    f.ok(&["listen", "on"]);
    drain_json(&f);
    let dir = f.signals().join("_broadcast");
    std::fs::create_dir_all(&dir).unwrap();
    let line = agent_identity::signal::format_signal("claude:peer-sess", "elsewhere", "/tmp/elsewhere", Some("me-1-0"), "an answer");
    std::fs::write(dir.join("peer-sess-1-1.signal"), line).unwrap();
    let d = drain_json(&f);
    assert_eq!(d["messages"][0]["re"], "me-1-0", "{d}");
    assert_eq!(d["messages"][0]["message"], "an answer");
}
