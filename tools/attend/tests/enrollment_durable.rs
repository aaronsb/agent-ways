// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! Enrollment is durable (#720 review, finding 3): liveness does not undo it.
//!
//! A session that joined a channel and was then pruned from it, its
//! heartbeat stale and its Claude Code process gone, must still be enrolled
//! after a peer's `attend channel` prunes it, and its drain must still
//! deliver. Only an explicit opt-out un-enrolls it. A member whose heartbeat
//! lapsed through a long turn while its process runs on is not pruned at all
//! (ADR-129, addendum of 2026-10-13).

mod common;
use common::{age_file, Fixture, DAY, MINUTE};
use std::time::Duration;

#[test]
fn a_member_in_a_turn_longer_than_the_grace_is_not_pruned() {
    let f = Fixture::new("long-turn");
    f.ok(&["channel", "join", "side"]);
    f.drain("plain");

    // A long turn: no Stop boundary, so the heartbeat goes stale, while the
    // session's Claude Code process (this test) runs on.
    age_file(&f.heartbeat(&f.sid), 2 * MINUTE);
    f.without_session(|| f.ok(&["channel"]));
    let joined = f.ok(&["channel", "--joined"]);
    assert!(joined.contains("#side"), "a live member is kept: {joined}");
}

#[test]
fn a_stale_pruned_member_stays_enrolled_and_drained() {
    let f = Fixture::new("stale");
    // A session of another Claude Code process, which has exited since it
    // joined: a shell stands in for it, and its record goes with it.
    let gone = "sess-gone";
    f.attend_in_process(gone, Some(DAY), &["channel", "join", "side"]);
    f.attend_in_process(gone, Some(DAY), &["message", "inbox", "--drain", "--format", "plain"]);
    let fresh = f.without_session(|| f.ok(&["channel", "--json"]));
    assert!(fresh.contains(r#""name":"side","members":1"#), "a member while its heartbeat is fresh: {fresh}");
    age_file(&f.heartbeat(gone), 2 * MINUTE);
    // A peer lists channels, which prunes stale members.
    f.without_session(|| f.ok(&["channel"]));
    let members = f.without_session(|| f.ok(&["channel", "--json"]));
    assert!(!members.contains(r#""name":"side","members":1"#), "the fixture must have pruned the member: {members}");

    f.put("_broadcast", "other-1-late", "after the prune", Duration::ZERO);
    let out = f.attend_in_process(gone, Some(DAY), &["message", "inbox", "--drain", "--format", "plain"]);
    assert!(out.contains("after the prune"), "an enrolled session's drain must deliver: {out:?}");
    assert!(f.heartbeat(gone).exists());
}

#[test]
fn leaving_the_last_channel_un_enrolls_a_join_only_session() {
    let f = Fixture::new("leave");
    f.ok(&["channel", "join", "a"]);
    f.ok(&["channel", "join", "b"]);
    f.ok(&["channel", "leave", "a"]);
    assert!(f.marker(&f.sid).exists(), "still in #b");
    f.ok(&["channel", "leave", "b"]);
    assert!(!f.enrolled(&f.sid), "leaving the last channel opts out");

    f.put("_broadcast", "other-1-x", "not for a session that left", Duration::ZERO);
    assert_eq!(f.drain("plain"), "");
}

#[test]
fn scene_private_un_enrolls_when_attend_run_is_not_running() {
    let f = Fixture::new("private");
    f.ok(&["channel", "join", "a"]);
    assert!(f.marker(&f.sid).exists());
    f.ok(&["channel", "scene", "private"]);
    assert!(!f.enrolled(&f.sid));
}

/// A fresh `attend listen on` or `attend channel join` after `scene private` enrolls the
/// session again.
#[test]
fn enrolling_again_after_private_clears_the_opt_out() {
    let f = Fixture::new("privateagain");
    f.ok(&["channel", "join", "a"]);
    f.ok(&["channel", "scene", "private"]);
    f.ok(&["channel", "join", "b"]);
    f.put("_broadcast", "other-1-y", "after rejoining", Duration::ZERO);
    assert!(f.drain("plain").contains("after rejoining"));
}
