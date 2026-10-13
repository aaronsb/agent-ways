//! Who a message is for, and where it lands (ADR-401 `to`, ADR-406 items 1,
//! 7 and 10). A reply goes to its parent's sender as a direct message, a send
//! that names someone is direct, and a send that names no one goes to
//! `#open`. A session reads a tray signal only when its `to` names it or it
//! has no `to`, and an idle session wakes only for what is addressed to it.
//!
//! The verbs read HOME and the XDG roots from the environment, which is
//! process-wide, so each in-process test holds [`LOCK`] while it points them
//! at its own fixture.

mod common;
use common::Fixture;
use std::path::Path;
use std::time::Duration;
use std::sync::{Mutex, MutexGuard};

static LOCK: Mutex<()> = Mutex::new(());

fn enter(f: &Fixture) -> MutexGuard<'static, ()> {
    let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("HOME", &f.home);
    std::env::set_var("XDG_CACHE_HOME", f.home.join(".cache"));
    std::env::set_var("XDG_CONFIG_HOME", f.home.join("config"));
    std::env::set_var("XDG_DATA_HOME", f.home.join("data"));
    std::env::set_var("USER", "fixture-human");
    std::env::remove_var("CLAUDE_SESSION_ID");
    guard
}

#[cfg(feature = "sensor-peers")]
use common::Peer;

/// Write `line` as signal `id` into `tray`.
fn signal(f: &Fixture, tray: &str, id: &str, line: &str) {
    let dir = f.signals().join(tray);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{id}.signal")), format!("{line}\n")).unwrap();
}

/// The parsed record of signal `id` in `dir`, as an owned (to, body) pair.
fn read(dir: &Path, id: &str) -> (Option<String>, String) {
    let raw = std::fs::read_to_string(dir.join(format!("{id}.signal"))).unwrap_or_else(|e| panic!("{id} in {}: {e}", dir.display()));
    let sig = agent_identity::parse_signal(agent_identity::wire_line(&raw)).expect("parses");
    (sig.to.map(str::to_string), sig.message.to_string())
}

#[cfg_attr(not(feature = "sensor-peers"), allow(dead_code))]
fn files(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir).map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default()
}

fn send(to: Option<&str>, channel: Option<&str>, text: &str) -> Result<attend::verbs::Sent, attend::verbs::SendError> {
    attend::verbs::send::send(&attend::verbs::SendRequest {
        message: text.to_string(),
        to: to.map(str::to_string),
        channel: channel.map(str::to_string),
        re: None,
    })
}

// ── send ─────────────────────────────────────────────────────────────────

/// A send that names no one goes to `#open`, `to = *`.
#[test]
fn an_untargeted_send_goes_to_open() {
    let f = Fixture::new("addr-untargeted");
    let _env = enter(&f);
    let sent = send(None, None, "anyone?").expect("sent");
    assert_eq!(sent.scope, "#open");
    assert_eq!(read(&f.signals().join("_broadcast"), &sent.id), (Some("*".to_string()), "anyone?".to_string()));
}

/// `channel: open` and `#open` reach `#open`; neither is refused as a channel
/// that does not exist.
#[test]
fn open_is_a_channel_name_for_open() {
    let f = Fixture::new("addr-open-channel");
    let _env = enter(&f);
    for name in ["open", "#open"] {
        let sent = send(None, Some(name), "to everyone").unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(sent.scope, "#open", "{name}");
        assert_eq!(read(&f.signals().join("_broadcast"), &sent.id).0.as_deref(), Some("*"), "{name}");
    }
}

/// A send naming a session, by its name or its id, is a direct message in
/// its project's tray with `to` = its session id.
// Live sessions are found by the peers sensor's process scan.
#[cfg(feature = "sensor-peers")]
#[test]
fn a_send_naming_a_session_is_direct() {
    let f = Fixture::new("addr-session");
    let _env = enter(&f);
    let peer = Peer::start(&f, "peer-sess", "peerproj");
    let name = agent_identity::Identity::for_cwd(&peer.cwd, agent_theme::ColorDepth::NoColor).nickname.to_string();
    for to in [name.as_str(), &name.to_lowercase(), "peer-sess"] {
        let sent = send(Some(to), None, "just you").unwrap_or_else(|e| panic!("{to}: {e}"));
        assert_eq!(sent.scope, "directed", "{to}");
        assert_eq!(read(&peer.tray(&f), &sent.id).0.as_deref(), Some("peer-sess"), "{to}");
    }
    assert!(files(&f.signals().join("_broadcast")).is_empty());
}

