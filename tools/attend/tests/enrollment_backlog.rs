//! Messages that arrived before a session enrolled (#725 review, finding 2).
//!
//! One cold-start rule for both conduits: a message addressed to the
//! project (`--to`) since the session began is delivered on enrollment
//! whatever its age, while older addressed mail and an old `#open` message
//! are baselined and announced, never consumed silently.
//! Each enrollment path is covered: `attend channel join` (the drain runs first) and
//! `/attend` (the peers sensor under `attend run` runs first).

mod common;
use common::{Fixture, MINUTE};
#[cfg(feature = "sensor-peers")]
use std::time::Duration;

const ADDRESSED: &str = "addressed ten minutes ago";
const OLD_OPEN: &str = "open ten minutes ago";
const NOTE: &str = "1 older message held back; reading it is optional: attend message inbox";

fn with_backlog(tag: &str) -> Fixture {
    let f = Fixture::new(tag);
    f.put(&f.project_tray(), "other-1-addressed", ADDRESSED, 10 * MINUTE);
    f.put("_broadcast", "other-1-open", OLD_OPEN, 10 * MINUTE);
    f
}

#[test]
fn join_then_drain_delivers_addressed_and_announces_the_rest() {
    let f = with_backlog("backlog-join");
    f.ok(&["channel", "join", "side"]);

    let first = f.drain("plain");
    assert!(first.contains(ADDRESSED), "addressed mail must reach the session: {first}");
    assert!(!first.contains(OLD_OPEN), "old #open backlog is not shown: {first}");
    assert!(first.contains(NOTE), "the baseline must be announced: {first}");

    let hook_again = f.drain("hook");
    assert_eq!(hook_again, "", "nothing is delivered twice");
    // The backlog is still there to read on demand.
    assert!(f.ok(&["message", "inbox"]).contains(OLD_OPEN));
}

/// A cold start with only backlog to report delivers nothing, so it must
/// not block the stop: the note goes as a non-blocking systemMessage. That
/// reaches the operator only, so the name the model needs comes from the
/// join's own output, and the systemMessage carries none.
#[test]
fn a_note_alone_does_not_block_the_stop() {
    let f = Fixture::new("backlog-note-only");
    f.put("_broadcast", "other-1-open", OLD_OPEN, 10 * MINUTE);
    let joined = f.ok(&["channel", "join", "side"]);
    assert!(joined.lines().any(|l| l.starts_with("you are ")), "the join names the session: {joined}");

    let hook = f.drain_hook_with("{}");
    assert!(hook.starts_with("{\"systemMessage\": "), "non-blocking: {hook}");
    assert!(!hook.contains("\"decision\""), "must not block: {hook}");
    assert!(hook.contains(NOTE), "the note: {hook}");
    assert!(!hook.contains("you are"), "the operator's UI gets no agent-directed name: {hook}");
    assert_eq!(f.drain_hook_with("{}"), "", "the note goes out once");
}

/// What the model receives on enrollment by join: the join's Bash output
/// names the session, and the first drain that delivers names it again in
/// the delivered text, the only part of the hook output the model reads.
#[test]
fn join_and_first_delivery_name_the_session() {
    let f = with_backlog("backlog-named");
    let joined = f.ok(&["channel", "join", "side"]);
    let name = joined.lines().find_map(|l| l.strip_prefix("you are ")).unwrap_or_else(|| panic!("no name line: {joined}")).to_string();
    assert!(!name.is_empty());

    let hook = f.drain_hook_with("{}");
    assert!(hook.starts_with("{\"hookSpecificOutput\": "), "addressed mail continues the turn: {hook}");
    assert!(hook.contains(&format!("(you are {name})")), "the delivery names the session: {hook}");
}

fn drain_doc(out: &str) -> serde_json::Value {
    serde_json::from_str(out).unwrap_or_else(|e| panic!("{e}: {out}"))
}

fn bodies(d: &serde_json::Value) -> Vec<String> {
    d["messages"].as_array().unwrap().iter().map(|m| m["message"].as_str().unwrap().to_string()).collect()
}

