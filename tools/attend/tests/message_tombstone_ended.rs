// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! `attend message tombstone --ended` tombstones, as one batch, every agent
//! with a message still shown on the bus that the single tombstone's rule
//! allows, and `--undo` takes the batch back (ADR-404 D3e, #1354).
//!
//! Sessions are written into the fixture's cache as attend writes them, as
//! `message_delete.rs` writes them: a heartbeat, an enrollment record, a
//! Claude Code session record. Their ids are UUIDs, the shape of a Claude
//! Code session id.

mod common;
use common::{age_file, Fixture, MINUTE};
use serde_json::Value;
use std::time::Duration;

/// Live, listening, and once known as [`R4`] before a `/clear`.
const R1: &str = "11111111-1111-4111-8111-111111111111";
const R2: &str = "22222222-2222-4222-8222-222222222222";
const R3: &str = "33333333-3333-4333-8333-333333333333";
/// R1's id before its `/clear`: no record of its own, so nothing but R1
/// reading as it keeps it from the batch.
const R4: &str = "44444444-4444-4444-8444-444444444444";
const R5: &str = "55555555-5555-4555-8555-555555555555";
const R6: &str = "66666666-6666-4666-8666-666666666666";
/// The caller's own session id.
const ME: &str = "99999999-9999-4999-8999-999999999999";

/// Write `line` as signal `id` into `room`, `age` old.
fn signal(f: &Fixture, room: &str, id: &str, line: &str, age: Duration) {
    let dir = f.signals().join(room);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{id}.signal"));
    std::fs::write(&path, format!("{line}\n")).unwrap();
    age_file(&path, age);
}

/// A message agent `sid` sent into `room`, ten minutes ago.
fn from(f: &Fixture, room: &str, id: &str, sid: &str) {
    signal(f, room, id, &format!("claude:{sid}|p|/srv/p|{id} from {sid}"), 10 * MINUTE);
}

