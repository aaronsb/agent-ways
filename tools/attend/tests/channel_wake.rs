//! A post in a named channel wakes the channel's idle members (ADR-406
//! item 1), addressed or not. `#open` posts still wait for the next turn
//! unless they name the session, the sender's own session is never woken by
//! its post, and the cache gate applies: a member whose prompt cache has
//! lapsed is not woken and reads the post with its next turn.

mod common;
use common::Fixture;
use std::time::Duration;

const MIN: Duration = Duration::from_secs(60);

/// A listening member of `#room`, past its cold start, whose last turn was
/// `ago` back.
fn member(tag: &str, ago: Option<Duration>) -> Fixture {
    let f = Fixture::new(tag);
    f.ok(&["channel", "join", "room"]);
    f.ok(&["message", "inbox", "--drain", "--json"]);
    f.last_turn(ago);
    f
}

/// Write `line` as signal `id` into `room`.
fn signal(f: &Fixture, room: &str, id: &str, line: &str) {
    let dir = f.signals().join(room);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{id}.signal")), format!("{line}\n")).unwrap();
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

/// An unaddressed post in the session's channel wakes it.
#[test]
fn an_unaddressed_channel_post_wakes_an_idle_member() {
    let f = member("chwake-member", Some(Duration::ZERO));
    signal(&f, "@room", "x-1-1", "claude:x|p|/tmp/p|to:#room|news for the room");
    let d = wake_drain(&f);
    assert_eq!(bodies(&d), ["news for the room"], "{d}");
    assert_eq!(unread(&f), 0);
}

/// A channel post addressed to another member still wakes this one.
#[test]
fn a_channel_post_addressed_to_another_member_wakes_this_one() {
    let f = member("chwake-other", Some(Duration::ZERO));
    signal(&f, "@room", "x-1-1", "claude:x|p|/tmp/p|to:alpha-sess|alpha, over to you");
    assert_eq!(bodies(&wake_drain(&f)), ["alpha, over to you"]);
}

/// An unaddressed `#open` post does not wake the session; it stays unread.
#[test]
fn an_unaddressed_open_post_does_not_wake() {
    let f = member("chwake-open", Some(Duration::ZERO));
    signal(&f, "_broadcast", "x-1-1", "claude:x|p|/tmp/p|to:*|chatter on open");
    let d = wake_drain(&f);
    assert_eq!(bodies(&d), Vec::<String>::new(), "{d}");
    assert_eq!(d["cache_lapsed"], false, "{d}");
    assert_eq!(unread(&f), 1);
}

/// The sender's own post in its channel does not wake it.
#[test]
fn the_senders_own_post_does_not_wake_it() {
    let f = member("chwake-own", Some(Duration::ZERO));
    signal(&f, "@room", "own-1-1", &format!("claude:{}|p|/tmp/p|to:#room|my own post", f.sid));
    let d = wake_drain(&f);
    assert_eq!(bodies(&d), Vec::<String>::new(), "{d}");
    assert_eq!(d["cache_lapsed"], false, "{d}");
}

/// A member whose cache has lapsed is not woken: the post stays unread, the
/// drain says the cache lapsed, and the operator's next turn delivers it.
#[test]
fn a_member_with_a_lapsed_cache_is_not_woken_and_reads_it_next_turn() {
    let f = member("chwake-cold", Some(61 * MIN));
    signal(&f, "@room", "x-1-1", "claude:x|p|/tmp/p|to:#room|news for the room");
    let d = wake_drain(&f);
    assert_eq!(bodies(&d), Vec::<String>::new(), "{d}");
    assert_eq!(d["cache_lapsed"], true, "{d}");
    assert_eq!(unread(&f), 1, "the post stays unread");
    let d: serde_json::Value = serde_json::from_str(&f.ok(&["message", "inbox", "--drain", "--json"])).unwrap();
    assert_eq!(bodies(&d), ["news for the room"], "{d}");
}

/// Under `attend run`, the Monitor poller prints an unaddressed channel post,
/// which wakes the session.
#[cfg(feature = "sensor-peers")]
#[test]
fn the_monitor_poller_wakes_a_member_for_a_channel_post() {
    let f = Fixture::new("chwake-monitor");
    f.ok(&["channel", "join", "room"]);
    let run = f.run();
    run.wait_for("the startup line", Duration::from_secs(30), |r| r.output().contains("you are"));
    signal(&f, "@room", "x-1-1", "claude:x|p|/tmp/p|to:#room|news for the room");
    run.wait_for("the channel post", Duration::from_secs(30), |r| r.output().contains("news for the room"));
}

/// Under `attend run` the cache gate holds the channel post on a lapsed
/// cache; once the cache is warm, it prints.
#[cfg(feature = "sensor-peers")]
#[test]
fn the_monitor_poller_holds_a_channel_post_on_a_lapsed_cache() {
    let f = Fixture::new("chwake-monitor-cold");
    f.ok(&["channel", "join", "room"]);
    f.last_turn(Some(61 * MIN));
    let run = f.run();
    run.wait_for("the startup line", Duration::from_secs(30), |r| r.output().contains("you are"));
    signal(&f, "@room", "x-1-1", "claude:x|p|/tmp/p|to:#room|news for the room");
    // Three polls at the fixture's one-second interval.
    std::thread::sleep(Duration::from_secs(3));
    assert!(!run.output().contains("news for the room"), "a cold member is not woken: {}", run.output());
    assert_eq!(unread(&f), 1, "and the post stays unread");
    f.last_turn(Some(Duration::ZERO));
    run.wait_for("the post once warm", Duration::from_secs(30), |r| r.output().contains("news for the room"));
}
