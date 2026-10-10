//! Joining a channel late does not deliver its history (ADR-136 Decision 2,
//! the cold-start rule of ADR-172 applied per room).
//!
//! The regression: an operator cleans up a channel, its old messages that a
//! live session had not consumed survive `/purge`, and a new agent that
//! joins the channel after its first drain was handed every one of them,
//! because the cold-start rule ran only on a session's first scan. The
//! join now marks the room's old messages seen before writing the
//! membership; younger messages stay live.

mod common;
use common::{Fixture, MINUTE};
use std::time::Duration;

const OLD: &str = "testing history from yesterday";
const LIVE: &str = "testing said just now";

/// A new agent that is already warm: it enrolled through another channel
/// and its first drain applied the cold start.
fn warm_agent(tag: &str) -> Fixture {
    let f = Fixture::new(tag);
    f.ok(&["join", "newtopic"]);
    f.drain("plain");
    f
}

#[test]
fn a_late_join_holds_back_the_rooms_old_messages_and_says_so() {
    let f = warm_agent("join-backlog-drain");
    for i in 0..40 {
        f.put("@testing", &format!("other-old-{i:02}"), &format!("{OLD} {i}"), 24 * 60 * MINUTE);
    }
    f.put("@testing", "other-live", LIVE, Duration::from_secs(5));

    let joined = f.ok(&["join", "testing"]);
    assert!(joined.contains("joined #testing (40 earlier messages not shown; attend inbox)"), "{joined}");

    let drained = f.drain("plain");
    assert!(drained.contains(LIVE), "a live message in the room is still delivered: {drained}");
    assert!(!drained.contains(OLD), "the room's history is not delivered: {drained}");
    assert!(drained.contains("1 message(s) drained"), "{drained}");
    // Still there to read on demand.
    assert!(f.ok(&["inbox", "--limit", "100"]).contains(OLD));
}

#[test]
fn rejoining_a_channel_already_joined_keeps_what_is_owed() {
    let f = warm_agent("join-backlog-rejoin");
    f.ok(&["join", "testing"]);
    f.put("@testing", "other-unread", OLD, 10 * MINUTE);
    let again = f.ok(&["join", "testing"]);
    assert!(!again.contains("not shown"), "{again}");
    assert!(f.drain("plain").contains(OLD), "a member's unconsumed message is still delivered");
}

#[test]
fn a_late_join_under_attend_run_holds_back_the_history_too() {
    let f = Fixture::new("join-backlog-run");
    // An old #open message: the sensor's cold start announces it, which
    // marks its first message scan done.
    f.put("_broadcast", "other-open", "old open", 60 * MINUTE);
    let run = f.run();
    run.wait_for("the cold start", Duration::from_secs(30), |r| r.output().contains("1 earlier message not shown"));
    for i in 0..12 {
        f.put("@testing", &format!("other-old-{i:02}"), &format!("{OLD} {i}"), 24 * 60 * MINUTE);
    }
    f.ok(&["join", "testing"]);
    f.put("@testing", "other-live", LIVE, Duration::ZERO);
    // The live message is the positive control: the sensor scanned the room.
    run.wait_for("the live message", Duration::from_secs(30), |r| r.output().contains(LIVE));
    assert!(!run.output().contains(OLD), "the room's history is not delivered: {}", run.output());
}

/// Record a pending invitation for the fixture's session, as attend-chat's
/// `/invite` does.
fn invite(f: &Fixture, channel: &str) {
    attend_groups::Groups::new(&f.signals(), "").invite(channel, &f.sid).unwrap();
}

#[test]
fn a_join_that_answers_an_invitation_delivers_the_rooms_recent_history() {
    // Cold, as an invited agent usually is: its join is what enrolls it.
    let f = Fixture::new("join-invited-cold");
    for i in 0..60 {
        // Ten minutes and more old: past the cold-start window.
        f.put("@testing", &format!("other-brief-{i:02}"), &format!("briefing {i:02}"), 10 * MINUTE + Duration::from_secs(60 - i));
    }
    f.put(&f.project_tray(), "other-addressed", "addressed before the join", 30 * MINUTE);
    f.put("_broadcast", "other-open", "old open chatter", 30 * MINUTE);
    invite(&f, "testing");

    let joined = f.ok(&["join", "testing"]);
    assert!(joined.contains("invited: 50 earlier message(s) to read, 10 older not shown"), "{joined}");
    let drained = f.drain("plain");
    assert!(drained.contains("51 message(s) drained"), "the 50 newest of the room and the addressed mail: {drained}");
    assert!(drained.contains("briefing 59") && drained.contains("briefing 10"), "{drained}");
    assert!(!drained.contains("briefing 09"), "older than the newest 50: {drained}");
    assert!(drained.contains("addressed before the join"), "{drained}");
    assert!(!drained.contains("old open chatter"), "the rest of the cold start still holds: {drained}");
    // The invitation is spent: leaving and joining again is a self-join.
    f.ok(&["leave", "testing"]);
    f.put("@testing", "other-later", "later history", 10 * MINUTE);
    assert!(f.ok(&["join", "testing"]).contains("1 earlier message not shown"));
}

#[test]
fn a_warm_session_answering_an_invitation_gets_the_briefing_too() {
    let f = warm_agent("join-invited-warm");
    for i in 0..3 {
        f.put("@testing", &format!("other-brief-{i}"), &format!("briefing {i}"), 10 * MINUTE);
    }
    invite(&f, "testing");
    assert!(f.ok(&["join", "testing"]).contains("invited: 3 earlier message(s) to read"));
    let drained = f.drain("plain");
    assert!(drained.contains("briefing 0") && drained.contains("briefing 2"), "{drained}");
}
