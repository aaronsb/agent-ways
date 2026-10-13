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
use std::path::{Path, PathBuf};
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

/// A live Claude Code session `sid` at `<home>/<project>`: a process whose
/// argv names claude, with a session record. Killed on drop. Only the peers
/// sensor's process scan finds it.
#[cfg_attr(not(feature = "sensor-peers"), allow(dead_code))]
struct Peer {
    child: std::process::Child,
    cwd: String,
}

#[cfg_attr(not(feature = "sensor-peers"), allow(dead_code))]
impl Peer {
    fn start(f: &Fixture, sid: &str, project: &str) -> Peer {
        let cwd = f.home.join(project);
        std::fs::create_dir_all(&cwd).unwrap();
        let cwd = cwd.to_string_lossy().into_owned();
        let child = std::process::Command::new("sh").args(["-c", "sleep 60; :", "claude"]).spawn().unwrap();
        std::fs::write(
            f.home.join(".claude").join("sessions").join(format!("{}.json", child.id())),
            format!(r#"{{"pid":{},"sessionId":"{sid}","cwd":"{cwd}"}}"#, child.id()),
        )
        .unwrap();
        Peer { child, cwd }
    }

    fn tray(&self, f: &Fixture) -> PathBuf {
        f.signals().join(claude_sessions::attend_key(&self.cwd))
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

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
