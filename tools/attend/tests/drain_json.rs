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
    assert_eq!(d["schema"], 1);
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
fn an_unresolved_session_says_so() {
    let f = Fixture::new("drain-json-unresolved");
    f.ok(&["listen", "on"]);
    f.put("_broadcast", "other-1-1-fresh", "fresh open", Duration::ZERO);
    let out = f.without_session(|| f.attend(&["inbox", "--drain", "--json"]));
    assert!(out.status.success());
    let d: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!((d["schema"].as_u64(), d["status"].as_str()), (Some(1), Some("unresolved")), "{d}");
    assert_eq!(d["messages"], serde_json::json!([]));
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

/// The attend mod's tests read the drain and unread documents this binary
/// prints, from `plugins/attend/tests/attend-output.ts`. This test writes
/// those documents from real runs and fails when the committed file no
/// longer matches, so the mod is never tested against a shape attend does
/// not print. `ATTEND_BLESS=1` rewrites the file.
#[test]
fn the_mod_fixtures_are_the_real_attend_output() {
    fn norm(mut v: Value) -> Value {
        if v["you_are"].is_string() {
            v["you_are"] = Value::from("Chaucer-alpha");
        }
        if let Some(ms) = v["messages"].as_array_mut() {
            for m in ms {
                m["when"] = Value::from("21:21");
            }
        }
        v
    }
    let mut out = vec![
        "// Generated from the real attend binary by tools/attend/tests/drain_json.rs".to_string(),
        "// (the_mod_fixtures_are_the_real_attend_output). Do not edit: rerun that test".to_string(),
        "// with ATTEND_BLESS=1. `when` and the session's name are normalized.".to_string(),
        String::new(),
    ];
    let mut put = |name: &str, v: Value| {
        out.push(format!("export const {name} = {}", serde_json::to_string(&v.to_string()).unwrap()));
    };

    let f = Fixture::new("fixtures-cold");
    let unresolved = f.without_session(|| f.attend(&["inbox", "--drain", "--json"]));
    assert!(unresolved.status.success());
    put("DRAIN_UNRESOLVED", norm(serde_json::from_slice(&unresolved.stdout).unwrap()));
    put("DRAIN_NOT_LISTENING", norm(drain_json(&f)));
    f.ok(&["listen", "on"]);
    put("DRAIN_COLD_EMPTY", norm(drain_json(&f)));
    put("DRAIN_EMPTY", norm(drain_json(&f)));

    let f = Fixture::new("fixtures-backlog");
    f.put(&f.project_tray(), "other-1-1-addressed", "addressed while dormant", 60 * MINUTE);
    f.put("_broadcast", "other-1-2-old", "old open backlog", 10 * MINUTE);
    f.put("_broadcast", "other-1-3-fresh", "fresh open", Duration::ZERO);
    f.ok(&["listen", "on"]);
    put("DRAIN_COLD_BACKLOG", norm(drain_json(&f)));

    let f = Fixture::new("fixtures-channel");
    f.ok(&["join", "room"]);
    drain_json(&f);
    f.put("@room", "other-1-4-room", "said in the room", Duration::ZERO);
    let unread = f.ok(&["unread", "--json"]);
    put("UNREAD_ROOM", serde_json::from_str(&unread).unwrap());
    let dir = f.signals().join("_broadcast");
    std::fs::create_dir_all(&dir).unwrap();
    let line = agent_identity::signal::format_signal("claude:other-session", "elsewhere", "/tmp/elsewhere", Some("other-1-4-room"), "an answer in the open");
    std::fs::write(dir.join("other-1-5-answer.signal"), line).unwrap();
    put("DRAIN_ROOM_AND_REPLY", norm(drain_json(&f)));

    let text = out.join("\n") + "\n";
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/attend/tests/attend-output.ts");
    if std::env::var_os("ATTEND_BLESS").is_some() {
        std::fs::write(&path, &text).unwrap();
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(committed, text, "{} is stale; rerun with ATTEND_BLESS=1 and rerun the mod's tests", path.display());
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
