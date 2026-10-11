//! The message verbs called as a library, in this test process, the way a
//! long-lived frontend (an MCP server, a mod) calls them (ADR-501 item 4).
//!
//! The verbs read HOME and the XDG roots from the environment, which is
//! process-wide, so each test holds [`LOCK`] while it points them at its
//! own fixture.

mod common;
use common::Fixture;
use std::sync::{Mutex, MutexGuard};

static LOCK: Mutex<()> = Mutex::new(());

const OPEN: sensor_peers::last_inbound::Said = sensor_peers::last_inbound::Said::Open;

/// Point this process at `f`'s sandbox for the life of the guard.
fn enter(f: &Fixture) -> MutexGuard<'static, ()> {
    let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("HOME", &f.home);
    std::env::set_var("XDG_CACHE_HOME", f.home.join(".cache"));
    std::env::set_var("XDG_CONFIG_HOME", f.home.join("config"));
    std::env::set_var("XDG_DATA_HOME", f.home.join("data"));
    std::env::remove_var("CLAUDE_SESSION_ID");
    guard
}

fn members(f: &Fixture, channel: &str) -> Vec<String> {
    attend_groups::load_groups(&f.signals()).get(channel).map(|e| e.members.clone()).unwrap_or_default()
}

/// `/clear` gives a running session a new id. A frontend that stays up across
/// it must act as the new id on its next call, and use that one id for every
/// part of the call: the enrollment and the membership both.
#[test]
fn a_join_after_an_id_change_enrolls_and_joins_as_the_new_id() {
    let f = Fixture::new("inproc-idchange");
    let _env = enter(&f);
    let old = f.sid.clone();

    attend::verbs::channels::join("before", false).expect("join as the first id");
    assert!(f.marker(&old).exists(), "the first join enrolls the first id");
    assert_eq!(members(&f, "before"), vec![old.clone()]);

    let new = "sess-inproc-after-clear";
    f.set_session_id(new);
    attend::verbs::channels::join("after", false).expect("join as the new id");
    assert!(f.marker(new).exists(), "the second join enrolls the new id");
    assert_eq!(members(&f, "after"), vec![new.to_string()], "and writes its membership under it");
}

fn reply(text: &str) -> Result<attend::verbs::Replied, attend::verbs::SendError> {
    attend::verbs::reply(&attend::verbs::SendRequest { message: text.to_string(), ..Default::default() })
}

/// `reply` threads under the newest message a conduit delivered, reports
/// how it threaded, and refuses when nothing has been delivered.
#[test]
fn reply_reports_how_it_threaded() {
    let f = Fixture::new("inproc-reply");
    let _env = enter(&f);

    assert!(matches!(reply("too early"), Err(attend::verbs::SendError::NoInbound)));

    sensor_peers::last_inbound::record(&f.sid, "other-session-1-1", &OPEN);
    let threaded = reply("an answer").expect("a threaded reply");
    assert_eq!(threaded.threading, attend::verbs::Threading::Threaded);
    assert_eq!(threaded.sent.re.as_deref(), Some("other-session-1-1"));

    // An id from an external sender is not a signal id: the reply still
    // goes out, without a thread, and says so.
    sensor_peers::last_inbound::record(&f.sid, "someone@kitty-1", &OPEN);
    let unthreaded = reply("still delivered").expect("an unthreaded reply");
    assert_eq!(unthreaded.threading, attend::verbs::Threading::Unthreaded);
    assert_eq!(unthreaded.sent.re, None);
    assert_eq!(unthreaded.sent.failed.len(), 0);
}

/// A reply to a message said in a channel goes to that channel, not to
/// `#open`; one to a message another session sent this project goes back to
/// that session's project; a destination the caller names wins over both.
#[test]
fn a_reply_goes_where_its_parent_was_said() {
    let f = Fixture::new("inproc-reply-route");
    let _env = enter(&f);
    attend::verbs::channels::join("mod-trial", false).expect("join");
    attend_groups::Groups::new(&f.signals(), "peer-sess").join("mod-trial", false).unwrap();
    attend_presence::heartbeat::touch("peer-sess").unwrap();

    f.put("@mod-trial", "peer-sess-1-1", "in the channel", std::time::Duration::ZERO);
    sensor_peers::last_inbound::record(&f.sid, "peer-sess-1-1", &OPEN);
    let sent = reply("answered in the channel").expect("a reply into the channel").sent;
    assert_eq!(sent.scope, "channel");
    assert!(f.signals().join("@mod-trial").join(format!("{}.signal", sent.id)).is_file());
    assert!(!f.signals().join("_broadcast").join(format!("{}.signal", sent.id)).exists());

    // `put` writes as a session working in /tmp/elsewhere, where none is
    // live, so the directed reply is refused naming that path.
    f.put(&f.project_tray(), "peer-sess-1-2", "to your project", std::time::Duration::ZERO);
    sensor_peers::last_inbound::record(&f.sid, "peer-sess-1-2", &OPEN);
    match reply("answered in its project") {
        Err(attend::verbs::SendError::NoPeerAt { path, .. }) => assert_eq!(path, "/tmp/elsewhere"),
        other => panic!("expected a reply directed at the sender's project: {other:?}"),
    }

    sensor_peers::last_inbound::record(&f.sid, "peer-sess-1-1", &OPEN);
    let req = attend::verbs::SendRequest {
        message: "elsewhere".to_string(),
        to: Some("/nowhere/live".to_string()),
        ..Default::default()
    };
    match attend::verbs::reply(&req) {
        Err(attend::verbs::SendError::NoPeerAt { path, .. }) => assert_eq!(path, "/nowhere/live"),
        other => panic!("the caller's --to wins over the parent's channel: {other:?}"),
    }
}