/// Observed on attend 0.20.0 with the attend mod: a session started fresh in
/// a project, listening from its config, was handed its predecessor's whole
/// conversation, mail addressed to the project that the predecessor had
/// already read and answered. Mail addressed to a project is addressed to
/// the sessions live there when it was sent (ADR-401), and a new agent is
/// replayed nothing from before it began (ADR-404 D3b): the new session
/// gets the held-back note, not the messages.
#[test]
fn a_new_session_is_not_replayed_its_predecessors_project_mail() {
    let f = Fixture::new("backlog-successor");
    let tray = f.project_tray();
    for i in 0..7 {
        f.put(&tray, &format!("other-1-to-{i}"), &format!("for the predecessor {i}"), 10 * MINUTE);
    }
    // Session A, live for a day, listens and reads them, as the mod does.
    f.ok(&["listen", "on"]);
    let a = drain_doc(&f.ok(&["message", "inbox", "--drain", "--json"]));
    assert_eq!(bodies(&a).len(), 7, "A was live when they were sent, so they are A's: {a}");

    // Session B: a new Claude Code process in the same project, started
    // a minute ago, turns listening on and drains, as the mod does at start.
    let b_sid = "sess-backlog-successor-b";
    f.attend_in_process(b_sid, Some(MINUTE), &["listen", "on"]);
    let b = drain_doc(&f.attend_in_process(b_sid, Some(MINUTE), &["message", "inbox", "--drain", "--json"]));
    assert_eq!(b["status"], "drained", "{b}");
    assert!(bodies(&b).is_empty(), "B is replayed none of A's mail: {b}");
    assert_eq!(b["held_back"], 7, "{b}");
    assert!(b["note"].as_str().is_some_and(|n| n.contains("7 older messages held back")), "{b}");

    // What is addressed to the project from here on is B's too.
    f.put(&tray, "other-1-to-new", "for whoever is here now", std::time::Duration::ZERO);
    let next = drain_doc(&f.attend_in_process(b_sid, Some(MINUTE), &["message", "inbox", "--drain", "--json"]));
    assert_eq!(bodies(&next), vec!["for whoever is here now".to_string()], "{next}");
}

/// Mail addressed to the project while no session listened: what was sent
/// while this session was live, though not listening, was addressed to it
/// and arrives when it listens ("delivers what was addressed to your
/// project meanwhile"); what was sent before it began was addressed to
/// whoever was live then, and is held back like any other backlog.
#[test]
fn mail_from_before_the_session_began_is_held_back_though_nobody_read_it() {
    let f = Fixture::new("backlog-nobody");
    let tray = f.project_tray();
    f.put(&tray, "other-1-before", "sent before this session began", 30 * MINUTE);
    f.put(&tray, "other-1-meanwhile", "sent while it was dormant", 5 * MINUTE);
    let sid = "sess-backlog-nobody-b";
    f.attend_in_process(sid, Some(10 * MINUTE), &["listen", "on"]);
    let d = drain_doc(&f.attend_in_process(sid, Some(10 * MINUTE), &["message", "inbox", "--drain", "--json"]));
    assert_eq!(bodies(&d), vec!["sent while it was dormant".to_string()], "{d}");
    assert_eq!(d["held_back"], 1, "{d}");
}

/// #950 review, finding 1: the attend mod holds a session's first drain
/// until the operator's first prompt, minutes after it started listening. A
/// question posted to `#open` during that wait is past the fresh window by
/// then, and must still arrive: the session was a member of `#open` from
/// the moment it listened. What `#open` carried before then stays held back.
#[test]
fn open_messages_since_listening_began_arrive_at_a_late_first_drain() {
    let f = Fixture::new("backlog-late-drain");
    f.put("_broadcast", "other-1-before", "posted before it listened", 30 * MINUTE);
    f.ok(&["listen", "on"]);
    // Ten minutes pass before the first drain: the record says it began
    // listening then, and a peer asked a question two minutes in.
    let marker = f.marker(&f.sid);
    let record = std::fs::read_to_string(&marker).unwrap();
    let back = std::time::SystemTime::now() - 10 * MINUTE;
    let since = back.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let record: String =
        record.lines().filter(|l| !l.starts_with("since ")).map(|l| format!("{l}\n")).collect::<String>() + &format!("since {since}\n");
    std::fs::write(&marker, record).unwrap();
    f.put("_broadcast", "other-1-during", "a question while it waited", 8 * MINUTE);

    let d = drain_doc(&f.ok(&["message", "inbox", "--drain", "--json"]));
    assert_eq!(bodies(&d), vec!["a question while it waited".to_string()], "{d}");
    assert_eq!(d["held_back"], 1, "{d}");
}

