// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! How attend's own signals reach a session, through the CLI as the attend
//! mod and the Stop hook run it.
//!
//! A system signal on the event lane never starts a turn: the wake drain
//! the mod polls with passes it over, and it reaches the session at the end
//! of the turn in progress through the Stop-hook drain, or with the next
//! delivery that wakes the session. Past its `expires` it is marked read
//! and never delivered. A system signal on the message lane wakes the
//! session as an addressed message does. The Stop-hook drain delivers only
//! to a session the attend mod enrolled (`attend listen on --host mod`).
//!
//! References:
//! - ADR-416 §4 and its turn-end settlement: the lane rule, the expiry and
//!   the Stop-hook drain kept as the mod's turn-end conduit.
//! - ADR-401, The system sender: `expires` and its absence.

mod common;
use agent_identity::signal::{format_system, Lane, SystemSignal};
use common::Fixture;
use serde_json::Value;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MIN: Duration = Duration::from_secs(60);

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

/// A fixture the attend mod enrolled, past its cold start, its cache warm.
fn mod_session(tag: &str) -> Fixture {
    let f = Fixture::new(tag);
    f.ok(&["listen", "on", "--host", "mod"]);
    f.ok(&["message", "inbox", "--drain", "--json"]);
    f
}

/// Write a system signal from `sensor` to the fixture's session, in its
/// project tray, as the sensor host writes one. Returns its file name.
fn put_system(f: &Fixture, name: &str, sensor: &str, lane: Lane, expires: Option<u64>, text: &str) -> String {
    let sys = SystemSignal { sensor, to: &f.sid, lane, expires };
    let line = format_system("proj", &f.origin, &sys, text).unwrap();
    let dir = f.signals().join(f.project_tray());
    std::fs::create_dir_all(&dir).unwrap();
    let file = format!("{name}.signal");
    std::fs::write(dir.join(&file), line).unwrap();
    file
}

/// Write a direct message from a peer to the fixture's session.
fn put_addressed(f: &Fixture, name: &str, text: &str) {
    let dir = f.signals().join(f.project_tray());
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.signal")), format!("claude:peer|p|/tmp/p|to:{}|{text}\n", f.sid)).unwrap();
}

fn drain_json(f: &Fixture, wake: bool) -> Value {
    let mut args = vec!["message", "inbox", "--drain", "--json"];
    if wake {
        args.push("--wake");
    }
    serde_json::from_str(&f.ok(&args)).unwrap()
}

fn bodies(d: &Value) -> Vec<String> {
    let mut b: Vec<String> = d["messages"].as_array().unwrap().iter().map(|m| m["message"].as_str().unwrap().to_string()).collect();
    b.sort();
    b
}

fn unread(f: &Fixture) -> Value {
    serde_json::from_str(&f.ok(&["message", "unread", "--json"])).unwrap()
}

/// What the Stop-hook drain hands the model, or "" when it prints nothing.
fn stop_hook(f: &Fixture) -> String {
    let out = f.drain_hook_with("{}");
    if out.trim().is_empty() {
        return String::new();
    }
    let doc: Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out}"));
    doc["hookSpecificOutput"]["additionalContext"].as_str().unwrap_or_default().to_string()
}

/// Whether the session's seen-set records signal file `file`.
fn seen(f: &Fixture, file: &str) -> bool {
    std::fs::read_to_string(f.state_file(&f.sid)).is_ok_and(|s| s.contains(&attend_state::seen_key(file)))
}

/// Done-when 1: an event-lane signal starts no turn. The wake drain passes
/// it over, warm cache and all, and leaves it unread; the Stop-hook drain at
/// the end of the turn delivers it; once drained, nothing wakes for it.
#[test]
fn an_event_signal_waits_for_the_turn_end_and_never_wakes() {
    let f = mod_session("sys-event");
    put_system(&f, "git-1-1", "git", Lane::Event, Some(now() + 3600), "branch switched to main");
    for _ in 0..2 {
        let d = drain_json(&f, true);
        assert_eq!(bodies(&d), Vec::<String>::new(), "an event never wakes: {d}");
        assert_eq!(d["cache_lapsed"], false, "and the cache is warm: {d}");
    }
    assert_eq!(unread(&f)["unread"], 1, "it waits unread");

    let text = stop_hook(&f);
    assert!(text.contains("branch switched to main"), "the turn's end delivers it: {text:?}");
    assert!(text.contains("attend (git)"), "named as attend's: {text}");

    assert_eq!(bodies(&drain_json(&f, true)), Vec::<String>::new(), "drained, it does not wake later");
    assert_eq!(stop_hook(&f), "", "nor come again at the next turn's end");
    assert_eq!(unread(&f)["unread"], 0);
}

