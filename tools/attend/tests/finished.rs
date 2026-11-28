// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! A session marked finished (`attend listen finish`): peers see it
//! finished, and new messages to it are refused, until it listens again.
//! Messages queued before the mark stay queued.
//!
//! Live sessions are found by the peers sensor's process scan, so every test
//! here needs it. The verbs read HOME and the XDG roots from the
//! environment, which is process-wide, so each test holds [`LOCK`] while it
//! points them at its own fixture.
//!
//! References:
//! - ADR-418 item 12: a stopped session is marked finished.
#![cfg(feature = "sensor-peers")]

mod common;
use attend::verbs::{SendError, SendRequest};
use attend_presence::enrollment::{self, Source};
use common::{Fixture, Peer};
use std::path::Path;
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

fn send(to: Option<&str>, channel: Option<&str>, text: &str) -> Result<attend::verbs::Sent, SendError> {
    attend::verbs::send(&SendRequest {
        message: text.to_string(),
        to: to.map(str::to_string),
        channel: channel.map(str::to_string),
        re: None,
    })
}

fn files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> =
        std::fs::read_dir(dir).map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    names.sort();
    names
}

/// The `to` of signal `id` in `dir`.
fn to_of(dir: &Path, id: &str) -> Option<String> {
    let raw = std::fs::read_to_string(dir.join(format!("{id}.signal"))).unwrap();
    agent_identity::parse_signal(agent_identity::wire_line(&raw)).and_then(|s| s.to.map(str::to_string))
}

fn nickname(cwd: &str) -> String {
    agent_identity::Identity::for_cwd(cwd, agent_theme::ColorDepth::NoColor).nickname.to_string()
}

/// `attend listen finish` marks the session finished and keeps it
/// listening; a beat leaves the mark, and `attend listen on` clears it.
#[test]
fn listen_finish_sets_the_mark_and_listen_on_clears_it() {
    let f = Fixture::new("finished-cli");
    f.ok(&["listen", "on"]);
    assert_eq!(f.ok(&["listen", "finish"]), "listening, finished\n");
    assert!(std::fs::read_to_string(f.marker(&f.sid)).unwrap().lines().any(|l| l.starts_with("finished ")));
    let state: serde_json::Value = serde_json::from_str(&f.ok(&["listen", "status", "--json"])).unwrap();
    assert!(state["finished"].is_u64(), "{state}");
    assert_eq!(f.ok(&["listen", "beat"]), "listening, finished\n", "a beat leaves the mark");
    assert_eq!(f.ok(&["listen", "on"]), "listening\n");
    let state: serde_json::Value = serde_json::from_str(&f.ok(&["listen", "status", "--json"])).unwrap();
    assert_eq!(state["finished"], serde_json::Value::Null);
}

/// A direct send to a finished session, by name or by id, is refused with
/// the finished wording and writes nothing. What was queued before the mark
/// stays queued, and once the session listens again the send goes through.
#[test]
fn a_direct_send_to_a_finished_session_is_refused_until_it_listens_again() {
    let f = Fixture::new("finished-direct");
    let _env = enter(&f);
    let peer = Peer::start(&f, "peer-sess", "peerproj");
    let name = nickname(&peer.cwd);
    let queued = send(Some(&name), None, "before the mark").expect("sent while live");

    enrollment::finish("peer-sess").unwrap();
    for to in [name.as_str(), "peer-sess"] {
        match send(Some(to), None, "after the mark") {
            Err(e @ SendError::RecipientFinished { .. }) => {
                let why = e.to_string();
                assert!(why.starts_with(&name) && why.contains("(session peer-sess) was marked finished "), "{to}: {why}");
                assert!(why.ends_with(" ago; nothing was sent"), "{to}: {why}");
                assert!(!why.contains("wake"), "{to}: {why}");
            }
            other => panic!("{to}: expected recipient_finished: {other:?}"),
        }
    }
    assert_eq!(files(&peer.tray(&f)), [format!("{}.signal", queued.id)], "the queued message stays, and nothing new lands");

    enrollment::enroll("peer-sess", Source::Listen, None).unwrap();
    let sent = send(Some(&name), None, "welcome back").expect("listening again clears the mark");
    assert_eq!(to_of(&peer.tray(&f), &sent.id).as_deref(), Some("peer-sess"));
}

