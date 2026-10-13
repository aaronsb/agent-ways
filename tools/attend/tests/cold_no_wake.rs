//! The cache gate on the wake rule (ADR-406 items 1 and 13): an idle
//! session whose prompt cache has lapsed is never woken. A message stays
//! unread and reaches the session with the operator's next turn. The clock
//! is keepwarm's, the newest turn in the transcript at its own cache tier,
//! less a two-minute margin; the fixture's stand-in `ways` serves it for the
//! fixture's session id only, and no reading counts as lapsed. Keepwarm's
//! own wake is not gated.

mod common;
use common::Fixture;
use std::time::Duration;

const MIN: Duration = Duration::from_secs(60);

/// A listening fixture past its cold start, whose last turn was `ago` back
/// (`None`: no transcript), with one message addressed to it pending.
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

/// After `/clear`, a running `attend run` hands over to a process under the
/// new id (rekey), so the gate reads the new id's transcript, never the old.
#[cfg(feature = "sensor-peers")]
#[test]
fn after_a_clear_the_monitor_poller_reads_the_new_ids_clock() {
    let f = Fixture::new("cold-clear");
    let run = f.run();
    run.wait_for("the startup line", Duration::from_secs(30), |r| r.output().contains("you are"));
    // The old id's transcript stops at the clear; the new one is warm.
    f.last_turn(Some(61 * MIN));
    f.last_turn_of("sess-cold-cleared", Some(Duration::ZERO));
    f.set_session_id("sess-cold-cleared");
    run.wait_for("the hand-over", Duration::from_secs(30), |r| r.output().contains("session id changed"));
    let dir = f.signals().join(f.project_tray());
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("x-1-1.signal"), "claude:x|p|/tmp/p|to:sess-cold-cleared|after the clear\n").unwrap();
    run.wait_for("the message to the new id", Duration::from_secs(30), |r| r.output().contains("after the clear"));
}

/// Under `attend run` the event lane obeys the gate too: on a lapsed cache a
/// git observation is dropped, and once the cache is warm the next one
/// prints.
#[cfg(feature = "sensor-peers")]
#[test]
fn the_monitor_poller_prints_no_event_on_a_lapsed_cache() {
    let f = Fixture::new("cold-events");
    std::fs::write(
        f.home.join("config").join("attend").join("config.yaml"),
        "sensors:\n  context:\n    enabled: false\n  processes:\n    enabled: false\n  disclosure:\n    enabled: false\n  keepwarm:\n    enabled: false\n  peers:\n    interval: 1\n    min_interval: 1\n  git:\n    interval: 1\n    min_interval: 1\ncleanup:\n  enabled: false\n",
    )
    .unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git").args(args).current_dir(&f.origin).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "start"]);
    f.last_turn(Some(61 * MIN));
    let run = f.run();
    run.wait_for("the startup line", Duration::from_secs(30), |r| r.output().contains("you are"));
    std::thread::sleep(Duration::from_secs(2));
    git(&["checkout", "-q", "-b", "while-cold"]);
    std::thread::sleep(Duration::from_secs(4));
    assert!(!run.output().contains("branch changed"), "a cold session is not woken by an event: {}", run.output());
    f.last_turn(Some(Duration::ZERO));
    // Past the gate's recheck interval, so the warm clock is read.
    std::thread::sleep(Duration::from_secs(11));
    git(&["checkout", "-q", "-b", "while-warm"]);
    run.wait_for("the event once warm", Duration::from_secs(30), |r| r.output().contains("while-warm"));
    assert!(!run.output().contains("→ while-cold"), "the cold event was dropped: {}", run.output());
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

/// Replace the binary `attend run` runs from with different bytes, as a
/// release or a rebuild does: written aside and renamed over it.
#[cfg(feature = "sensor-peers")]
fn replace_binary(bin: &std::path::Path) {
    let mut bytes = std::fs::read(env!("CARGO_BIN_EXE_attend")).unwrap();
    bytes.push(0);
    let next = bin.with_extension("next");
    std::fs::write(&next, bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&next, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::rename(&next, bin).unwrap();
}

/// A binary change reloads `attend run`. On a lapsed cache the reload
/// prints nothing to stdout, so Monitor wakes no one; its banner goes to
/// the log.
#[cfg(feature = "sensor-peers")]
#[test]
fn a_reload_on_a_lapsed_cache_prints_nothing() {
    let f = Fixture::new("cold-reload");
    f.last_turn(Some(61 * MIN));
    let bin = f.home.join("attend-copy");
    std::fs::copy(env!("CARGO_BIN_EXE_attend"), &bin).unwrap();
    let run = f.run_from(&bin, &[]);
    run.wait_for("the startup line", Duration::from_secs(30), |r| r.output().contains("you are"));
    let before = run.output();
    replace_binary(&bin);
    let err = f.home.join("run.err");
    run.wait_for("the reload in the log", Duration::from_secs(40), |_| {
        std::fs::read_to_string(&err).is_ok_and(|e| e.contains("[attend] reloaded"))
    });
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(run.output(), before, "a cold reload adds no stdout line");
}

/// On a warm cache the reload banner still goes to Monitor.
#[cfg(feature = "sensor-peers")]
#[test]
fn a_reload_on_a_warm_cache_prints_its_banner() {
    let f = Fixture::new("warm-reload");
    let bin = f.home.join("attend-copy");
    std::fs::copy(env!("CARGO_BIN_EXE_attend"), &bin).unwrap();
    let run = f.run_from(&bin, &[]);
    run.wait_for("the startup line", Duration::from_secs(30), |r| r.output().contains("you are"));
    replace_binary(&bin);
    run.wait_for("the reload banner", Duration::from_secs(40), |r| r.output().contains("[attend] reloaded"));
}
