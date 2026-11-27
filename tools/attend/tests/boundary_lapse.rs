// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! A session boundary lapses the session's pending event-lane notices.
//!
//! A compaction or a `/clear` changes what the session's context holds, so
//! a notice queued before it describes a session that is gone: a context
//! fill that no longer holds, a branch the model has forgotten it was told
//! about. The attend mod runs `attend message lapse --events` at the
//! boundary. The notices are marked read and never delivered. A peer's
//! message stays queued, and a notice queued after the boundary drains as
//! usual (#1368).
//!
//! References:
//! - ADR-401, The system sender: an event-lane signal lapses unread.
//! - ADR-416 §4: the event lane's delivery at a turn's end.

mod common;
use agent_identity::signal::{format_system, Lane, SystemSignal};
use common::Fixture;
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

fn mod_session(tag: &str) -> Fixture {
    let f = Fixture::new(tag);
    f.ok(&["listen", "on", "--host", "mod"]);
    f.ok(&["message", "inbox", "--drain", "--json"]);
    f
}

fn put_notice(f: &Fixture, name: &str, sensor: &str, text: &str) {
    let sys = SystemSignal { sensor, to: &f.sid, lane: Lane::Event, expires: Some(now() + 3600) };
    let line = format_system("proj", &f.origin, &sys, text).unwrap();
    let dir = f.signals().join(f.project_tray());
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.signal")), line).unwrap();
}

fn put_peer(f: &Fixture, name: &str, text: &str) {
    let dir = f.signals().join(f.project_tray());
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.signal")), format!("claude:peer|p|/tmp/p|to:{}|{text}\n", f.sid)).unwrap();
}

fn drained(f: &Fixture) -> Vec<String> {
    let d: Value = serde_json::from_str(&f.ok(&["message", "inbox", "--drain", "--json"])).unwrap();
    let mut b: Vec<String> = d["messages"].as_array().unwrap().iter().map(|m| m["message"].as_str().unwrap().to_string()).collect();
    b.sort();
    b
}

#[test]
fn a_boundary_lapses_every_pending_notice_and_keeps_the_peer_message() {
    let f = mod_session("boundary-lapse");
    for (i, sensor) in ["context", "git", "processes", "peers", "ways-update"].iter().enumerate() {
        put_notice(&f, &format!("{sensor}-1-{i}"), sensor, &format!("{sensor} notice"));
    }
    put_peer(&f, "peer-1-9", "are you there");
    f.ok(&["message", "lapse", "--events"]);
    assert_eq!(drained(&f), ["are you there"], "only the peer's message survives the boundary");
}

#[test]
fn a_notice_queued_after_the_boundary_still_drains() {
    let f = mod_session("boundary-after");
    put_notice(&f, "context-1-1", "context", "context at 77%");
    f.ok(&["message", "lapse", "--events"]);
    put_notice(&f, "context-2-2", "context", "context at 79%");
    assert_eq!(drained(&f), ["context at 79%"]);
}

fn put_raw(f: &Fixture, room: &str, name: &str, line: &str) {
    let dir = f.signals().join(room);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.signal")), format!("{line}\n")).unwrap();
}

#[test]
fn every_kind_of_peer_message_survives_the_lapse() {
    let f = mod_session("boundary-peers");
    f.ok(&["channel", "join", "room"]);
    put_notice(&f, "git-1-1", "git", "git notice");
    put_peer(&f, "peer-1-2", "a direct message");
    put_peer(&f, "peer-1-3", "handoff: take this over");
    put_raw(&f, "@room", "peer-1-4", "claude:peer|p|/tmp/p|a channel post");
    put_raw(&f, "_broadcast", "peer-1-5", &format!("claude:peer|p|/tmp/p|to:{}|an open post naming the session", f.sid));
    put_raw(&f, &f.project_tray(), "peer-1-6", &format!("claude:peer|p|/tmp/p|re:{}-1-0|a reply", f.sid));
    f.ok(&["message", "lapse", "--events"]);
    assert_eq!(
        drained(&f),
        ["a channel post", "a direct message", "a reply", "an open post naming the session", "handoff: take this over"],
    );
}

#[test]
fn a_project_notice_lapsed_by_one_session_still_drains_for_its_sibling() {
    let f = mod_session("boundary-sibling");
    let mut b = common::Claude::start(&f, "sibling-b");
    b.attend(&["listen", "on", "--host", "mod"]);
    b.attend(&["message", "inbox", "--drain", "--json"]);
    let sys = SystemSignal { sensor: "git", to: &f.sid, lane: Lane::Event, expires: Some(now() + 3600) };
    let line = format_system("proj", &f.origin, &sys, "branch switched").unwrap().replacen(&format!("|to:{}", f.sid), "", 1);
    assert!(!line.contains("to:"), "{line}");
    put_raw(&f, &f.project_tray(), "git-1-1", line.trim_end());
    f.ok(&["message", "lapse", "--events"]);
    assert_eq!(drained(&f), Vec::<String>::new(), "lapsed for the session that crossed the boundary");

    let out = b.attend(&["message", "inbox", "--drain", "--json"]);
    assert!(out.contains("branch switched"), "the sibling's own seen-set never recorded it: {out}");
}