/// A project send skips the finished sessions there, and is refused only
/// when every session there has finished.
#[test]
fn a_project_send_skips_finished_sessions() {
    let f = Fixture::new("finished-project");
    let _env = enter(&f);
    let a = Peer::start(&f, "peer-a", "shared");
    let _b = Peer::start(&f, "peer-b", "shared");
    enrollment::finish("peer-a").unwrap();
    let sent = send(Some(&a.cwd), None, "whoever is here").expect("peer-b is live");
    assert_eq!(to_of(&a.tray(&f), &sent.id).as_deref(), Some("peer-b"));

    enrollment::finish("peer-b").unwrap();
    match send(Some(&a.cwd), None, "anyone?") {
        Err(SendError::RecipientFinished { sessions }) => {
            let mut ids: Vec<&str> = sessions.iter().map(|s| s.session.as_str()).collect();
            ids.sort();
            assert_eq!(ids, ["peer-a", "peer-b"]);
        }
        other => panic!("expected recipient_finished: {other:?}"),
    }
}

/// A channel post goes while one live member other than the sender has not
/// finished, and is refused once every such member has.
#[test]
fn a_channel_post_needs_a_member_that_has_not_finished() {
    let f = Fixture::new("finished-channel");
    let _env = enter(&f);
    let _a = Peer::start(&f, "peer-a", "pa");
    let _b = Peer::start(&f, "peer-b", "pb");
    attend::verbs::channels::join("deploy", false).expect("join");
    for sid in ["peer-a", "peer-b"] {
        attend_groups::Groups::new(&f.signals(), sid).join("deploy", false).unwrap();
    }
    enrollment::finish("peer-a").unwrap();
    send(None, Some("deploy"), "still someone").expect("peer-b has not finished");

    enrollment::finish("peer-b").unwrap();
    match send(None, Some("deploy"), "nobody") {
        Err(SendError::RecipientFinished { sessions }) => assert_eq!(sessions.len(), 2, "{sessions:?}"),
        other => panic!("expected recipient_finished: {other:?}"),
    }
}

/// A reply to a finished session's message is refused with the finished
/// wording, as a reply to an exited one is with its own.
#[test]
fn a_reply_to_a_finished_session_is_refused() {
    let f = Fixture::new("finished-reply");
    let _env = enter(&f);
    let peer = Peer::start(&f, "peer-sess", "peerproj");
    let dir = f.signals().join("_broadcast");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("peer-sess-1-1.signal"), format!("claude:peer-sess|peerproj|{}|to:*|who knows?\n", peer.cwd)).unwrap();
    sensor_peers::last_inbound::record(&f.sid, "peer-sess-1-1", &sensor_peers::last_inbound::Said::Open, "claude:peer-sess");
    enrollment::finish("peer-sess").unwrap();
    match attend::verbs::reply(&SendRequest { message: "too late".to_string(), ..Default::default() }) {
        Err(e @ SendError::RecipientFinished { .. }) => {
            assert!(e.to_string().contains(&nickname(&peer.cwd)), "{e}");
        }
        other => panic!("expected recipient_finished: {other:?}"),
    }
    assert!(files(&peer.tray(&f)).is_empty(), "nothing reached the tray");
}

/// The peer list shows a finished session as finished, with its age, in
/// its status and in the JSON's `finished` field.
#[test]
fn the_peer_list_shows_a_finished_session() {
    let f = Fixture::new("finished-peers");
    let _env = enter(&f);
    let _peer = Peer::start(&f, "peer-sess", "peerproj");
    let peer = |p: &attend::verbs::peers::Peers| p.peers.iter().find(|a| a.session_id.as_deref() == Some("peer-sess")).cloned().unwrap();
    assert_eq!(peer(&attend::verbs::peers::peers()).finished, None);

    enrollment::finish("peer-sess").unwrap();
    let listed = peer(&attend::verbs::peers::peers());
    assert!(listed.status.starts_with("finished ") && listed.status.ends_with(" ago"), "{}", listed.status);
    assert!(listed.finished.is_some());
    let json = serde_json::to_value(&listed).unwrap();
    assert!(json["finished"].is_u64(), "{json}");
}
