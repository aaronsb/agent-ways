// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The cache gate on the wake rule (ADR-406 items 2 and 13): an idle
//! session whose prompt cache has lapsed is never woken. A message stays
//! unread and reaches the session with the operator's next turn. The clock
//! is keepwarm's, the newest turn in the transcript at its own cache tier,
//! less a two-minute margin; the fixture's stand-in `ways` serves it for the
//! fixture's session id only, and a transcript with no reading counts as
//! lapsed. A session with no turn yet has no cache to lose and is woken.
//! Keepwarm's own wake is not gated.

mod common;
use common::Fixture;
use std::time::Duration;

const MIN: Duration = Duration::from_secs(60);

/// A listening fixture past its cold start, whose last turn was `ago` back
/// (`None`: no transcript, no turn yet), with one message addressed to it
/// pending.
fn addressed(tag: &str, ago: Option<Duration>) -> Fixture {
    let f = Fixture::new(tag);
    f.ok(&["listen", "on"]);
    f.ok(&["message", "inbox", "--drain", "--json"]);
    f.last_turn(ago);
    let dir = f.signals().join(f.project_tray());
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("x-1-1.signal"), format!("claude:x|p|/tmp/p|to:{}|for you\n", f.sid)).unwrap();
    f
}

fn wake_drain(f: &Fixture) -> serde_json::Value {
    serde_json::from_str(&f.ok(&["message", "inbox", "--drain", "--json", "--wake"])).unwrap()
}

fn bodies(d: &serde_json::Value) -> Vec<String> {
    d["messages"].as_array().unwrap().iter().map(|m| m["message"].as_str().unwrap().to_string()).collect()
}

fn unread(f: &Fixture) -> u64 {
    let u: serde_json::Value = serde_json::from_str(&f.ok(&["message", "unread", "--json"])).unwrap();
    u["unread"].as_u64().unwrap()
}

/// Warm, idle, addressed: the wake drain delivers, up to the margin before
/// the cache's hour.
#[test]
fn a_warm_idle_session_is_woken() {
    let f = addressed("cold-warm", Some(57 * MIN));
    let d = wake_drain(&f);
    assert_eq!(bodies(&d), ["for you"], "{d}");
    assert_eq!(d["cache_lapsed"], false, "{d}");
}

/// Inside the two-minute margin before the hour, the cache counts as lapsed:
/// the transcript line trails the request that refreshed it.
#[test]
fn the_last_two_minutes_of_the_hour_count_as_lapsed() {
    let f = addressed("cold-margin", Some(59 * MIN));
    let d = wake_drain(&f);
    assert_eq!(bodies(&d), Vec::<String>::new(), "{d}");
    assert_eq!(d["cache_lapsed"], true, "{d}");
}

/// A turn on the five-minute tier lapses on its own clock: four minutes
/// back is past it, where the hour tier would still be warm.
#[test]
fn a_five_minute_tier_lapses_after_its_own_minutes() {
    let f = addressed("cold-5m", None);
    f.last_turn_at_tier(&f.sid, Some(4 * MIN), "5m");
    let d = wake_drain(&f);
    assert_eq!(bodies(&d), Vec::<String>::new(), "{d}");
    assert_eq!(d["cache_lapsed"], true, "{d}");
    f.last_turn_at_tier(&f.sid, Some(MIN), "5m");
    assert_eq!(bodies(&wake_drain(&f)), ["for you"]);
}

/// Cold, idle, addressed: the wake drain delivers nothing, marks nothing and
/// says the cache has lapsed; the message stays unread for the row to count.
#[test]
fn a_cold_idle_session_is_not_woken() {
    let f = addressed("cold-cold", Some(61 * MIN));
    for _ in 0..2 {
        let d = wake_drain(&f);
        assert_eq!(bodies(&d), Vec::<String>::new(), "{d}");
        assert_eq!(d["cache_lapsed"], true, "{d}");
    }
    assert_eq!(unread(&f), 1, "the message stays unread");
}

/// Cold, then the operator starts a turn: the drain at the turn boundary
/// delivers what waited, without regard to the cache.
#[test]
fn the_operators_next_turn_delivers_what_waited() {
    let f = addressed("cold-turn", Some(61 * MIN));
    assert_eq!(bodies(&wake_drain(&f)), Vec::<String>::new());
    let d: serde_json::Value = serde_json::from_str(&f.ok(&["message", "inbox", "--drain", "--json"])).unwrap();
    assert_eq!(bodies(&d), ["for you"], "{d}");
    assert_eq!(unread(&f), 0);
}

/// Once the operator's turn has run, the cache is warm again and a wake
/// drain delivers as usual.
#[test]
fn a_session_the_operator_warmed_is_woken_again() {
    let f = addressed("cold-rewarmed", Some(61 * MIN));
    assert_eq!(bodies(&wake_drain(&f)), Vec::<String>::new());
    f.last_turn(Some(Duration::ZERO));
    assert_eq!(bodies(&wake_drain(&f)), ["for you"]);
}