/// Done-when 1 and the mixed batch: an event-lane signal rides the next
/// delivery that wakes the session. One wake drain returns both, the
/// message and the event, and the next finds nothing.
#[test]
fn an_event_rides_the_next_waking_delivery() {
    let f = mod_session("sys-mixed");
    put_system(&f, "git-1-1", "git", Lane::Event, Some(now() + 3600), "branch switched to main");
    put_addressed(&f, "peer-1-2", "are you there");
    let d = drain_json(&f, true);
    assert_eq!(bodies(&d), ["are you there", "branch switched to main"], "{d}");
    let lanes: Vec<&str> = d["messages"].as_array().unwrap().iter().map(|m| m["lane"].as_str().unwrap()).collect();
    assert!(lanes.contains(&"event") && lanes.contains(&"message"), "{d}");
    assert_eq!(bodies(&drain_json(&f, true)), Vec::<String>::new(), "one wake for the batch");
    assert_eq!(unread(&f)["unread"], 0);
}

/// Done-when 3: a message-lane system signal wakes the session as an
/// addressed message does.
#[test]
fn a_message_lane_system_signal_wakes() {
    let f = mod_session("sys-message");
    put_system(&f, "chat-1-1", "chat", Lane::Message, None, "ann asked about the deploy");
    let d = drain_json(&f, true);
    assert_eq!(bodies(&d), ["ann asked about the deploy"], "{d}");
    assert_eq!(d["messages"][0]["from_kind"], "system", "{d}");
}

/// Done-when 2: past its `expires`, an event is never delivered, by the
/// Stop hook, a waking delivery or a plain drain, and is not counted
/// unread. The drain records it read, so it never comes back. A notice
/// with no `expires` (`expires: session`) is kept however old it is.
#[test]
fn an_expired_signal_is_never_delivered() {
    let f = mod_session("sys-expired");
    let lapsed = put_system(&f, "git-1-1", "git", Lane::Event, Some(now() - 10), "branch switched an hour ago");
    let kept = put_system(&f, "ways-update-1-2", "ways-update", Lane::Event, None, "agent-ways has a newer release");
    let path = f.signals().join(f.project_tray()).join(&kept);
    std::fs::File::options().write(true).open(&path).unwrap().set_modified(SystemTime::now() - 120 * MIN).unwrap();

    assert_eq!(unread(&f)["unread"], 1, "the lapsed one is not unread");
    put_addressed(&f, "peer-1-3", "wake up");
    let d = drain_json(&f, true);
    assert_eq!(bodies(&d), ["agent-ways has a newer release", "wake up"], "a waking delivery leaves it out: {d}");
    assert!(seen(&f, &lapsed), "and records it read");

    let g = mod_session("sys-expired-hook");
    let lapsed = put_system(&g, "git-1-1", "git", Lane::Event, Some(now() - 10), "branch switched an hour ago");
    assert_eq!(stop_hook(&g), "", "the turn's end delivers nothing");
    assert!(seen(&g, &lapsed), "and records it read");
    assert_eq!(bodies(&drain_json(&g, false)), Vec::<String>::new(), "it never resurfaces");
    assert_eq!(unread(&g)["unread"], 0);
}

/// Done-when 4: the Stop-hook drain delivers nothing to a session the mod
/// did not enroll, a shell's `attend listen on` included, and marks
/// nothing, so what waits is still there once the mod enrolls it.
#[test]
fn a_session_the_mod_did_not_enroll_gets_nothing_from_the_stop_hook() {
    let f = Fixture::new("sys-unenrolled");
    f.ok(&["listen", "on"]);
    f.ok(&["message", "inbox", "--drain", "--json"]);
    put_system(&f, "git-1-1", "git", Lane::Event, Some(now() + 3600), "branch switched to main");
    put_addressed(&f, "peer-1-2", "for you");
    assert_eq!(f.drain_hook_with("{}"), "", "the hook prints nothing");
    assert_eq!(unread(&f)["unread"], 2, "and marks nothing");

    f.ok(&["listen", "on", "--host", "mod"]);
    let text = stop_hook(&f);
    assert!(text.contains("branch switched to main") && text.contains("for you"), "{text}");
}

/// `attend message unread` counts an unread event, which waits for the
/// next turn, but never calls the cache cold for it: only a message that
/// would wake the session does.
#[test]
fn unread_counts_an_event_without_calling_it_cold() {
    let f = mod_session("sys-unread");
    f.last_turn(Some(61 * MIN));
    put_system(&f, "git-1-1", "git", Lane::Event, Some(now() + 3600), "branch switched to main");
    let u = unread(&f);
    assert_eq!((u["unread"].as_u64(), u["cache_lapsed"].as_bool()), (Some(1), Some(false)), "{u}");
    assert_eq!(f.ok(&["message", "unread"]).lines().next(), Some("1 unread"));
    let d = drain_json(&f, true);
    assert_eq!(d["cache_lapsed"], false, "the wake drain holds nothing for the cache: {d}");

    put_addressed(&f, "peer-1-2", "for you");
    assert_eq!(unread(&f)["cache_lapsed"], true);
    assert_eq!(f.ok(&["message", "unread"]).lines().next(), Some("2 waiting · cold"));
}
