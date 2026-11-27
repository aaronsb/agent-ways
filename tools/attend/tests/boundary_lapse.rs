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
