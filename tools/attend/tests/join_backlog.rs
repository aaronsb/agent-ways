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
