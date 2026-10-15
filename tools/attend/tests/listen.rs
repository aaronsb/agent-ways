//! `attend listen` and `attend message unread` through the CLI (ADR-406 item 10).
//!
//! Listening is what the Stop-hook drain delivers to. A session listens
//! without joining a channel through `attend listen on`; while it does not
//! listen the drain delivers nothing and marks nothing read. `attend message unread`
//! counts what the drain would deliver, and reads only.

mod common;
use common::{Fixture, MINUTE};
use serde_json::{json, Value};
use std::time::Duration;

fn doc(f: &Fixture, args: &[&str]) -> Value {
    let out = f.attend(args);
    assert!(out.status.success(), "attend {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)))
}

/// Mail addressed to the project an hour ago, `#open` backlog ten minutes
/// old, and a live `#open` message.
fn backlog(f: &Fixture, tag: &str) {
    f.put(&f.project_tray(), &format!("other-1-{tag}-addressed"), &format!("{tag} addressed"), 60 * MINUTE);
    f.put("_broadcast", &format!("other-1-{tag}-old"), &format!("{tag} old open"), 10 * MINUTE);
    f.put("_broadcast", &format!("other-1-{tag}-fresh"), &format!("{tag} fresh open"), Duration::ZERO);
}

#[test]
fn a_session_that_never_listened_is_delivered_nothing() {
    let f = Fixture::new("listen-never");
    backlog(&f, "a");
    assert_eq!(f.ok(&["listen", "status"]), "not listening\n");
    assert_eq!(f.ok(&["listen"]), "not listening\n", "status is the default");
    assert_eq!(f.drain("plain"), "", "the drain is silent");
    assert!(!f.state_file(&f.sid).exists(), "and marks nothing read");
}

#[test]
fn listen_on_delivers_what_was_addressed_while_dormant() {
    let f = Fixture::new("listen-on");
    backlog(&f, "a");
    assert_eq!(f.ok(&["listen", "on"]), "listening\n");
    let state = doc(&f, &["listen", "status", "--json"]);
    assert_eq!(state["listening"], true);
    assert_eq!(state["via"], json!(["listen"]));
    assert_eq!(state["resolved"], true);
    assert_eq!(state["run_live"], false);
    assert_eq!(state["session_id"], f.sid.as_str());
    assert!(f.marker(&f.sid).exists());
    assert!(f.heartbeat(&f.sid).exists(), "a listening session looks alive");

    let plain = f.drain("plain");
    assert!(plain.contains("a addressed") && plain.contains("a fresh open"), "{plain}");
    assert!(!plain.contains("a old open"), "old #open backlog is held back: {plain}");
    assert!(plain.contains("1 older message held back"), "{plain}");
}

#[test]
fn listen_off_stops_delivery_and_listen_on_applies_the_cold_start_again() {
    let f = Fixture::new("listen-off");
    f.ok(&["listen", "on"]);
    f.drain("plain");
    assert_eq!(f.ok(&["listen", "off"]), "not listening\n");
    assert_eq!(doc(&f, &["listen", "--json"])["via"], json!([]));

    backlog(&f, "b");
    let seen_before = std::fs::read_to_string(f.state_file(&f.sid)).unwrap();
    assert_eq!(f.drain("plain"), "", "not listening: nothing delivered");
    assert_eq!(std::fs::read_to_string(f.state_file(&f.sid)).unwrap(), seen_before, "and nothing marked");

    f.ok(&["listen", "on"]);
    let plain = f.drain("plain");
    assert!(plain.contains("b addressed") && plain.contains("b fresh open"), "{plain}");
    assert!(!plain.contains("b old open"), "{plain}");
    assert!(plain.contains("1 older message held back"), "{plain}");
    assert!(f.drain("plain").contains("no pending messages"));
}

#[test]
fn a_joined_session_listens_and_listen_off_keeps_its_channels() {
    let f = Fixture::new("listen-join");
    f.ok(&["channel", "join", "room"]);
    let state = doc(&f, &["listen", "--json"]);
    assert_eq!(state["listening"], true);
    assert_eq!(state["via"], json!(["join"]));

    assert_eq!(f.ok(&["listen", "off"]), "not listening\n");
    assert!(f.ok(&["channel", "--joined"]).contains("room"), "membership kept");
    f.put("@room", "other-1-room", "in the room", Duration::ZERO);
    assert_eq!(f.drain("plain"), "");

    // A later join listens again.
    f.ok(&["channel", "join", "other"]);
    assert_eq!(f.ok(&["listen"]), "listening\n");
}

#[test]
fn listen_needs_a_session() {
    let f = Fixture::new("listen-unresolved");
    let out = f.without_session(|| f.attend(&["listen", "on", "--json"]));
    assert_eq!(out.status.code(), Some(1));
    let d: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(d["error"]["kind"], "unresolved");
    let out = f.without_session(|| f.attend(&["listen", "status"]));
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "not listening\n");
}

