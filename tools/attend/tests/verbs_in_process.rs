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

/// `reply` is compiled only with the peers sensor, which records what it
/// threads under.
#[cfg(feature = "sensor-peers")]
mod reply {
    use super::*;
    use sensor_peers::last_inbound::{record, Said};

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

        record(&f.sid, "other-session-1-1", &Said::Open, "external:someone@kitty");
        let threaded = reply("an answer").expect("a threaded reply");
        assert_eq!(threaded.threading, attend::verbs::Threading::Threaded);
        assert_eq!(threaded.sent.re.as_deref(), Some("other-session-1-1"));

        // An id from an external sender is not a signal id: the reply still
        // goes out, without a thread, and says so.
        record(&f.sid, "someone@kitty-1", &Said::Open, "external:someone@kitty");
        let unthreaded = reply("still delivered").expect("an unthreaded reply");
        assert_eq!(unthreaded.threading, attend::verbs::Threading::Unthreaded);
        assert_eq!(unthreaded.sent.re, None);
        assert_eq!(unthreaded.sent.failed.len(), 0);
    }

    // Where a reply goes when the caller names no destination: to its parent's
    // sender, or in its parent's channel. One test per route.

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

    /// A last-inbound record as written before the room was kept: the id alone.
    fn old_record(f: &Fixture, id: &str) {
        let path = sensor_peers::last_inbound::path(&f.sid);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, id).unwrap();
    }

    /// A reply refused because nothing says where its parent was said, with
    /// nothing written to `#open`.
    fn assert_room_unknown(f: &Fixture, result: Result<attend::verbs::Replied, attend::verbs::SendError>) {
        match result {
            Err(e @ attend::verbs::SendError::ParentRoomUnknown) => {
                let why = e.to_string();
                assert!(why.contains("--channel") && why.contains("--to") && why.contains("attend send"), "{why}");
            }
            other => panic!("expected parent_room_unknown: {other:?}"),
        }
        assert!(open_tray(f).is_empty(), "nothing reached #open");
    }

    /// This session and a live peer, `peer-sess`, both in `mod-trial`.
    fn shared_channel(f: &Fixture) {
        attend::verbs::channels::join("mod-trial", false).expect("join");
        attend_groups::Groups::new(&f.signals(), "peer-sess").join("mod-trial", false).unwrap();
        attend_presence::heartbeat::touch("peer-sess").unwrap();
    }

    /// The operator's case of 2026-10-12: a message from attend-chat sent to
    /// this project, answered with attend_reply, went to `#open`. A reply to a
    /// person is a direct message in their person tray, `to` their member id.
    #[test]
    fn a_reply_to_an_external_directed_parent_goes_to_the_person() {
        let f = Fixture::new("route-external");
        let _env = enter(&f);
        signal(&f, &f.project_tray(), "aaron-kitty-1-1", "external:aaron@kitty|aaron|/home/aaron|@Chaucer-beta can you look?");
        record(&f.sid, "aaron-kitty-1-1", &Said::Project(None), "external:aaron@kitty");
        let sent = reply("looking now").expect("a direct reply").sent;
        assert_eq!((sent.scope, sent.to.as_str()), ("directed", "aaron"));
        let raw = std::fs::read_to_string(attend_groups::person_dir(&f.signals(), "aaron").join(format!("{}.signal", sent.id)))
            .expect("the reply is in the person tray");
        let sig = agent_identity::parse_signal(agent_identity::wire_line(&raw)).unwrap();
        assert_eq!((sig.to, sig.reply_to, sig.message), (Some("aaron"), Some("aaron-kitty-1-1"), "looking now"));
        assert!(open_tray(&f).is_empty(), "nothing reached #open");
    }

    /// A reply to a message said in a channel goes to that channel, not to
    /// `#open`.
    #[test]
    fn a_reply_to_a_channel_parent_stays_in_the_channel() {
        let f = Fixture::new("route-channel");
        let _env = enter(&f);
        shared_channel(&f);
        f.put("@mod-trial", "peer-sess-1-1", "in the channel", std::time::Duration::ZERO);
        record(&f.sid, "peer-sess-1-1", &channel("mod-trial"), "claude:peer-sess");
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
        old_record(&f, "peer-sess-1-1");
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
        record(&f.sid, "peer-sess-1-1", &channel("mod-trial"), "claude:peer-sess");
        attend_groups::Groups::new(&f.signals(), "peer-sess").dissolve("mod-trial").unwrap();
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
        record(&f.sid, "peer-sess-1-9", &channel("mod-trial"), "claude:peer-sess");
        let sent = reply("answered in the channel").expect("a reply into the channel").sent;
        assert_eq!(sent.scope, "channel");
        assert_eq!(sent.re.as_deref(), Some("peer-sess-1-9"));

        record(&f.sid, "someone@kitty-1", &channel("mod-trial"), "external:someone@kitty");
        let unthreaded = reply("still in the channel").expect("an unthreaded reply");
        assert_eq!(unthreaded.threading, attend::verbs::Threading::Unthreaded);
        assert_eq!(unthreaded.sent.scope, "channel");
        assert!(open_tray(&f).is_empty());
    }

    /// An old record names no room, and once the parent is reaped and the
    /// session has left its channel, nothing else does either. The reply is
    /// refused rather than sent to `#open`.
    #[test]
    fn a_reply_to_a_reaped_parent_with_no_recorded_room_is_refused() {
        let f = Fixture::new("route-unknown-reaped");
        let _env = enter(&f);
        shared_channel(&f);
        old_record(&f, "peer-sess-1-9");
        attend::verbs::channels::leave("mod-trial");
        assert_room_unknown(&f, reply("a private answer"));
    }

    /// An old record whose id cannot be threaded cannot be looked up either.
    #[test]
    fn a_reply_to_an_unthreadable_id_with_no_recorded_room_is_refused() {
        let f = Fixture::new("route-unknown-unthreadable");
        let _env = enter(&f);
        old_record(&f, "someone@kitty-1");
        assert_room_unknown(&f, reply("an answer"));
    }

    /// A record in this project's tray that does not parse names no sender, so
    /// no place to answer it.
    #[test]
    fn a_reply_to_an_unparseable_directed_parent_is_refused() {
        let f = Fixture::new("route-unknown-unparseable");
        let _env = enter(&f);
        signal(&f, &f.project_tray(), "peer-sess-1-1", "no fields here");
        old_record(&f, "peer-sess-1-1");
        assert_room_unknown(&f, reply("an answer"));
    }

    /// A Claude sender with an empty cwd names no project to answer in. Its
    /// room is not recorded, so once the parent is reaped the reply is refused;
    /// it is never mistaken for an external sender's and sent to `#open`. While
    /// the parent's file stands, its sender is known and has exited, and the
    /// reply is refused naming it.
    #[test]
    fn a_reply_to_a_directed_parent_with_an_empty_sender_cwd_is_refused() {
        let f = Fixture::new("route-unknown-emptycwd");
        let _env = enter(&f);
        record(&f.sid, "peer-sess-1-1", &Said::Project(Some(String::new())), "claude:peer-sess");
        assert_room_unknown(&f, reply("an answer"));

        signal(&f, &f.project_tray(), "peer-sess-1-1", "claude:peer-sess|peerproj||to your project");
        match reply("an answer") {
            Err(attend::verbs::SendError::SenderExited { session, .. }) => assert_eq!(session, "peer-sess"),
            other => panic!("expected sender_exited: {other:?}"),
        }
        assert!(open_tray(&f).is_empty(), "nothing reached #open");
    }

    /// A message another Claude session sent this project with `--to` is
    /// answered with a direct message to that session, while it is live.
    #[test]
    fn a_reply_to_a_directed_parent_goes_to_the_live_sender() {
        let f = Fixture::new("route-directed");
        let _env = enter(&f);
        let peer = common::Peer::start(&f, "peer-sess", "peerproj");
        let peer_cwd = peer.cwd.clone();
        signal(&f, &f.project_tray(), "peer-sess-1-1", &format!("claude:peer-sess|peerproj|{peer_cwd}|to your project"));
        record(&f.sid, "peer-sess-1-1", &Said::Project(Some(peer_cwd.clone())), "claude:peer-sess");
        let result = reply("answered in your project");
        drop(peer);
        let sent = result.expect("a directed reply").sent;
        assert_eq!((sent.scope, sent.to.as_str()), ("directed", "peer-sess"));
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
        record(&f.sid, "peer-sess-1-1", &channel("mod-trial"), "claude:peer-sess");
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
}
