//! `attend message delete` marks what no live recipient still has to read,
//! and every reader skips what is marked (ADR-404 D3e).
//!
//! Recipients are written into the fixture's cache as attend writes them:
//! a heartbeat, an enrollment record, a seen-set, a Claude Code session
//! record naming the project. Their ids are UUIDs, the shape of a Claude
//! Code session id.

mod common;
use common::{age_file, Fixture, MINUTE};
use serde_json::Value;
use std::time::Duration;

const R1: &str = "11111111-1111-4111-8111-111111111111";
const R2: &str = "22222222-2222-4222-8222-222222222222";
const R3: &str = "33333333-3333-4333-8333-333333333333";

/// Write `line` as signal `id` into `room`, `age` old.
fn signal(f: &Fixture, room: &str, id: &str, line: &str, age: Duration) {
    let dir = f.signals().join(room);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{id}.signal"));
    std::fs::write(&path, format!("{line}\n")).unwrap();
    age_file(&path, age);
}

/// A post from another session.
fn post(f: &Fixture, room: &str, id: &str, body: &str, age: Duration) {
    signal(f, room, id, &format!("claude:other-session|elsewhere|/tmp/elsewhere|{body}"), age);
}

/// Session `sid` heartbeating `ago` back.
fn heartbeat(f: &Fixture, sid: &str, ago: Duration) {
    let path = f.heartbeat(sid);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "").unwrap();
    age_file(&path, ago);
}