/// Session `sid` listening, alive, working at `project`, and known as
/// `earlier` before a `/clear`.
fn listener(f: &Fixture, sid: &str, project: &str, earlier: &str) {
    let beat = f.heartbeat(sid);
    std::fs::create_dir_all(beat.parent().unwrap()).unwrap();
    std::fs::write(&beat, "").unwrap();
    let marker = f.marker(sid);
    std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
    std::fs::write(&marker, format!("listen\nprevious {earlier}\n")).unwrap();
    std::fs::write(
        f.home.join(".claude").join("sessions").join(format!("rec-{sid}.json")),
        format!(r#"{{"pid":999999,"sessionId":"{sid}","cwd":"{project}"}}"#),
    )
    .unwrap();
}

/// Session `sid` ran at `/srv/gone` and its process has exited.
fn exited(f: &Fixture, sid: &str) {
    std::fs::write(
        f.home.join(".claude").join("sessions").join(format!("gone-{sid}.json")),
        format!(r#"{{"pid":999999,"sessionId":"{sid}","cwd":"/srv/gone"}}"#),
    )
    .unwrap();
}

fn json(f: &Fixture, args: &[&str]) -> Value {
    let mut argv = vec!["message", "tombstone", "--json"];
    argv.extend_from_slice(args);
    serde_json::from_str(&f.ok(&argv)).unwrap()
}

fn ids(v: &Value, field: &str) -> Vec<String> {
    v[field].as_array().unwrap_or_else(|| panic!("no {field} in {v}")).iter().map(|i| i.as_str().unwrap().to_string()).collect()
}

fn tombstoned(f: &Fixture) -> Vec<String> {
    ids(&json(f, &["--list"]), "sessions")
}

/// The bus holds two ended agents, a live one, an id the live one had
/// before its `/clear`, a person, the caller's own session, and an ended
/// agent whose only message its tombstone already hides, each in a room of
/// its own kind. `--ended` takes only the two ended agents with a message
/// still shown.
fn crowded_bus(tag: &str) -> Fixture {
    let f = Fixture::new(tag);
    f.set_session_id(ME);
    let tray = f.project_tray();
    listener(&f, R1, "/srv/r1", R4);
    exited(&f, R2);
    exited(&f, R3);
    exited(&f, R5);
    from(&f, "@deploy", "live-1", R1);
    from(&f, "_broadcast", "earlier-1", R4);
    from(&f, "_broadcast", "ended-2", R2);
    from(&f, &tray, "ended-3", R3);
    from(&f, "_broadcast", "mine-1", ME);
    from(&f, "_broadcast", "hidden-5", R5);
    signal(&f, "_people/fixture-human", "person-1", "external:fixture-human@host|p|/srv/p|from a person", 10 * MINUTE);
    f.ok(&["message", "tombstone", R5]);
    f
}

#[test]
fn ended_tombstones_only_the_ended_agents() {
    let f = crowded_bus("ended-select");
    let v = json(&f, &["--ended"]);
    assert_eq!(ids(&v, "sessions"), [R2, R3], "{v}");
    assert_eq!((v["count"].as_u64(), v["dry_run"].as_bool()), (Some(2), Some(false)));
    assert!(v["batch"].is_string(), "{v}");
    assert_eq!(tombstoned(&f), [R2, R3, R5]);
    let listing = f.ok(&["message", "inbox"]);
    assert!(!listing.contains("ended-2") && !listing.contains("ended-3"), "{listing}");
    assert!(listing.contains("earlier-1"), "{listing}");
    let again = json(&f, &["--ended"]);
    assert_eq!((again["count"].as_u64(), again["batch"].is_null()), (Some(0), true), "nothing left shown: {again}");
}

#[test]
fn a_dry_run_says_what_it_would_tombstone_and_changes_nothing() {
    let f = crowded_bus("ended-dry");
    let store = f.signals().join("_tombstones");
    let before: Vec<_> = std::fs::read_dir(&store).unwrap().flatten().map(|e| e.file_name()).collect();
    let v = json(&f, &["--ended", "--dry-run"]);
    assert_eq!(ids(&v, "sessions"), [R2, R3], "{v}");
    assert_eq!((v["dry_run"].as_bool(), v["batch"].is_null()), (Some(true), true));
    let said = f.ok(&["message", "tombstone", "--ended", "-n"]);
    assert_eq!(said, format!("would tombstone 2 ended session(s):\n  {R2}\n  {R3}\n"));
    let after: Vec<_> = std::fs::read_dir(&store).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(before, after, "no tombstone and no batch record written");
    assert_eq!(tombstoned(&f), [R5]);
}

/// The batch's undo removes what the batch wrote and moves back what it
/// moved; a tombstone written after the batch stays, whether of a session
/// in the batch or of another.
#[test]
fn undo_takes_back_exactly_the_batch() {
    let f = Fixture::new("ended-undo");
    for (n, sid) in [R2, R3, R5].into_iter().enumerate() {
        exited(&f, sid);
        from(&f, "_broadcast", &format!("m-{n}"), sid);
    }
    exited(&f, R6);
    let first = json(&f, &["--ended"]);
    assert_eq!(ids(&first, "sessions"), [R2, R3, R5]);
    // R3 tombstoned again on its own, and R6, which sent nothing, after it.
    f.ok(&["message", "tombstone", R3]);
    f.ok(&["message", "tombstone", R6]);

    let undone = json(&f, &["--undo"]);
    assert_eq!(undone["batch"], first["batch"]);
    assert_eq!((ids(&undone, "sessions"), ids(&undone, "kept")), (vec![R2.to_string(), R5.to_string()], vec![R3.to_string()]), "{undone}");
    assert_eq!(tombstoned(&f), [R3, R6], "the later tombstones stand");
    let listing = f.ok(&["message", "inbox"]);
    assert!(listing.contains("m-0") && listing.contains("m-2") && !listing.contains("m-1"), "{listing}");

    let out = f.attend(&["message", "tombstone", "--undo"]);
    assert_eq!(out.status.code(), Some(1), "one undo per batch");
    assert!(String::from_utf8_lossy(&out.stderr).contains("no tombstone batch to undo"));

    // A batch named, beside a newer one.
    let named = json(&f, &["--ended"]);
    let batch = named["batch"].as_str().unwrap().to_string();
    // R6 back under --resume: what it sends now is past its tombstone.
    std::thread::sleep(Duration::from_millis(20));
    signal(&f, "_broadcast", "m-6", &format!("claude:{R6}|p|/srv/p|m-6 from {R6}"), Duration::ZERO);
    std::thread::sleep(Duration::from_millis(20));
    let newer = json(&f, &["--ended"]);
    assert_eq!(ids(&newer, "sessions"), [R6], "its tombstone moves: {newer}");
    let said = f.ok(&["message", "tombstone", "--undo", &batch]);
    assert!(said.starts_with(&format!("removed the tombstones of 2 session(s) from batch {batch}")), "{said}");
    assert_eq!(tombstoned(&f), [R3, R6], "the newer batch's R6 stays");
    assert!(!f.ok(&["message", "inbox"]).contains("m-6"));
    assert_eq!(ids(&json(&f, &["--undo"]), "sessions"), [R6]);
    assert_eq!(tombstoned(&f), [R3, R6], "R6's own tombstone is back at its cut");
    assert!(f.ok(&["message", "inbox"]).contains("m-6"), "and what R6 sent since shows");

    // A single undo works as it did, the id on either side of --undo.
    f.ok(&["message", "tombstone", R3, "--undo"]);
    f.ok(&["message", "tombstone", "--undo", R6]);
    assert!(tombstoned(&f).is_empty());
}

#[test]
fn ended_refuses_a_session_id_and_dry_run_needs_ended() {
    let f = Fixture::new("ended-usage");
    exited(&f, R2);
    for args in [vec!["--ended", R2], vec!["--dry-run", R2], vec![R2, "--undo", "000000000000001-1"]] {
        let mut argv = vec!["message", "tombstone"];
        argv.extend(&args);
        let out = f.attend(&argv);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }
    assert!(tombstoned(&f).is_empty());
}
