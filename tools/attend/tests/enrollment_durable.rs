//! Enrollment is durable (#720 review, finding 3): liveness does not undo it.
//!
//! A session that joined a channel and then ran one turn longer than the
//! 90 s heartbeat grace must still be enrolled after a peer's
//! `attend channels` prunes it from the channel, and its drain must still
//! deliver. Only an explicit opt-out un-enrolls it.

mod common;
use common::{age_file, Fixture, MINUTE};
use std::time::Duration;

#[test]
fn a_stale_pruned_member_stays_enrolled_and_drained() {
    let f = Fixture::new("stale");
    f.ok(&["join", "side"]);
    f.drain("plain");

    // A long turn: no Stop boundary, so the heartbeat goes stale.
    age_file(&f.heartbeat(&f.sid), 2 * MINUTE);
    // A peer lists channels, which prunes stale members.
    f.without_session(|| f.ok(&["channels"]));
    let joined = f.ok(&["channels", "--joined"]);
    assert!(!joined.contains("#side"), "the fixture must have pruned the member: {joined}");

    f.put("_broadcast", "other-1-late", "after the prune", Duration::ZERO);
    let out = f.drain("plain");
    assert!(out.contains("after the prune"), "an enrolled session's drain must deliver: {out:?}");
    assert!(f.heartbeat(&f.sid).exists());
}

#[test]
fn leaving_the_last_channel_un_enrolls_a_join_only_session() {
    let f = Fixture::new("leave");
    f.ok(&["join", "a"]);
    f.ok(&["join", "b"]);
    f.ok(&["leave", "a"]);
    assert!(f.marker(&f.sid).exists(), "still in #b");
    f.ok(&["leave", "b"]);
    assert!(!f.marker(&f.sid).exists(), "leaving the last channel opts out");

    f.put("_broadcast", "other-1-x", "not for a session that left", Duration::ZERO);
    assert_eq!(f.drain("plain"), "");
}

#[test]
fn scene_private_un_enrolls_when_attend_run_is_not_running() {
    let f = Fixture::new("private");
    f.ok(&["join", "a"]);
    assert!(f.marker(&f.sid).exists());
    f.ok(&["scene", "private"]);
    assert!(!f.marker(&f.sid).exists());
}

#[test]
fn leaving_every_channel_keeps_a_session_that_ran_attend() {
    let f = Fixture::new("runleave");
    {
        let run = f.run();
        run.wait_for("enrollment by attend run", Duration::from_secs(20), |_| f.marker(&f.sid).exists());
    }
    f.ok(&["join", "a"]);
    f.ok(&["leave", "a"]);
    assert!(f.marker(&f.sid).exists(), "attend run enrolled it; leave does not undo that");
}

/// #725 re-review, finding 3: `scene private` under a live `attend run`
/// is recorded, and takes effect once the run is gone.
#[test]
fn scene_private_under_a_live_run_sticks() {
    let f = Fixture::new("privaterun");
    f.ok(&["join", "a"]);
    {
        let run = f.run();
        run.wait_for("enrollment by attend run", Duration::from_secs(20), |_| {
            std::fs::read_to_string(f.marker(&f.sid)).is_ok_and(|m| m.contains("run"))
        });
        f.ok(&["scene", "private"]);
    }
    f.put("_broadcast", "other-1-x", "after opting out", Duration::ZERO);
    assert_eq!(f.drain("plain"), "", "the opt-out holds once the run is gone");
    assert!(!f.marker(&f.sid).exists());
}

/// A fresh `attend run` or `attend join` after `scene private` enrolls the
/// session again.
#[test]
fn enrolling_again_after_private_clears_the_opt_out() {
    let f = Fixture::new("privateagain");
    f.ok(&["join", "a"]);
    f.ok(&["scene", "private"]);
    f.ok(&["join", "b"]);
    f.put("_broadcast", "other-1-y", "after rejoining", Duration::ZERO);
    assert!(f.drain("plain").contains("after rejoining"));
}