/// A send naming a person is written to their person tray, whether or not
/// attend-chat is open.
#[test]
fn a_send_naming_a_person_lands_in_the_person_tray() {
    let f = Fixture::new("addr-person");
    let _env = enter(&f);
    let sent = send(Some("fixture-human"), None, "for the operator").expect("sent");
    let tray = attend_groups::person_dir(&f.signals(), "fixture-human");
    assert_eq!(read(&tray, &sent.id), (Some("fixture-human".to_string()), "for the operator".to_string()));
}

/// A send to a project path is for every live session there.
// Live sessions are found by the peers sensor's process scan.
#[cfg(feature = "sensor-peers")]
#[test]
fn a_send_naming_a_project_is_for_every_live_session_there() {
    let f = Fixture::new("addr-project");
    let _env = enter(&f);
    let a = Peer::start(&f, "peer-a", "shared");
    let _b = Peer::start(&f, "peer-b", "shared");
    let sent = send(Some(&a.cwd), None, "both of you").expect("sent");
    let (to, _) = read(&a.tray(&f), &sent.id);
    let mut ids: Vec<&str> = to.as_deref().unwrap().split(',').collect();
    ids.sort();
    assert_eq!(ids, ["peer-a", "peer-b"]);
}

/// A name two live sessions answer to is refused, with both listed.
// Live sessions are found by the peers sensor's process scan.
#[cfg(feature = "sensor-peers")]
#[test]
fn an_ambiguous_name_is_refused() {
    let f = Fixture::new("addr-ambiguous");
    let _env = enter(&f);
    let a = Peer::start(&f, "peer-a", "twin");
    let _b = Peer::start(&f, "peer-b", "twin");
    let name = agent_identity::Identity::for_cwd(&a.cwd, agent_theme::ColorDepth::NoColor).nickname.to_string();
    match send(Some(&name), None, "which one?") {
        Err(e @ attend::verbs::SendError::AmbiguousRecipient { .. }) => {
            let why = e.to_string();
            assert!(why.contains("peer-a") && why.contains("peer-b"), "{why}");
        }
        other => panic!("expected ambiguous_recipient: {other:?}"),
    }
    assert!(files(&a.tray(&f)).is_empty());
}

/// A name that is no live session and no known person is refused.
#[test]
fn an_unknown_name_is_refused() {
    let f = Fixture::new("addr-unknown");
    let _env = enter(&f);
    assert!(matches!(send(Some("Nobody-here"), None, "hello?"), Err(attend::verbs::SendError::NoRecipient { .. })));
}

// ── reply ────────────────────────────────────────────────────────────────

#[cfg(feature = "sensor-peers")]
mod reply {
    use super::*;
    use sensor_peers::last_inbound::{record, Said};

    fn reply(text: &str) -> Result<attend::verbs::Replied, attend::verbs::SendError> {
        attend::verbs::reply(&attend::verbs::SendRequest { message: text.to_string(), ..Default::default() })
    }

    /// A reply to an `#open` post is a direct message to the poster.
    #[test]
    fn a_reply_to_an_open_post_goes_to_the_poster() {
        let f = Fixture::new("addr-open-poster");
        let _env = enter(&f);
        let peer = Peer::start(&f, "peer-sess", "peerproj");
        signal(&f, "_broadcast", "peer-sess-1-1", &format!("claude:peer-sess|peerproj|{}|to:*|who knows?", peer.cwd));
        record(&f.sid, "peer-sess-1-1", &Said::Open, "claude:peer-sess");
        let sent = reply("I do").expect("a reply").sent;
        assert_eq!(sent.scope, "directed");
        assert_eq!(read(&peer.tray(&f), &sent.id).0.as_deref(), Some("peer-sess"));
        assert_eq!(files(&f.signals().join("_broadcast")), ["peer-sess-1-1.signal"], "the reply did not go to #open");
    }

    /// A reply to a session that has exited is refused, naming the session.
    #[test]
    fn a_reply_to_an_exited_session_is_refused_naming_it() {
        let f = Fixture::new("addr-exited");
        let _env = enter(&f);
        let gone = f.home.join("goneproj").to_string_lossy().into_owned();
        signal(&f, "_broadcast", "gone-sess-1-1", &format!("claude:gone-sess|goneproj|{gone}|still there?"));
        record(&f.sid, "gone-sess-1-1", &Said::Open, "claude:gone-sess");
        match reply("too late") {
            Err(e @ attend::verbs::SendError::SenderExited { .. }) => {
                let name = agent_identity::Identity::for_cwd(&gone, agent_theme::ColorDepth::NoColor).nickname.to_string();
                let why = e.to_string();
                assert!(why.contains(&name), "names the session: {why}");
            }
            other => panic!("expected sender_exited: {other:?}"),
        }
        assert_eq!(files(&f.signals().join("_broadcast")), ["gone-sess-1-1.signal"]);
    }

