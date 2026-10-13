//! The cache gate on the wake rule (ADR-406 items 1 and 13): an idle
//! session whose prompt cache has lapsed is never woken by a message. The
//! message stays unread and reaches the session with the operator's next
//! turn. The clock is keepwarm's, the newest turn in the transcript, which
//! the fixture's stand-in `ways` serves; no reading counts as lapsed.
//! Keepwarm's own wake is not gated.

mod common;
use common::Fixture;
use std::time::Duration;

const MIN: Duration = Duration::from_secs(60);

/// A listening fixture past its cold start, whose last turn was `ago` back
/// (`None`: no transcript), with one message addressed to it pending.
fn addressed(tag: &str, ago: Option<Duration>) -> Fixture {
    let f = Fixture::new(tag);
    f.ok(&["listen", "on"]);
    f.ok(&["inbox", "--drain", "--json"]);
    f.last_turn(ago);
    let dir = f.signals().join(f.project_tray());
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("x-1-1.signal"), format!("claude:x|p|/tmp/p|to:{}|for you\n", f.sid)).unwrap();
    f
}

fn wake_drain(f: &Fixture) -> serde_json::Value {
    serde_json::from_str(&f.ok(&["inbox", "--drain", "--json", "--wake"])).unwrap()
}

fn bodies(d: &serde_json::Value) -> Vec<String> {
    d["messages"].as_array().unwrap().iter().map(|m| m["message"].as_str().unwrap().to_string()).collect()
}

fn unread(f: &Fixture) -> u64 {
    let u: serde_json::Value = serde_json::from_str(&f.ok(&["unread", "--json"])).unwrap();
    u["unread"].as_u64().unwrap()
}

/// Warm, idle, addressed: the wake drain delivers, up to the last minute of
/// the cache's hour.
#[test]
fn a_warm_idle_session_is_woken() {
    let f = addressed("cold-warm", Some(59 * MIN));
    let d = wake_drain(&f);
    assert_eq!(bodies(&d), ["for you"], "{d}");
    assert_eq!(d["cache_lapsed"], false, "{d}");
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
    let d: serde_json::Value = serde_json::from_str(&f.ok(&["inbox", "--drain", "--json"])).unwrap();
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

/// No transcript to read: the last activity is unknown, which counts as cold.
#[test]
fn unknown_activity_counts_as_cold() {
    let f = addressed("cold-unknown", None);
    let d = wake_drain(&f);
    assert_eq!(bodies(&d), Vec::<String>::new(), "{d}");
    assert_eq!(d["cache_lapsed"], true, "{d}");
    assert_eq!(unread(&f), 1);
}

/// Nothing addressed is pending: the drain reports no lapse, since it held
/// nothing for one.
#[test]
fn an_open_post_alone_reports_no_lapse() {
    let f = Fixture::new("cold-open");
    f.ok(&["listen", "on"]);
    f.ok(&["inbox", "--drain", "--json"]);
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
    std::fs::write(
        f.home.join("config").join("attend").join("config.yaml"),
        "sensors:\n  keepwarm:\n    enabled: true\ncleanup:\n  enabled: false\n",
    )
    .unwrap();
    assert!(f.attend(&["keepwarm", "on"]).status.success());
    assert_eq!(f.ok(&["keepwarm", "check"]), "", "the first check baselines");
    assert!(f.ok(&["keepwarm", "check"]).starts_with("keepwarm: 51m idle"));
    assert_eq!(bodies(&wake_drain(&f)), ["for you"]);
}

/// Under `attend run`, the Monitor poller obeys the same gate: cold, the
/// addressed message prints nothing and stays unread; once the cache is
/// warm, it prints.
#[cfg(feature = "sensor-peers")]
#[test]
fn the_monitor_poller_does_not_wake_a_cold_session() {
    let f = Fixture::new("cold-monitor");
    f.last_turn(Some(61 * MIN));
    let run = f.run();
    run.wait_for("the startup line", Duration::from_secs(30), |r| r.output().contains("you are"));
    let dir = f.signals().join(f.project_tray());
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("x-1-1.signal"), format!("claude:x|p|/tmp/p|to:{}|for you\n", f.sid)).unwrap();
    // Three polls at the fixture's one-second interval.
    std::thread::sleep(Duration::from_secs(3));
    assert!(!run.output().contains("for you"), "a cold session is not woken: {}", run.output());
    assert_eq!(unread(&f), 1, "and the message stays unread");
    f.last_turn(Some(Duration::ZERO));
    run.wait_for("the message once warm", Duration::from_secs(30), |r| r.output().contains("for you"));
}