#[test]
fn unread_counts_by_room_and_marks_nothing() {
    let f = Fixture::new("unread");
    f.ok(&["channel", "join", "room"]);
    f.drain("plain");
    f.put("_broadcast", "other-1-u1", "one", Duration::ZERO);
    f.put("_broadcast", "other-1-u2", "two", Duration::ZERO);
    f.put(&f.project_tray(), "other-1-u3", "three", Duration::ZERO);
    f.put("@room", "other-1-u4", "four", Duration::ZERO);

    let counts = doc(&f, &["message", "unread", "--json"]);
    assert_eq!(counts["unread"], 4);
    assert_eq!(counts["held_back"], 0);
    assert_eq!(counts["listening"], true);
    assert_eq!(
        counts["rooms"],
        json!([
            {"label": "project", "kind": "project", "count": 1},
            {"label": "#open", "kind": "open", "count": 2},
            {"label": "@room", "kind": "channel", "name": "room", "count": 1},
        ])
    );
    let text = f.ok(&["message", "unread"]);
    assert!(text.starts_with("4 unread\n") && text.contains("  #open 2\n") && text.contains("  @room 1\n"), "{text}");

    assert_eq!(doc(&f, &["message", "unread", "--json"])["unread"], 4, "counting marked nothing");
    assert!(f.drain("plain").contains("4 message(s) drained"));
    assert_eq!(doc(&f, &["message", "unread", "--json"])["unread"], 0);
}

/// A session no conduit has baselined is counted as its cold start would
/// deliver, with what it would hold back apart, and nothing is written.
#[test]
fn unread_on_a_cold_session_counts_what_the_cold_start_delivers() {
    let f = Fixture::new("unread-cold");
    backlog(&f, "c");
    let counts = doc(&f, &["message", "unread", "--json"]);
    assert_eq!(counts["unread"], 2);
    assert_eq!(counts["held_back"], 1);
    assert_eq!(counts["listening"], false);
    assert!(!f.state_file(&f.sid).exists());
}

/// `/clear` gives the session a new id. `unread` adopts what this Claude
/// Code process enrolled under the old id, as `listen` and the drain do, so
/// it still reports the session listening, counts against the old
/// seen-set, and reads the old id's channels.
#[test]
fn unread_after_clear_counts_as_the_session_did_before() {
    let f = Fixture::new("unread-clear");
    f.ok(&["listen", "on"]);
    f.ok(&["channel", "join", "room"]);
    f.put("_broadcast", "other-1-c1", "delivered open", Duration::ZERO);
    f.put(&f.project_tray(), "other-1-c2", "delivered addressed", Duration::ZERO);
    assert!(f.drain("plain").contains("2 message(s) drained"));
    f.put("@room", "other-1-c3", "unread in room", Duration::ZERO);
    let before = doc(&f, &["message", "unread", "--json"]);
    assert_eq!(before["unread"], 1);

    f.set_session_id("sess-unread-clear-after");
    let after = doc(&f, &["message", "unread", "--json"]);
    assert_eq!(after["listening"], true, "{after}");
    assert_eq!(after["unread"], 1, "{after}");
    assert_eq!(after["rooms"], before["rooms"], "{after}");
    assert_eq!(f.ok(&["listen"]), "listening\n");
}

/// `listen off`, then a join: listening again by any way reopens the cold
/// start, as `listen on` does, so the `#open` backlog built up while not
/// listening is held back with its note.
#[test]
fn a_join_after_listen_off_holds_back_the_backlog() {
    let f = Fixture::new("join-after-off");
    f.ok(&["listen", "on"]);
    f.drain("plain");
    f.ok(&["listen", "off"]);
    f.put("_broadcast", "other-1-j1", "old open while off", 30 * MINUTE);
    f.put(&f.project_tray(), "other-1-j2", "addressed while off", 30 * MINUTE);
    f.ok(&["channel", "join", "newroom"]);
    let plain = f.drain("plain");
    assert!(plain.contains("addressed while off"), "{plain}");
    assert!(!plain.contains("old open while off"), "{plain}");
    assert!(plain.contains("1 older message held back"), "{plain}");
}