    /// A reply to a post in a named channel stays in the channel, `to` the
    /// poster.
    #[test]
    fn a_reply_in_a_channel_stays_there_addressed_to_the_poster() {
        let f = Fixture::new("addr-channel");
        let _env = enter(&f);
        attend::verbs::channels::join("deploy", false).expect("join");
        attend_groups::Groups::new(&f.signals(), "peer-sess").join("deploy", false).unwrap();
        attend_presence::heartbeat::touch("peer-sess").unwrap();
        signal(&f, "@deploy", "peer-sess-1-1", "claude:peer-sess|peerproj|/tmp/peerproj|shipping?");
        record(&f.sid, "peer-sess-1-1", &Said::Channel("deploy".to_string()), "claude:peer-sess");
        let sent = reply("ship it").expect("a reply").sent;
        assert_eq!(sent.scope, "channel");
        assert_eq!(read(&f.signals().join("@deploy"), &sent.id).0.as_deref(), Some("peer-sess"));
    }

    /// A reply to a person reached through `#open` is written to their tray
    /// whether or not anything of theirs is live.
    #[test]
    fn a_reply_to_a_person_is_always_written() {
        let f = Fixture::new("addr-person-reply");
        let _env = enter(&f);
        signal(&f, "_broadcast", "aaron-kitty-1-1", "external:aaron@kitty|aaron|/home/aaron|anyone free?");
        record(&f.sid, "aaron-kitty-1-1", &Said::Open, "external:aaron@kitty");
        let sent = reply("me").expect("a reply").sent;
        assert_eq!(read(&attend_groups::person_dir(&f.signals(), "aaron"), &sent.id).0.as_deref(), Some("aaron"));
    }
}

// ── reading and waking ───────────────────────────────────────────────────

/// Two sessions share one project tray (alpha and beta). A direct message to
/// one is not read by the other; an untagged message is read by both.
#[test]
fn a_direct_message_in_a_shared_tray_is_read_only_by_its_recipient() {
    let f = Fixture::new("addr-shared-tray");
    let _env = enter(&f);
    let tray = f.project_tray();
    signal(&f, &tray, "x-1-1", &format!("claude:x|p|/tmp/p|to:{}|for me", f.sid));
    signal(&f, &tray, "x-1-2", "claude:x|p|/tmp/p|to:beta-sess|for beta");
    signal(&f, &tray, "x-1-3", "claude:x|p|/tmp/p|for the project");
    let bodies: Vec<String> = attend::verbs::inbox::list(Default::default()).messages.into_iter().map(|m| m.message).collect();
    assert_eq!(bodies, ["for me", "for the project"]);
    assert!(matches!(attend::verbs::inbox::read("x-1-2"), Err(attend::verbs::inbox::ReadError::NotFound)));
}

/// The drain reads by the same rule.
#[test]
fn the_drain_skips_another_sessions_direct_message() {
    let f = Fixture::new("addr-drain-filter");
    f.ok(&["listen", "on"]);
    let tray = f.project_tray();
    signal(&f, &tray, "x-1-1", &format!("claude:x|p|/tmp/p|to:{}|for me", f.sid));
    signal(&f, &tray, "x-1-2", "claude:x|p|/tmp/p|to:beta-sess|for beta");
    let d: serde_json::Value = serde_json::from_str(&f.ok(&["inbox", "--drain", "--json"])).unwrap();
    let bodies: Vec<&str> = d["messages"].as_array().unwrap().iter().map(|m| m["message"].as_str().unwrap()).collect();
    assert_eq!(bodies, ["for me"]);
}

