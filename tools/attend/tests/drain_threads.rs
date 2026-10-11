//! A drained reply names the message it answers, and `attend reply` to a
//! message delivered from a channel stays in that channel (#900, ADR-406
//! item 10). Both through the CLI, as the mod and the Stop hook run it.

mod common;
use common::Fixture;

/// Write a signal from `peer-sess` into `tray`, threaded under `re` when given.
fn put_from_peer(f: &Fixture, tray: &str, id: &str, re: Option<&str>, body: &str) {
    let dir = f.signals().join(tray);
    std::fs::create_dir_all(&dir).unwrap();
    let line = agent_identity::signal::format_signal("claude:peer-sess", "elsewhere", "/tmp/elsewhere", re, body);
    std::fs::write(dir.join(format!("{id}.signal")), line).unwrap();
}

/// `peer-sess` in channel `room`, and alive by its heartbeat.
#[cfg(feature = "sensor-peers")]
fn peer_in(f: &Fixture, room: &str) {
    attend_groups::Groups::new(&f.signals(), "peer-sess").join(room, false).unwrap();
    let hb = f.heartbeat("peer-sess");
    std::fs::create_dir_all(hb.parent().unwrap()).unwrap();
    std::fs::write(hb, "").unwrap();
}

#[test]
fn a_drained_reply_carries_its_parent_in_both_formats() {
    let f = Fixture::new("drain-threads-re");
    f.ok(&["join", "room"]);
    f.drain("plain");
    put_from_peer(&f, "_broadcast", "peer-sess-1-1", Some("me-1-0"), "an answer");
    put_from_peer(&f, "_broadcast", "peer-sess-1-2", None, "a new topic");

    let plain = f.drain("plain");
    let answer = plain.split("\n\n").find(|b| b.contains("an answer")).expect(&plain);
    assert!(answer.contains("  id:      peer-sess-1-1\n  re:      me-1-0\n"), "{answer}");
    let topic = plain.split("\n\n").find(|b| b.contains("a new topic")).expect(&plain);
    assert!(!topic.contains("re:"), "an unthreaded message has no re line: {topic}");

    put_from_peer(&f, "_broadcast", "peer-sess-1-3", Some("me-1-0"), "another answer");
    let hook = f.drain_hook_with("{}");
    assert!(hook.contains("id peer-sess-1-3, re me-1-0"), "{hook}");
}

#[cfg(feature = "sensor-peers")]
#[test]
fn a_reply_to_a_drained_channel_message_stays_in_the_channel() {
    let f = Fixture::new("drain-threads-channel");
    f.ok(&["join", "room"]);
    f.drain("plain");
    peer_in(&f, "room");
    f.put("@room", "peer-sess-1-1", "said in the room", std::time::Duration::ZERO);
    assert!(f.drain("plain").contains("said in the room"));

    let out = f.attend(&["reply", "--json", "answered in the room"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let sent: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(sent["scope"], "channel");
    assert_eq!(sent["re"], "peer-sess-1-1");
    let id = sent["id"].as_str().unwrap();
    assert!(f.signals().join("@room").join(format!("{id}.signal")).is_file(), "written to the channel");
    assert!(!f.signals().join("_broadcast").join(format!("{id}.signal")).exists(), "not to #open");
}
