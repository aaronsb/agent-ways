//! The message verbs called as a library, in this test process, the way a
//! long-lived frontend (an MCP server, a mod) calls them (ADR-501 item 4).
//!
//! The verbs read HOME and the XDG roots from the environment, which is
//! process-wide, so each test holds [`LOCK`] while it points them at its
//! own fixture.

mod common;
use common::Fixture;
use sensor_peers::last_inbound::{record, Said};
use std::sync::{Mutex, MutexGuard};

static LOCK: Mutex<()> = Mutex::new(());

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

    record(&f.sid, "other-session-1-1", &Said::Open);
    let threaded = reply("an answer").expect("a threaded reply");
    assert_eq!(threaded.threading, attend::verbs::Threading::Threaded);
    assert_eq!(threaded.sent.re.as_deref(), Some("other-session-1-1"));

    // An id from an external sender is not a signal id: the reply still
    // goes out, without a thread, and says so.
    record(&f.sid, "someone@kitty-1", &Said::Open);
    let unthreaded = reply("still delivered").expect("an unthreaded reply");
    assert_eq!(unthreaded.threading, attend::verbs::Threading::Unthreaded);
    assert_eq!(unthreaded.sent.re, None);
    assert_eq!(unthreaded.sent.failed.len(), 0);
}

// Where a reply goes when the caller names no destination: where its parent
// was said. One test per route.

fn channel(name: &str) -> Said {
    Said::Channel(name.to_string())
}

/// Write `line` as signal `id` into `tray`.
fn signal(f: &Fixture, tray: &str, id: &str, line: &str) {
    let dir = f.signals().join(tray);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{id}.signal")), format!("{line}\n")).unwrap();
}

/// The signals in `#open`: a refused reply must leave it empty.
fn open_tray(f: &Fixture) -> Vec<String> {
    std::fs::read_dir(f.signals().join("_broadcast"))
        .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default()
}

/// This session and a live peer, `peer-sess`, both in `mod-trial`.
fn shared_channel(f: &Fixture) {
    attend::verbs::channels::join("mod-trial", false).expect("join");
    attend_groups::Groups::new(&f.signals(), "peer-sess").join("mod-trial", false).unwrap();
    attend_presence::heartbeat::touch("peer-sess").unwrap();
}

#[test]
fn a_reply_to_an_open_parent_goes_to_open() {
    let f = Fixture::new("route-open");
    let _env = enter(&f);
    f.put("_broadcast", "peer-sess-1-1", "said to everyone", std::time::Duration::ZERO);
    record(&f.sid, "peer-sess-1-1", &Said::Open);
    let sent = reply("answered to everyone").expect("a reply in #open").sent;
    assert_eq!(sent.scope, "#open");
    assert!(open_tray(&f).contains(&format!("{}.signal", sent.id)));
}

/// A directed message from outside Claude Code came from a shell no session
/// listens at, so its answer goes where people read: `#open`.
#[test]
fn a_reply_to_an_external_directed_parent_goes_to_open() {
    let f = Fixture::new("route-external");
    let _env = enter(&f);
    signal(&f, &f.project_tray(), "me-kitty-1-1", "external:me@kitty|me|/home/me|to your project");
    record(&f.sid, "me-kitty-1-1", &Said::Project(None));
    let sent = reply("answered").expect("a reply in #open").sent;
    assert_eq!(sent.scope, "#open");
}

/// A reply to a message said in a channel goes to that channel, not to
/// `#open`.
#[test]
fn a_reply_to_a_channel_parent_stays_in_the_channel() {
    let f = Fixture::new("route-channel");
    let _env = enter(&f);
    shared_channel(&f);
    f.put("@mod-trial", "peer-sess-1-1", "in the channel", std::time::Duration::ZERO);
    record(&f.sid, "peer-sess-1-1", &channel("mod-trial"));
    let sent = reply("answered in the channel").expect("a reply into the channel").sent;
    assert_eq!(sent.scope, "channel");
    assert!(f.signals().join("@mod-trial").join(format!("{}.signal", sent.id)).is_file());
    assert!(open_tray(&f).is_empty());
}