/// A transcript `ways` cannot read: the last activity is unknown, which
/// counts as cold.
#[test]
fn unknown_activity_counts_as_cold() {
    let f = addressed("cold-unknown", None);
    f.unreadable_transcript(&f.sid);
    let d = wake_drain(&f);
    assert_eq!(bodies(&d), Vec::<String>::new(), "{d}");
    assert_eq!(d["cache_lapsed"], true, "{d}");
    assert_eq!(unread(&f), 1);
}

/// The operator: "a fresh claude code with no interaction should be able
/// to be woken on a message". A session with no transcript yet has had no
/// turn and has no cache to lose: an addressed message wakes it.
#[test]
fn a_session_with_no_transcript_yet_is_woken() {
    let f = addressed("cold-fresh", None);
    assert!(!f.transcript(&f.sid).exists());
    let d = wake_drain(&f);
    assert_eq!(bodies(&d), ["for you"], "{d}");
    assert_eq!(d["cache_lapsed"], false, "{d}");
}

/// A transcript with no assistant turn in it yet (the session-start
/// rows alone) is a session with no cache either: it is woken, and
/// `attend message unread` does not call the message cold.
#[test]
fn a_session_whose_transcript_holds_no_turn_is_woken() {
    let f = addressed("cold-noturn", None);
    f.no_turn_yet(&f.sid);
    assert_eq!(f.ok(&["message", "unread"]).lines().next(), Some("1 unread"));
    let d = wake_drain(&f);
    assert_eq!(bodies(&d), ["for you"], "{d}");
    assert_eq!(d["cache_lapsed"], false, "{d}");
}

/// Nothing addressed is pending: the drain reports no lapse, since it held
/// nothing for one.
#[test]
fn an_open_post_alone_reports_no_lapse() {
    let f = Fixture::new("cold-open");
    f.ok(&["listen", "on"]);
    f.ok(&["message", "inbox", "--drain", "--json"]);
    f.last_turn(Some(61 * MIN));
    let dir = f.signals().join("_broadcast");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("x-1-1.signal"), "claude:x|p|/tmp/p|to:*|on open\n").unwrap();
    let d = wake_drain(&f);
    assert_eq!(bodies(&d), Vec::<String>::new());
    assert_eq!(d["cache_lapsed"], false, "{d}");
}

/// Keepwarm's own wake is not gated: armed, 51 idle minutes on a large
/// context, the check still prints its line, and an addressed message still
/// wakes the warm session.
#[cfg(feature = "sensor-keepwarm")]
#[test]
fn the_keepwarm_wake_still_fires_at_fifty_minutes() {
    let f = addressed("cold-keepwarm", Some(51 * MIN));
    assert!(f.attend(&["keepwarm", "on"]).status.success());
    assert_eq!(f.ok(&["keepwarm", "check"]), "", "the first check baselines");
    assert!(f.ok(&["keepwarm", "check"]).starts_with("keepwarm: 51m idle"));
    assert_eq!(bodies(&wake_drain(&f)), ["for you"]);
}

/// `attend message unread` and `attend peer status` say what waits on a lapsed cache.
#[test]
fn unread_and_status_say_the_message_waits_cold() {
    let f = addressed("cold-unread", Some(61 * MIN));
    assert_eq!(f.ok(&["message", "unread"]).lines().next(), Some("1 waiting · cold"));
    let u: serde_json::Value = serde_json::from_str(&f.ok(&["message", "unread", "--json"])).unwrap();
    assert_eq!(u["cache_lapsed"], true, "{u}");
    assert!(f.ok(&["peer", "status"]).contains("1 waiting · cold"));
    f.last_turn(Some(Duration::ZERO));
    assert_eq!(f.ok(&["message", "unread"]).lines().next(), Some("1 unread"));
    assert!(f.ok(&["peer", "status"]).contains("1 unread"));
}

/// A `ways` that hangs is killed after the read's timeout, and the reading
/// counts as unknown, so the session is not woken.
#[test]
fn a_hung_ways_counts_as_lapsed() {
    let f = addressed("cold-hung", Some(Duration::ZERO));
    std::fs::write(f.home.join("bin").join("ways"), "#!/bin/sh\nexec sleep 30\n").unwrap();
    let started = std::time::Instant::now();
    let d = wake_drain(&f);
    assert!(started.elapsed() < Duration::from_secs(15), "the read gave up at its timeout: {:?}", started.elapsed());
    assert_eq!(bodies(&d), Vec::<String>::new(), "{d}");
    assert_eq!(d["cache_lapsed"], true, "{d}");
}