/// A session record with no start time (an older Claude Code) cannot say
/// whom old project mail was for, so it is held back as old `#open`
/// backlog is; a message younger than the fresh window still arrives.
#[test]
fn without_a_start_time_old_project_mail_is_held_back() {
    let f = Fixture::new("backlog-unknown-start");
    let tray = f.project_tray();
    f.put(&tray, "other-1-old", "old project mail", 30 * MINUTE);
    f.put(&tray, "other-1-live", "live project mail", std::time::Duration::ZERO);
    let sid = "sess-backlog-unknown-start-b";
    f.attend_in_process(sid, None, &["listen", "on"]);
    let d = drain_doc(&f.attend_in_process(sid, None, &["message", "inbox", "--drain", "--json"]));
    assert_eq!(bodies(&d), vec!["live project mail".to_string()], "{d}");
    assert_eq!(d["held_back"], 1, "{d}");
}

// `attend run` has no sensor to scan the room without the peers sensor.
#[cfg(feature = "sensor-peers")]
#[test]
fn attend_run_delivers_addressed_and_announces_the_rest() {
    let f = with_backlog("backlog-run");
    {
        let run = f.run();
        run.wait_for("the addressed message and the note", Duration::from_secs(30), |r| {
            let out = r.output();
            out.contains(ADDRESSED) && out.contains(NOTE)
        });
        assert!(!run.output().contains(OLD_OPEN), "old #open backlog is not shown: {}", run.output());
        // The sensor applied the rule, and its checkpoint says so.
        run.wait_for("the checkpoint", Duration::from_secs(10), |_| {
            std::fs::read_to_string(f.state_file(&f.sid))
                .is_ok_and(|s| s.contains("other-1-addressed") && s.contains("baselined: true"))
        });
    }
    let drained = f.drain("plain");
    assert!(!drained.contains(ADDRESSED) && !drained.contains(OLD_OPEN), "{drained}");
}

/// #725 re-review, finding 2: under `/attend` the turn usually ends a few
/// seconds after `attend run` starts, before the peers sensor's first
/// message scan (its first poll only baselines peer presence, yet
/// checkpoints). A drain in that window must still apply the rule: the
/// 50 cap on addressed mail, old `#open` held back, and the note.
// `attend run` has no sensor to scan the room without the peers sensor.
#[cfg(feature = "sensor-peers")]
#[test]
fn a_drain_before_the_sensor_scans_applies_the_rule() {
    let f = Fixture::with_peers_interval("backlog-window", 30);
    let tray = f.project_tray();
    for i in 0..55 {
        f.put(&tray, &format!("other-1-to-{i:02}"), &format!("addressed {i:02}"), 10 * MINUTE + Duration::from_secs(i));
    }
    for i in 0..3 {
        f.put("_broadcast", &format!("other-1-open-{i}"), &format!("old open {i}"), 60 * MINUTE);
    }
    let run = f.run();
    run.wait_for("the sensor's first checkpoint", Duration::from_secs(10), |_| f.state_file(&f.sid).exists());
    let hook = f.drain_hook_with("{}");
    assert!(hook.contains("50 peer message(s)"), "the 50 cap: {hook}");
    assert!(hook.contains("8 older messages held back; reading them is optional: attend message inbox"), "the note: {hook}");
    assert!(!hook.contains("old open"), "old #open is held back: {hook}");
}