/// After this session leaves the channel its parent was said in, the reply
/// is refused: anywhere else would carry the channel's answer out of it.
#[test]
fn a_reply_to_a_parent_in_a_channel_left_is_refused() {
    let f = Fixture::new("route-left");
    let _env = enter(&f);
    shared_channel(&f);
    f.put("@mod-trial", "peer-sess-1-1", "in the channel", std::time::Duration::ZERO);
    // An old-format record, with no room: the parent's file alone says
    // where it was said.
    std::fs::create_dir_all(sensor_peers::last_inbound::path(&f.sid).parent().unwrap()).unwrap();
    std::fs::write(sensor_peers::last_inbound::path(&f.sid), "peer-sess-1-1").unwrap();
    attend::verbs::channels::leave("mod-trial");
    match reply("a private answer") {
        Err(e @ attend::verbs::SendError::ParentChannelLeft { .. }) => {
            assert!(matches!(&e, attend::verbs::SendError::ParentChannelLeft { channel } if channel == "mod-trial"));
            let why = e.to_string();
            assert!(why.contains("attend join mod-trial") && why.contains("--channel") && why.contains("send"), "{why}");
        }
        other => panic!("expected parent_channel_left: {other:?}"),
    }
    assert!(open_tray(&f).is_empty(), "nothing reached #open");
}

/// A channel dissolved since its message arrived takes the parent's file
/// with it; the recorded room says where it was, and the reply is refused.
#[test]
fn a_reply_to_a_parent_in_a_dissolved_channel_is_refused() {
    let f = Fixture::new("route-gone");
    let _env = enter(&f);
    shared_channel(&f);
    f.put("@mod-trial", "peer-sess-1-1", "in the channel", std::time::Duration::ZERO);
    record(&f.sid, "peer-sess-1-1", &channel("mod-trial"));
    attend_groups::Groups::new(&f.signals(), "peer-sess").dissolve("mod-trial");
    match reply("a private answer") {
        Err(attend::verbs::SendError::ParentChannelGone { channel }) => assert_eq!(channel, "mod-trial"),
        other => panic!("expected parent_channel_gone: {other:?}"),
    }
    assert!(open_tray(&f).is_empty(), "nothing reached #open");
}

/// A parent reaped from a channel that still stands is answered in that
/// channel, by the room recorded beside its id; so is a parent whose id
/// cannot be threaded.
#[test]
fn a_reply_to_a_reaped_parent_goes_to_its_recorded_room() {
    let f = Fixture::new("route-reaped");
    let _env = enter(&f);
    shared_channel(&f);
    record(&f.sid, "peer-sess-1-9", &channel("mod-trial"));
    let sent = reply("answered in the channel").expect("a reply into the channel").sent;
    assert_eq!(sent.scope, "channel");
    assert_eq!(sent.re.as_deref(), Some("peer-sess-1-9"));

    record(&f.sid, "someone@kitty-1", &channel("mod-trial"));
    let unthreaded = reply("still in the channel").expect("an unthreaded reply");
    assert_eq!(unthreaded.threading, attend::verbs::Threading::Unthreaded);
    assert_eq!(unthreaded.sent.scope, "channel");
    assert!(open_tray(&f).is_empty());
}

/// A message another Claude session sent this project with `--to` is
/// answered in that session's project, while it is live.
#[test]
fn a_reply_to_a_directed_parent_goes_to_the_live_sender() {
    let f = Fixture::new("route-directed");
    let _env = enter(&f);
    let peer_cwd = f.home.join("peerproj");
    std::fs::create_dir_all(&peer_cwd).unwrap();
    let peer_cwd = peer_cwd.to_string_lossy().into_owned();
    // A live process whose argv names claude, recorded as the peer session.
    let mut peer = std::process::Command::new("sh").args(["-c", "sleep 30; :", "claude"]).spawn().unwrap();
    std::fs::write(
        f.home.join(".claude").join("sessions").join(format!("{}.json", peer.id())),
        format!(r#"{{"pid":{},"sessionId":"peer-sess","cwd":"{peer_cwd}"}}"#, peer.id()),
    )
    .unwrap();
    signal(&f, &f.project_tray(), "peer-sess-1-1", &format!("claude:peer-sess|peerproj|{peer_cwd}|to your project"));
    record(&f.sid, "peer-sess-1-1", &Said::Project(Some(peer_cwd.clone())));
    let result = reply("answered in your project");
    peer.kill().ok();
    peer.wait().ok();
    let sent = result.expect("a directed reply").sent;
    assert_eq!(sent.scope, "directed");
    let tray = f.signals().join(claude_sessions::attend_key(&peer_cwd));
    assert!(tray.join(format!("{}.signal", sent.id)).is_file());
    assert!(open_tray(&f).is_empty());
}

/// A destination the caller names wins over the parent's room.
#[test]
fn the_callers_destination_wins_over_the_parents_room() {
    let f = Fixture::new("route-override");
    let _env = enter(&f);
    shared_channel(&f);
    f.put("@mod-trial", "peer-sess-1-1", "in the channel", std::time::Duration::ZERO);
    record(&f.sid, "peer-sess-1-1", &channel("mod-trial"));
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