/// The wake drain (`--wake`) delivers nothing and marks nothing while only
/// `#open` and channel posts are pending; once something addressed to the
/// session arrives, it delivers everything pending.
#[test]
fn the_wake_drain_waits_for_an_addressed_message() {
    let f = Fixture::new("addr-wake");
    f.ok(&["listen", "on"]);
    // The cold start is behind us before anything arrives.
    f.ok(&["inbox", "--drain", "--json"]);
    signal(&f, "_broadcast", "x-1-1", "claude:x|p|/tmp/p|to:*|on open");
    let wake = |f: &Fixture| -> serde_json::Value { serde_json::from_str(&f.ok(&["inbox", "--drain", "--json", "--wake"])).unwrap() };
    assert_eq!(wake(&f)["messages"], serde_json::json!([]), "an #open post wakes no one");
    assert_eq!(wake(&f)["messages"], serde_json::json!([]), "and is still pending");
    signal(&f, &f.project_tray(), "x-1-2", &format!("claude:x|p|/tmp/p|to:{}|for me", f.sid));
    let d = wake(&f);
    let bodies: Vec<&str> = d["messages"].as_array().unwrap().iter().map(|m| m["message"].as_str().unwrap()).collect();
    assert_eq!(bodies, ["on open", "for me"]);
}

/// `attend unread` counts what the session would read, and no more.
#[test]
fn unread_counts_only_what_the_session_reads() {
    let f = Fixture::new("addr-unread");
    f.ok(&["listen", "on"]);
    let tray = f.project_tray();
    signal(&f, &tray, "x-1-1", &format!("claude:x|p|/tmp/p|to:{}|for me", f.sid));
    signal(&f, &tray, "x-1-2", "claude:x|p|/tmp/p|to:beta-sess|for beta");
    let u: serde_json::Value = serde_json::from_str(&f.ok(&["unread", "--json"])).unwrap();
    assert_eq!(u["unread"], 1, "{u}");
}

/// Under `attend run` (Monitor), a line wakes the session, so the poller
/// prints a message only once something addressed to the session is pending.
/// An `#open` post alone stays unread; it goes out with the next one that
/// wakes.
#[cfg(feature = "sensor-peers")]
#[test]
fn the_monitor_poller_wakes_only_for_an_addressed_message() {
    let f = Fixture::new("addr-monitor-wake");
    let run = f.run();
    run.wait_for("the startup line", std::time::Duration::from_secs(30), |r| r.output().contains("you are"));
    signal(&f, "_broadcast", "x-1-1", "claude:x|p|/tmp/p|to:*|chatter on open");
    // Three polls at the fixture's one-second interval.
    std::thread::sleep(std::time::Duration::from_secs(3));
    assert!(!run.output().contains("chatter on open"), "an #open post wakes no one: {}", run.output());
    let u: serde_json::Value = serde_json::from_str(&f.ok(&["unread", "--json"])).unwrap();
    assert_eq!(u["unread"], 1, "and stays unread: {u}");
    signal(&f, &f.project_tray(), "x-1-2", &format!("claude:x|p|/tmp/p|to:{}|for you", f.sid));
    run.wait_for("the direct message", std::time::Duration::from_secs(30), |r| r.output().contains("for you"));
    assert!(run.output().contains("chatter on open"), "what was pending goes with it: {}", run.output());
}

/// `attend reply` with no `re`, as `attend_reply` sends it when the agent
/// gives no handle, answers the newest message the drain delivered by the
/// same rule: a person gets a direct message, a live session gets one in
/// its tray, an exited session is refused by name, and nothing reaches
/// `#open`.
#[cfg(feature = "sensor-peers")]
#[test]
fn a_reply_without_re_follows_the_routing_and_never_falls_back_to_open() {
    let f = Fixture::new("addr-reply-no-re");
    f.ok(&["listen", "on"]);
    f.drain("plain");
    let open = f.signals().join("_broadcast");
    let reply = |text: &str| f.attend(&["reply", "--json", "--", text]);
    let open_files = || files(&open).len();

    // An #open post from a person.
    signal(&f, "_broadcast", "aaron-kitty-1-1", "external:aaron@kitty|aaron|/home/aaron|to:*|who is free?");
    assert!(f.drain("plain").contains("who is free?"));
    let out = reply("me");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let sent: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!((sent["to"].as_str(), sent["re"].as_str()), (Some("aaron"), Some("aaron-kitty-1-1")));
    assert_eq!(read(&attend_groups::person_dir(&f.signals(), "aaron"), sent["id"].as_str().unwrap()).1, "me");
    assert_eq!(open_files(), 1, "only the post itself is in #open");

    // An #open post from a live session.
    let peer = Peer::start(&f, "peer-sess", "peerproj");
    signal(&f, "_broadcast", "peer-sess-1-2", &format!("claude:peer-sess|peerproj|{}|to:*|any ideas?", peer.cwd));
    assert!(f.drain("plain").contains("any ideas?"));
    let out = reply("one idea");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let sent: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(sent["to"].as_str(), Some("peer-sess"));
    assert!(peer.tray(&f).join(format!("{}.signal", sent["id"].as_str().unwrap())).is_file());
    assert_eq!(open_files(), 2);

    // An #open post from a session that has exited.
    signal(&f, "_broadcast", "gone-sess-1-3", "claude:gone-sess|goneproj|/tmp/goneproj|to:*|still there?");
    assert!(f.drain("plain").contains("still there?"));
    let out = reply("too late");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("which has exited"), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(open_files(), 3, "nothing fell back to #open");
}