/// After `listen off`, `unread` counts what the next `listen on` and drain
/// would deliver, with the older backlog counted as held back.
#[test]
fn unread_after_listen_off_counts_what_listen_on_delivers() {
    let f = Fixture::new("unread-off");
    f.ok(&["listen", "on"]);
    f.drain("plain");
    f.ok(&["listen", "off"]);
    backlog(&f, "d");
    let counts = doc(&f, &["message", "unread", "--json"]);
    assert_eq!(counts["listening"], false);
    assert_eq!(counts["unread"], 2, "{counts}");
    assert_eq!(counts["held_back"], 1, "{counts}");
    f.ok(&["listen", "on"]);
    assert!(f.drain("plain").contains("2 message(s) drained"));
}

/// `listen off` while an `attend run` holds the session: it keeps listening
/// until the run exits, and says so on stderr.
#[test]
fn listen_off_under_a_live_run_keeps_listening() {
    let f = Fixture::new("listen-off-run");
    let run = f.run();
    run.wait_for("the run's lock", Duration::from_secs(20), |_| {
        doc(&f, &["listen", "--json"])["run_live"] == true
    });
    let out = f.attend(&["listen", "off", "--json"]);
    assert!(out.status.success());
    let state: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(state["listening"], true);
    assert_eq!(state["run_live"], true);
    assert!(String::from_utf8_lossy(&out.stderr).contains("listens until that run exits"));
    drop(run);
    assert_eq!(f.ok(&["listen"]), "not listening\n", "the opt-out applies once the run is gone");
}

/// `listen beat`: the attend mod's timer keeps a listening session's
/// heartbeat inside the grace through a turn longer than it, which no drain
/// touches (the mod-session liveness defect of 2026-10-13). A session that
/// does not listen is written nothing.
#[test]
fn listen_beat_touches_a_listening_session_only() {
    let f = Fixture::new("listen-beat");
    assert_eq!(f.ok(&["listen", "beat"]), "not listening\n");
    assert!(!f.heartbeat(&f.sid).exists(), "a session that does not listen looks no more alive");
    f.ok(&["listen", "on", "--host", "mod"]);
    common::age_file(&f.heartbeat(&f.sid), Duration::from_secs(120));
    let stale = std::fs::metadata(f.heartbeat(&f.sid)).unwrap().modified().unwrap();
    assert_eq!(f.ok(&["listen", "beat"]), "listening\n");
    let beat = std::fs::metadata(f.heartbeat(&f.sid)).unwrap().modified().unwrap();
    assert!(beat.duration_since(stale).unwrap() >= Duration::from_secs(110), "the beat freshened the heartbeat");
    assert!(!f.state_file(&f.sid).exists(), "and delivered nothing: no drain ran");
    f.ok(&["listen", "off"]);
    common::age_file(&f.heartbeat(&f.sid), Duration::from_secs(120));
    f.ok(&["listen", "beat"]);
    let after = std::fs::metadata(f.heartbeat(&f.sid)).unwrap().modified().unwrap();
    assert!(std::time::SystemTime::now().duration_since(after).unwrap() >= Duration::from_secs(110), "after listen off it touches nothing");
    let out = f.without_session(|| f.attend(&["listen", "beat"]));
    assert_eq!(out.status.code(), Some(1), "no session, no beat");
}

/// `listen beat --mod <version>` records the version the session's mod runs,
/// for the drain's stale-mod check; a beat without the flag, as an older mod
/// sends it, works as before and records nothing.
#[test]
fn listen_beat_records_the_mod_version_when_given() {
    let f = Fixture::new("listen-beat-mod");
    assert_eq!(f.ok(&["listen", "beat"]), "not listening\n");
    assert_eq!(f.ok(&["listen", "beat", "--mod", "0.27.1"]), "not listening\n");
    assert!(!f.state_file(&f.sid).exists(), "a session that does not listen is written nothing");
    f.ok(&["listen", "on", "--host", "mod"]);
    assert_eq!(f.ok(&["listen", "beat"]), "listening\n", "an older mod sends no version");
    assert!(!f.state_file(&f.sid).exists(), "and records none");
    assert_eq!(f.ok(&["listen", "beat", "--mod", "0.27.1"]), "listening\n");
    let state = std::fs::read_to_string(f.state_file(&f.sid)).unwrap();
    assert!(state.contains("mod_version: 0.27.1"), "{state}");
    f.ok(&["listen", "beat", "--mod", "0.28.0"]);
    let state = std::fs::read_to_string(f.state_file(&f.sid)).unwrap();
    assert!(state.contains("mod_version: 0.28.0") && !state.contains("0.27.1"), "{state}");
}