/// Session `sid` listening, alive, working at `project`.
fn listener(f: &Fixture, sid: &str, project: &str) {
    heartbeat(f, sid, Duration::ZERO);
    let marker = f.marker(sid);
    std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
    std::fs::write(&marker, "listen\n").unwrap();
    std::fs::write(
        f.home.join(".claude").join("sessions").join(format!("rec-{sid}.json")),
        format!(r#"{{"pid":999999,"sessionId":"{sid}","cwd":"{project}"}}"#),
    )
    .unwrap();
}

/// Session `sid` has read `read` and is past its cold start.
fn read(f: &Fixture, sid: &str, read: &[&str]) {
    let path = f.state_file(sid);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let lines: String = read.iter().map(|id| format!("seen_signal: {id}.signal\n")).collect();
    std::fs::write(&path, format!("{lines}baselined: true\n")).unwrap();
}

fn delete(f: &Fixture, args: &[&str]) -> Value {
    let mut argv = vec!["message", "delete", "--json"];
    argv.extend_from_slice(args);
    serde_json::from_str(&f.ok(&argv)).unwrap()
}

fn marked(v: &Value) -> Vec<String> {
    let mut ids: Vec<String> = v["ids"].as_array().unwrap().iter().map(|i| i.as_str().unwrap().to_string()).collect();
    ids.sort();
    ids
}

fn kept(v: &Value, sid: &str) -> u64 {
    v["held"].as_array().unwrap().iter().find(|h| h["session"] == sid).map_or(0, |h| h["kept"].as_u64().unwrap())
}

#[test]
fn a_live_recipients_unread_message_is_kept_and_its_read_one_marked() {
    let f = Fixture::new("del-live");
    listener(&f, R1, "/srv/r1");
    read(&f, R1, &["read-1"]);
    post(&f, "_broadcast", "read-1", "already read", 10 * MINUTE);
    post(&f, "_broadcast", "unread-1", "not yet read", 10 * MINUTE);

    let v = delete(&f, &["--open"]);
    assert_eq!(marked(&v), ["read-1"]);
    assert_eq!(kept(&v, R1), 1);
    assert!(v["batch"].is_string());
    assert!(f.signals().join("_broadcast/read-1.signal").exists(), "a delete marks and removes no file");

    let listing = f.ok(&["message", "inbox"]);
    assert!(!listing.contains("already read"), "a marked message leaves the listing: {listing}");
    assert!(listing.contains("not yet read"), "{listing}");
}

#[test]
fn a_dead_recipient_holds_nothing() {
    let f = Fixture::new("del-dead");
    listener(&f, R2, "/srv/r2");
    read(&f, R2, &[]);
    heartbeat(&f, R2, 10 * MINUTE);
    post(&f, "_broadcast", "old-1", "for a session that has gone", 10 * MINUTE);
    assert_eq!(marked(&delete(&f, &["--open"])), ["old-1"]);
}

/// A listening session with no seen-set holds what its cold start would
/// deliver, a message younger than the fresh window, and not the backlog
/// the cold start holds back.
#[test]
fn a_session_with_no_seen_set_holds_what_its_cold_start_delivers() {
    let f = Fixture::new("del-cold");
    listener(&f, R3, "/srv/r3");
    post(&f, "_broadcast", "fresh-1", "inside the fresh window", Duration::from_secs(100));
    post(&f, "_broadcast", "backlog-1", "held back by the cold start", 10 * MINUTE);
    let v = delete(&f, &["--open"]);
    assert_eq!(marked(&v), ["backlog-1"]);
    assert_eq!(kept(&v, R3), 1);
}

/// A person in attend-chat heartbeats under their member id and keeps no
/// seen-set; a message their cold start would show is still marked.
#[test]
fn a_person_holds_nothing() {
    let f = Fixture::new("del-human");
    heartbeat(&f, "fixture-human", Duration::ZERO);
    post(&f, "_broadcast", "fresh-1", "the person has not read this", Duration::from_secs(100));
    // Outside a session, so the fixture's own running session holds nothing.
    let v = f.without_session(|| delete(&f, &["--open"]));
    assert_eq!(marked(&v), ["fresh-1"]);
    assert_eq!(v["held"], serde_json::json!([]));
}

#[test]
fn a_message_inside_the_grace_floor_is_never_marked() {
    let f = Fixture::new("del-grace");
    post(&f, "_broadcast", "new-1", "thirty seconds old", Duration::from_secs(30));
    post(&f, "_broadcast", "old-1", "two minutes old", 2 * MINUTE);
    assert_eq!(marked(&delete(&f, &["--open"])), ["old-1"]);
}

#[test]
fn the_cutoff_keeps_what_is_newer() {
    let f = Fixture::new("del-cutoff");
    post(&f, "_broadcast", "hour-1", "an hour old", 60 * MINUTE);
    post(&f, "_broadcast", "day-1", "two days old", 48 * 60 * MINUTE);
    assert_eq!(marked(&delete(&f, &["--open", "--older-than", "1d", "--dry-run"])), ["day-1"]);
    assert!(!f.cache().join("signals/_tombstones/messages").exists(), "a dry run marks nothing");
    assert_eq!(marked(&delete(&f, &["--open", "--older-than", "all"])), ["day-1", "hour-1"]);
}

/// A project tray is outside `--open`, and in a wider scope a direct
/// message to another session at the project is not the live one's to keep.
#[test]
fn another_sessions_tray_mail_is_outside_the_live_ones_hold() {
    let f = Fixture::new("del-tray");
    let project = "/srv/shared";
    let tray = claude_sessions::attend_key(project);
    listener(&f, R1, project);
    read(&f, R1, &[]);
    let to = |id: &str, sid: &str, body: &str| {
        signal(&f, &tray, id, &format!("claude:other-session|elsewhere|/tmp/elsewhere|to:{sid}|{body}"), 10 * MINUTE)
    };
    to("mine-1", R1, "for the live session");
    to("theirs-1", R2, "for a session that has gone");

    assert_eq!(marked(&delete(&f, &["--open"])), Vec::<String>::new(), "--open leaves project trays alone");
    let v = delete(&f, &["--everywhere"]);
    assert_eq!(marked(&v), ["theirs-1"]);
    assert_eq!(kept(&v, R1), 1);
}

#[test]
fn a_channel_scope_names_one_channel() {
    let f = Fixture::new("del-channel");
    post(&f, "@deploy", "deploy-1", "in deploy", 10 * MINUTE);
    post(&f, "_broadcast", "open-1", "in open", 10 * MINUTE);
    let v = delete(&f, &["--channel", "deploy"]);
    assert_eq!(marked(&v), ["deploy-1"]);
    assert_eq!(v["scope"], "#deploy");
    let out = f.attend(&["message", "delete", "--channel", "nowhere"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no channel named #nowhere"));
}

#[test]
fn undo_takes_the_newest_delete_back() {
    let f = Fixture::new("del-undo");
    post(&f, "_broadcast", "a-1", "first", 10 * MINUTE);
    let first = delete(&f, &["--open"]);
    post(&f, "_broadcast", "b-1", "second", 10 * MINUTE);
    std::thread::sleep(Duration::from_millis(5));
    delete(&f, &["--open"]);

    let undone: Value = serde_json::from_str(&f.ok(&["message", "delete", "--undo", "--json"])).unwrap();
    assert_eq!(undone["unmarked"], 1);
    let listing = f.ok(&["message", "inbox"]);
    assert!(listing.contains("second") && !listing.contains("first"), "{listing}");

    let batch = first["batch"].as_str().unwrap();
    f.ok(&["message", "delete", "--undo", batch]);
    assert!(f.ok(&["message", "inbox"]).contains("first"));
    let out = f.attend(&["message", "delete", "--undo"]);
    assert_eq!(out.status.code(), Some(1), "nothing left to undo");
}

/// The drain passes over a marked message without delivering it or
/// recording it read, so an undo brings it back to the next drain.
#[test]
fn the_drain_skips_a_marked_message_until_it_is_undone() {
    let f = Fixture::new("del-drain");
    post(&f, "_broadcast", "gone-1", "marked before the join", Duration::from_secs(100));
    // Deleted while this session was not running, so it held nothing.
    f.without_session(|| delete(&f, &["--open"]));
    post(&f, "_broadcast", "kept-1", "posted after the delete", Duration::from_secs(100));
    f.ok(&["channel", "join", "side"]);

    let unread: Value = serde_json::from_str(&f.ok(&["message", "unread", "--json"])).unwrap();
    assert_eq!(unread["unread"], 1, "{unread}");
    let drained = f.drain("plain");
    assert!(drained.contains("posted after the delete"), "{drained}");
    assert!(!drained.contains("marked before the join"), "{drained}");
    let state = std::fs::read_to_string(f.state_file(&f.sid)).unwrap();
    assert!(!state.contains("gone-1.signal"), "a marked message is not recorded read: {state}");

    f.ok(&["message", "delete", "--undo"]);
    let drained = f.drain("plain");
    assert!(drained.contains("marked before the join"), "undo restores delivery: {drained}");
}

/// The Monitor conduit skips a marked message too.
#[cfg(feature = "sensor-peers")]
#[test]
fn the_peers_sensor_skips_a_marked_message() {
    let f = Fixture::new("del-sensor");
    let tray = f.project_tray();
    post(&f, &tray, "gone-1", "marked before the run", Duration::from_secs(100));
    f.without_session(|| delete(&f, &["--everywhere"]));
    post(&f, &tray, "kept-1", "posted after the delete", Duration::from_secs(100));
    let run = f.run();
    run.wait_for("the unmarked message", Duration::from_secs(20), |r| r.output().contains("posted after the delete"));
    assert!(!run.output().contains("marked before the run"), "{}", run.output());
    f.ok(&["message", "delete", "--undo"]);
    run.wait_for("the message the undo restored", Duration::from_secs(20), |r| r.output().contains("marked before the run"));
}

/// `--dm` takes both halves of the thread between the caller and a session
/// and nothing else that session said.
#[test]
fn a_dm_scope_takes_both_halves_of_the_thread_and_no_third_partys() {
    let f = Fixture::new("del-dm");
    let me = f.sid.clone();
    let mine = f.project_tray();
    let theirs = claude_sessions::attend_key("/srv/r1");
    let line = |from: &str, to: &str, body: &str| format!("claude:{from}|p|/srv/p|to:{to}|{body}");
    signal(&f, &mine, "in-1", &line(R1, &me, "R1 to me"), 10 * MINUTE);
    signal(&f, &theirs, "out-1", &line(&me, R1, "me to R1"), 10 * MINUTE);
    signal(&f, &theirs, "third-1", &line(R2, R1, "R2 to R1"), 10 * MINUTE);
    signal(&f, &theirs, "other-1", &line(R1, R2, "R1 to R2"), 10 * MINUTE);
    signal(&f, &mine, "shared-1", &line(R1, &format!("{me},{R2}"), "R1 to me and R2"), 10 * MINUTE);
    post(&f, "_broadcast", "open-1", "an #open post", 10 * MINUTE);
    // The caller runs, and has read what was sent to it.
    read(&f, &me, &["in-1", "shared-1"]);

    let v = delete(&f, &["--dm", R1]);
    assert_eq!(marked(&v), ["in-1", "out-1"]);
    assert_eq!(v["scope"], format!("dm {R1}"));
}

/// Outside a Claude Code session the caller is the person at the terminal,
/// whose direct messages sit in their person tray.
#[test]
fn a_persons_dm_thread_is_read_from_their_tray() {
    let f = Fixture::new("del-dm-person");
    let line = |from: &str, to: &str, body: &str| format!("claude:{from}|p|/srv/p|to:{to}|{body}");
    signal(&f, "_people/fixture-human", "in-1", &line(R1, "fixture-human", "R1 to the person"), 10 * MINUTE);
    signal(&f, "_people/someone-else", "else-1", &line(R1, "someone-else", "R1 to someone else"), 10 * MINUTE);
    let v = f.without_session(|| delete(&f, &["--dm", R1]));
    assert_eq!(marked(&v), ["in-1"]);
}

#[test]
fn from_narrows_a_scope_to_one_sender() {
    let f = Fixture::new("del-from");
    signal(&f, "_broadcast", "r1-1", &format!("claude:{R1}|p|/srv/p|from R1"), 10 * MINUTE);
    signal(&f, "_broadcast", "r2-1", &format!("claude:{R2}|p|/srv/p|from R2"), 10 * MINUTE);
    signal(&f, "@deploy", "r1-2", &format!("claude:{R1}|p|/srv/p|from R1 in deploy"), 10 * MINUTE);
    assert_eq!(marked(&delete(&f, &["--open", "--from", R1])), ["r1-1"]);
    assert_eq!(marked(&delete(&f, &["--everywhere", "--from", R1])), ["r1-2"]);
}

/// The caller's thread includes what reached it under the id it had before
/// `/clear`.
#[test]
fn a_dm_scope_reads_the_callers_earlier_ids() {
    let f = Fixture::new("del-dm-clear");
    std::fs::create_dir_all(f.marker(&f.sid).parent().unwrap()).unwrap();
    std::fs::write(f.marker(&f.sid), "listen\nprevious sess-before-clear\n").unwrap();
    let line = |from: &str, to: &str, body: &str| format!("claude:{from}|p|/srv/p|to:{to}|{body}");
    signal(&f, &f.project_tray(), "old-1", &line(R1, "sess-before-clear", "R1 to my earlier id"), 10 * MINUTE);
    read(&f, &f.sid, &["old-1"]);
    assert_eq!(marked(&delete(&f, &["--dm", R1])), ["old-1"]);
}

/// Session `sid` ran at `/srv/gone` and its process has exited.
fn exited(f: &Fixture, sid: &str) {
    std::fs::write(
        f.home.join(".claude").join("sessions").join(format!("gone-{sid}.json")),
        format!(r#"{{"pid":999999,"sessionId":"{sid}","cwd":"/srv/gone"}}"#),
    )
    .unwrap();
}

fn tombstone(f: &Fixture, args: &[&str]) -> std::process::Output {
    let mut argv = vec!["message", "tombstone"];
    argv.extend_from_slice(args);
    f.attend(&argv)
}

#[test]
fn a_tombstone_refuses_a_live_session_and_its_earlier_ids() {
    let f = Fixture::new("tomb-live");
    listener(&f, R1, "/srv/r1");
    std::fs::write(f.marker(R1), "listen\nprevious 44444444-4444-4444-8444-444444444444\n").unwrap();
    let out = tombstone(&f, &[R1]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("has not exited"), "{}", String::from_utf8_lossy(&out.stderr));
    let out = tombstone(&f, &["44444444-4444-4444-8444-444444444444"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("earlier id of live session"));
    let out = tombstone(&f, &[&f.sid]);
    assert_eq!(out.status.code(), Some(1), "the caller's own id");
    assert_eq!(f.ok(&["message", "tombstone", "--list"]).trim(), "no tombstones");
}

/// Only an agent's session id is tombstoned, and only once this host has
/// seen its process exit: a person's member id, or a session nothing says
/// has gone, is refused.
#[test]
fn a_tombstone_needs_an_agent_that_has_exited() {
    let f = Fixture::new("tomb-exited");
    let out = tombstone(&f, &["fixture-human"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not a session id"), "{}", String::from_utf8_lossy(&out.stderr));
    let out = tombstone(&f, &[R3]);
    assert_eq!(out.status.code(), Some(1), "nothing says R3 has exited");
    assert!(String::from_utf8_lossy(&out.stderr).contains("has not exited"));
    exited(&f, R3);
    f.ok(&["message", "tombstone", R3]);
}

/// The drain reads a tombstoned session's messages without delivering
/// them, the listing leaves them out, and once the tombstone is removed
/// what that session sends is delivered again.
#[test]
fn a_tombstoned_sessions_messages_are_read_and_not_shown_until_undone() {
    let f = Fixture::new("tomb-drain");
    let tray = f.project_tray();
    exited(&f, R2);
    f.ok(&["message", "tombstone", R2]);
    assert_eq!(f.ok(&["message", "tombstone", "--list"]).trim(), R2);
    signal(&f, &tray, "dead-1", &format!("claude:{R2}|p|/srv/p|from the tombstoned session"), Duration::from_secs(100));
    post(&f, &tray, "live-1", "from someone else", Duration::from_secs(100));
    // Sent after the tombstone: the session came back (--resume), so it shows.
    std::thread::sleep(Duration::from_millis(20));
    signal(&f, &tray, "back-1", &format!("claude:{R2}|p|/srv/p|sent after the tombstone"), Duration::ZERO);
    f.ok(&["channel", "join", "side"]);

    assert!(!f.ok(&["message", "inbox"]).contains("from the tombstoned session"));
    let drained = f.drain("plain");
    assert!(drained.contains("from someone else"), "{drained}");
    assert!(drained.contains("sent after the tombstone"), "{drained}");
    assert!(!drained.contains("from the tombstoned session"), "{drained}");
    let state = std::fs::read_to_string(f.state_file(&f.sid)).unwrap();
    assert!(state.contains("seen_signal: dead-1.signal"), "read without being shown: {state}");

    f.ok(&["message", "tombstone", R2, "--undo"]);
    signal(&f, &tray, "dead-2", &format!("claude:{R2}|p|/srv/p|after the undo"), Duration::ZERO);
    let drained = f.drain("plain");
    assert!(drained.contains("after the undo"), "{drained}");
    assert!(!drained.contains("from the tombstoned session"), "what was read stays read: {drained}");
}

/// The Monitor conduit skips a tombstoned session too.
#[cfg(feature = "sensor-peers")]
#[test]
fn the_peers_sensor_skips_a_tombstoned_session() {
    let f = Fixture::new("tomb-sensor");
    let tray = f.project_tray();
    exited(&f, R2);
    f.ok(&["message", "tombstone", R2]);
    signal(&f, &tray, "dead-1", &format!("claude:{R2}|p|/srv/p|from the tombstoned session"), Duration::from_secs(100));
    post(&f, &tray, "live-1", "from someone else", Duration::from_secs(100));
    let run = f.run();
    run.wait_for("the other message", Duration::from_secs(20), |r| r.output().contains("from someone else"));
    assert!(!run.output().contains("from the tombstoned session"), "{}", run.output());
}

/// A sleeping child process that stands in for a running Claude Code
/// process. Killed on drop.
struct Running(std::process::Child);

impl Running {
    fn start() -> Self {
        Running(std::process::Command::new("sleep").arg("120").spawn().unwrap())
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.0.kill().ok();
        self.0.wait().ok();
    }
}

/// A listening session whose enrollment names a running process under a
/// host name this host no longer has (a renamed laptop): nothing says it
/// exited, so it holds what it has not read.
#[test]
fn a_session_this_host_cannot_judge_holds_its_mail() {
    let f = Fixture::new("del-unknown");
    let proc = Running::start();
    let pid = proc.0.id();
    let start = attend_presence::process::start_time(pid).expect("a start time for the child");
    listener(&f, R1, "/srv/r1");
    // No session record on this host names it: it runs elsewhere.
    std::fs::remove_file(f.home.join(".claude").join("sessions").join(format!("rec-{R1}.json"))).unwrap();
    heartbeat(&f, R1, 10 * MINUTE);
    read(&f, R1, &[]);
    post(&f, "_broadcast", "unread-1", "not yet read", 10 * MINUTE);

    std::fs::write(f.marker(R1), format!("listen\nclaude {pid}-{start}@old-hostname\n")).unwrap();
    let v = delete(&f, &["--open", "--dry-run"]);
    assert_eq!(marked(&v), Vec::<String>::new(), "{v}");
    assert_eq!(kept(&v, R1), 1);

    // Control: the same record under this host's name reads as running.
    let host = attend_presence::process::host_name();
    std::fs::write(f.marker(R1), format!("listen\nclaude {pid}-{start}@{host}\n")).unwrap();
    assert_eq!(kept(&delete(&f, &["--open", "--dry-run"]), R1), 1);
}

/// A Claude Code session that runs and does not listen holds what its cold
/// start would deliver once it listens: addressed mail sent since it began.
#[test]
fn a_running_session_that_does_not_listen_holds_its_mail() {
    // PID 1 stands in for the session's process: it runs, and it started
    // long enough ago for its record to predate the message.
    let Some(began) = attend_presence::process::started_at(1) else { return };
    let Ok(up) = began.elapsed() else { return };
    if up < 15 * MINUTE {
        eprintln!("skipped: pid 1 started {up:?} ago, too recently to date a session before the message");
        return;
    }
    let f = Fixture::new("del-running");
    let project = "/srv/running";
    let started = (std::time::SystemTime::now() - 10 * MINUTE).max(began + Duration::from_secs(1));
    let started = started.duration_since(std::time::UNIX_EPOCH).unwrap().as_millis();
    std::fs::write(
        f.home.join(".claude").join("sessions").join("pid1.json"),
        format!(r#"{{"pid":1,"sessionId":"{R2}","cwd":"{project}","startedAt":{started}}}"#),
    )
    .unwrap();
    let tray = claude_sessions::attend_key(project);
    signal(&f, &tray, "dm-1", &format!("claude:{R3}|o|/tmp/o|to:{R2}|dm for R2"), 5 * MINUTE);
    let v = delete(&f, &["--everywhere"]);
    assert_eq!(marked(&v), Vec::<String>::new(), "{v}");
    assert_eq!(kept(&v, R2), 1);
}

/// The caller's own running session is a recipient like any other.
#[test]
fn the_callers_own_unread_mail_is_kept() {
    let f = Fixture::new("del-caller");
    post(&f, "_broadcast", "fresh-1", "the caller has not read this", Duration::from_secs(100));
    let v = delete(&f, &["--open"]);
    assert_eq!(marked(&v), Vec::<String>::new());
    assert_eq!(kept(&v, &f.sid), 1);
}

/// Once cleanup has removed a delete's messages, its undo says so and
/// undoes no older delete in its place.
#[test]
fn undo_after_cleanup_says_what_became_of_the_delete() {
    let f = Fixture::new("del-reaped");
    std::fs::create_dir_all(f.home.join(".claude/projects").join(claude_sessions::project_slug(&f.origin))).unwrap();
    let tray = f.project_tray();
    post(&f, &tray, "older-1", "an older delete", 10 * MINUTE);
    f.without_session(|| delete(&f, &["--everywhere"]));
    std::thread::sleep(Duration::from_millis(5));
    post(&f, &tray, "newer-1", "the newest delete", 10 * MINUTE);
    let newest = f.without_session(|| delete(&f, &["--everywhere"]));
    let batch = newest["batch"].as_str().unwrap().to_string();

    // Its mark is eight days old and its record past the grace window.
    let marks = f.signals().join("_tombstones");
    age_file(&marks.join("messages/newer-1.signal"), 8 * 24 * 60 * MINUTE);
    age_file(&marks.join("batches").join(&batch), 60 * MINUTE);
    f.ok(&["message", "cleanup"]);
    assert!(!f.signals().join(&tray).join("newer-1.signal").exists());

    let out = f.attend(&["message", "delete", "--undo"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains(&format!("the newest delete, {batch}")), "{err}");
    assert!(f.signals().join(&tray).join("older-1.signal").exists());
    assert!(!f.ok(&["message", "inbox"]).contains("an older delete"), "the older delete stands");

    let out = f.attend(&["message", "delete", "--undo", &batch]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("nothing to restore"));
}

/// Reading a deleted message by its id says it was deleted and how to bring
/// it back; so does one a tombstoned session sent.
#[test]
fn reading_a_deleted_message_says_how_to_restore_it() {
    let f = Fixture::new("del-read");
    post(&f, "_broadcast", "gone-1", "deleted", 10 * MINUTE);
    let batch = f.without_session(|| delete(&f, &["--open"]))["batch"].as_str().unwrap().to_string();
    let out = f.attend(&["message", "inbox", "gone-1"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains(&format!("gone-1 was deleted; attend message delete --undo {batch} restores it")), "{err}");
    let json: Value = serde_json::from_slice(&f.attend(&["message", "inbox", "gone-1", "--json"]).stdout).unwrap();
    assert_eq!(json["error"]["kind"], "deleted");

    exited(&f, R2);
    f.ok(&["message", "tombstone", R2]);
    std::thread::sleep(Duration::from_millis(20));
    signal(&f, "_broadcast", "dead-1", &format!("claude:{R2}|p|/srv/p|from the tombstoned session"), MINUTE);
    let out = f.attend(&["message", "inbox", "dead-1"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains(&format!("attend message tombstone {R2} --undo restores it")), "{err}");
}