// ── after a /clear ───────────────────────────────────────────────────────

/// `/clear` gives the session a new id. A direct message sent to the old id
/// before the drain read it is still the session's: the drain delivers it,
/// `inbox` lists it, and a reply to a message it sent under the old id
/// still wakes it.
#[test]
fn a_direct_message_to_the_id_before_a_clear_is_still_read() {
    let f = Fixture::new("addr-clear");
    let old = f.sid.clone();
    f.ok(&["listen", "on"]);
    f.drain("plain");
    signal(&f, &f.project_tray(), "x-1-1", &format!("claude:x|p|/tmp/p|to:{old}|sent before the clear"));
    f.set_session_id("sess-addr-clear-new");
    // The new id has had a turn, so its cache is warm (ADR-406 item 1).
    f.last_turn_of("sess-addr-clear-new", Some(Duration::ZERO));
    let listed = f.ok(&["inbox", "--json"]);
    assert!(listed.contains("sent before the clear"), "{listed}");
    let d: serde_json::Value = serde_json::from_str(&f.ok(&["inbox", "--drain", "--json", "--wake"])).unwrap();
    let bodies: Vec<&str> = d["messages"].as_array().unwrap().iter().map(|m| m["message"].as_str().unwrap()).collect();
    assert_eq!(bodies, ["sent before the clear"], "{d}");

    // A channel reply to a message the session sent under its old id.
    signal(&f, "_broadcast", "x-1-2", &format!("claude:x|p|/tmp/p|to:*|re:{old}-1791000000000000001-0|answering you"));
    let d: serde_json::Value = serde_json::from_str(&f.ok(&["inbox", "--drain", "--json", "--wake"])).unwrap();
    let bodies: Vec<&str> = d["messages"].as_array().unwrap().iter().map(|m| m["message"].as_str().unwrap()).collect();
    assert_eq!(bodies, ["answering you"], "a reply to the old id wakes the new one: {d}");
}

// ── siblings at one project ──────────────────────────────────────────────

/// Alpha and beta work at one project. Alpha's reply to beta's direct
/// message, with no `re`, threads to it and goes to beta.
#[cfg(feature = "sensor-peers")]
#[test]
fn a_reply_to_a_siblings_direct_message_goes_to_the_sibling() {
    let f = Fixture::new("addr-sibling");
    let beta = Peer::start(&f, "beta-sess", "proj");
    assert_eq!(beta.cwd, f.origin);
    f.ok(&["listen", "on"]);
    f.drain("plain");
    signal(&f, &f.project_tray(), "beta-sess-1-1", &format!("claude:beta-sess|proj|{}|to:{}|alpha, a question", f.origin, f.sid));
    assert!(f.drain("plain").contains("alpha, a question"));
    let out = f.attend(&["reply", "--json", "--", "an answer"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let sent: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!((sent["re"].as_str(), sent["to"].as_str()), (Some("beta-sess-1-1"), Some("beta-sess")));
    assert!(f.signals().join(f.project_tray()).join(format!("{}.signal", sent["id"].as_str().unwrap())).is_file());
}

/// Under `attend run`, beta's direct message in the shared tray stays off
/// alpha's Monitor line, even in the poll where alpha's own wakes it.
#[cfg(feature = "sensor-peers")]
#[test]
fn the_monitor_poller_leaves_a_siblings_direct_message_unread() {
    let f = Fixture::new("addr-monitor-filter");
    let run = f.run();
    run.wait_for("the startup line", std::time::Duration::from_secs(30), |r| r.output().contains("you are"));
    let tray = f.project_tray();
    signal(&f, &tray, "x-1-1", "claude:x|p|/tmp/p|to:beta-sess|FOR-BETA");
    signal(&f, &tray, "x-1-2", &format!("claude:x|p|/tmp/p|to:{}|FOR-ALPHA", f.sid));
    run.wait_for("alpha's message", std::time::Duration::from_secs(30), |r| r.output().contains("FOR-ALPHA"));
    assert!(!run.output().contains("FOR-BETA"), "{}", run.output());
}

// ── channel posts addressed to someone (PR #974 review, O2) ──────────────

/// A channel post whose `to` names alpha is read by every member, beta
/// included: `to` marks attention in a shared room and restricts no one.
/// It wakes the session it names and no other.
#[test]
fn a_channel_post_addressed_to_one_member_is_read_by_all_and_wakes_only_it() {
    let f = Fixture::new("addr-channel-to");
    f.ok(&["join", "room"]);
    f.drain("plain");
    signal(&f, "@room", "x-1-1", "claude:x|p|/tmp/p|to:alpha-sess|alpha, over to you");
    let listed = f.ok(&["inbox", "--json"]);
    assert!(listed.contains("alpha, over to you"), "beta reads it: {listed}");
    let wake = |f: &Fixture| -> serde_json::Value { serde_json::from_str(&f.ok(&["inbox", "--drain", "--json", "--wake"])).unwrap() };
    assert_eq!(wake(&f)["messages"], serde_json::json!([]), "a post addressed to alpha does not wake beta");
    signal(&f, "@room", "x-1-2", &format!("claude:x|p|/tmp/p|to:{}|beta, your turn", f.sid));
    let d = wake(&f);
    let bodies: Vec<&str> = d["messages"].as_array().unwrap().iter().map(|m| m["message"].as_str().unwrap()).collect();
    assert_eq!(bodies, ["alpha, over to you", "beta, your turn"], "addressed to beta, it wakes beta: {d}");
}

/// A send that names a channel and someone stays in the channel, addressed
/// to them; `#open` likewise.
#[cfg(feature = "sensor-peers")]
#[test]
fn a_send_with_channel_and_to_posts_in_the_channel_addressed() {
    let f = Fixture::new("addr-channel-and-to");
    let _env = enter(&f);
    let peer = Peer::start(&f, "peer-sess", "peerproj");
    attend::verbs::channels::join("room", false).expect("join");
    attend_groups::Groups::new(&f.signals(), "peer-sess").join("room", false).unwrap();
    attend_presence::heartbeat::touch("peer-sess").unwrap();
    let sent = send(Some("peer-sess"), Some("#room"), "you, in the room").expect("sent");
    assert_eq!((sent.scope, sent.to.as_str()), ("channel", "peer-sess"));
    assert_eq!(read(&f.signals().join("@room"), &sent.id).0.as_deref(), Some("peer-sess"));
    let sent = send(Some("peer-sess"), Some("open"), "you, in the open").expect("sent");
    assert_eq!((sent.scope, sent.to.as_str()), ("#open", "peer-sess"));
    assert!(files(&peer.tray(&f)).is_empty(), "nothing went to the tray");
}

/// A channel post addressed to someone outside the channel would reach no
/// one, since only members read it, so it is refused and nothing is written
/// (PR #974 re-check, E1). A member addressee still posts; `#open` has no
/// membership to check.
#[cfg(feature = "sensor-peers")]
#[test]
fn a_channel_post_to_a_non_member_is_refused() {
    let f = Fixture::new("addr-channel-nonmember");
    let _member = Peer::start(&f, "peer-sess", "peerproj");
    let _outsider = Peer::start(&f, "other-sess", "otherproj");
    f.ok(&["join", "room"]);
    attend_groups::Groups::new(&f.signals(), "peer-sess").join("room", false).unwrap();
    std::fs::create_dir_all(f.cache().join("heartbeat")).unwrap();
    std::fs::write(f.heartbeat("peer-sess"), "").unwrap();
    let room = f.signals().join("@room");
    for (to, who) in [("other-sess", "a session"), ("fixture-human", "a person")] {
        let out = f.attend(&["send", "--channel", "room", "--to", to, "--", "are you there?"]);
        assert_eq!(out.status.code(), Some(1), "{who}: {}", String::from_utf8_lossy(&out.stderr));
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains(&format!("@{to} is not in #room; nothing sent")), "{who}: {err}");
        assert!(files(&room).is_empty(), "{who}: nothing written");
    }
    let out = f.attend(&["send", "--channel", "room", "--to", "peer-sess", "--json", "--", "you, in the room"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(files(&room).len(), 1, "a member addressee posts");
    let out = f.attend(&["send", "--channel", "open", "--to", "other-sess", "--", "you, in the open"]);
    assert!(out.status.success(), "#open has no membership: {}", String::from_utf8_lossy(&out.stderr));
}
