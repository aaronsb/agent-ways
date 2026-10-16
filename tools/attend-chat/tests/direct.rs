//! What reaches the operator directly, and what the operator sends directly
//! (ADR-401 `to`): a reply or a direct message to the person lands in their
//! person tray, `_people/<member-id>/`, which the chat watches and shows as
//! direct in the operator view and its sender's DM view; an `@Name` send
//! writes a direct message whose `to` is that session's id. In its own file,
//! so its own fixture.

mod support;

use std::path::Path;
use std::time::{Duration, Instant};

use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use attend_chat::app::Chat;
use attend_chat::signal::{self, Channel, Signal};
use attend_chat::tabs::Tab;
use attend_chat::watcher;

const PEER_SID: &str = "bbbbbbbb-0000-4000-8000-000000000002";
const PEER_CWD: &str = "/work/peer";

fn base() -> &'static Path {
    support::fixture("direct", |_| {
        support::pin_alive(PEER_SID);
        let sessions = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join(".claude").join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(sessions.join("4242.json"), format!(r#"{{"pid":4242,"sessionId":"{PEER_SID}","cwd":"{PEER_CWD}"}}"#)).unwrap();
    });
    // The fixture root holds the signals base under its cache.
    Box::leak(signal::signals_base().into_boxed_path())
}

/// The person tray's signals reach the chat through the watcher, as
/// `Channel::Person`; another person's tray is read too, as a direct
/// message between others, which no view of the person's shows (#995).
#[test]
fn the_person_tray_streams_into_the_chat() {
    let base = base();
    let (tx, rx) = std::sync::mpsc::channel::<attend_chat::watcher::Delivery>();
    watcher::spawn_watcher(base.to_path_buf(), String::new(), "aaron".to_string(), tx).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    for (member, body) in [("someone-else", "not yours"), ("aaron", "a reply for you")] {
        let tray = attend_groups::person_dir(base, member);
        std::fs::create_dir_all(&tray).unwrap();
        let line = agent_identity::signal::format_tagged(
            &format!("claude:{PEER_SID}"),
            "peer",
            PEER_CWD,
            &agent_identity::Envelope { to: Some(member), ..Default::default() },
            None,
            body,
        );
        agent_identity::signal::write_signal_file(&tray, &format!("{member}-1-0.signal"), &line).unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut got = Vec::new();
    while Instant::now() < deadline && got.is_empty() {
        got.extend(rx.try_iter().filter_map(watcher::Delivery::signal));
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(300));
    got.extend(rx.try_iter().filter_map(watcher::Delivery::signal));
    // Other tests in this binary post to #open and the project tray at the
    // same time; only person-tray signals are this test's.
    let mut bodies: Vec<(&str, &Channel)> =
        got.iter().filter(|s| s.message.contains("for you") || s.message.contains("not yours")).map(|s| (s.message.as_str(), &s.channel)).collect();
    bodies.sort_by_key(|(m, _)| *m);
    assert_eq!(bodies, [("a reply for you", &Channel::Person), ("not yours", &Channel::Elsewhere("someone-else".into()))]);
    assert!(!attend_chat::tabs::shows(got.iter().find(|s| s.message == "not yours").unwrap(), &Tab::Merged));
}

/// A direct message to the person shows in the operator view and in its
/// sender's DM view, labelled `→ you`. No channel tab shows it: ADR-405's
/// second amendment of 2026-10-15 leaves DMs to the operator view and the
/// DM views of 2026-10-13.
#[test]
fn a_direct_message_shows_in_merged_and_the_senders_view_only() {
    base();
    let mut c = Chat::new(None, Palette::default(), Shape::PLAIN).heartbeat(false);
    c.push(Signal {
        id: "dm-1".into(),
        from: format!("claude:{PEER_SID}"),
        project: "peer".into(),
        cwd: PEER_CWD.into(),
        reply_to: None,
        message: "a reply for you".into(),
        ts: 1_790_949_000,
        channel: Channel::Person,
        tray: String::new(),
        beside_post: false,
    });
    for tab in [Tab::Merged, Tab::Dm(PEER_SID.into())] {
        assert!(attend_chat::tabs::shows(&c.signals()[0], &tab), "{tab:?}");
    }
    for tab in [Tab::Channel("open".into()), Tab::Channel("deploy".into())] {
        assert!(!attend_chat::tabs::shows(&c.signals()[0], &tab), "{tab:?}");
    }
    let rows = testkit::rows(&testkit::render_screen(&mut c, 80, 25));
    let header = rows.iter().find(|r| r.contains("→ you")).unwrap_or_else(|| panic!("{rows:#?}"));
    assert!(rows.iter().any(|r| r.contains("a reply for you")), "{rows:#?}");
    assert!(!header.contains("a reply for you"), "the label is on the header: {header}");
}

/// The operator's report of 2026-10-15: a channel made after a DM reached
/// them showed that DM. A chat that backfilled an earlier DM from the
/// person tray, through the watcher, shows a new channel empty.
#[test]
fn a_new_channel_shows_no_earlier_direct_message() {
    let base = base();
    let tray = attend_groups::person_dir(base, &signal::human_member_id());
    std::fs::create_dir_all(&tray).unwrap();
    let line = agent_identity::signal::format_tagged(
        &format!("claude:{PEER_SID}"),
        "peer",
        PEER_CWD,
        &agent_identity::Envelope { to: Some(&signal::human_member_id()), ..Default::default() },
        None,
        "an older dm, from before the channel",
    );
    agent_identity::signal::write_signal_file(&tray, "older-dm-1-0.signal", &line).unwrap();
    let (tx, rx) = std::sync::mpsc::channel::<attend_chat::watcher::Delivery>();
    watcher::spawn_watcher(base.to_path_buf(), String::new(), signal::human_member_id(), tx).unwrap();
    let mut c = Chat::new(Some(rx), Palette::default(), Shape::PLAIN).sidebar(true).heartbeat(false).enhanced(true).dry_run(true);
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && !c.signals().iter().any(|s| s.message.contains("an older dm")) {
        c.drain();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(c.signals().iter().any(|s| s.message.contains("an older dm") && s.channel == Channel::Person), "{:#?}", c.signals());
    attend_groups::Groups::new(base, "").create("fresh", None).unwrap();
    c.refresh();
    let rows = testkit::rows(&testkit::render_screen(&mut c, 120, 35));
    let (x, y) = rows
        .iter()
        .enumerate()
        .skip(1)
        .find_map(|(y, r)| {
            let col = r.split('│').next()?;
            col.find("#fresh").map(|b| (col[..b].chars().count() as u16, y as u16))
        })
        .unwrap_or_else(|| panic!("no #fresh in the column: {rows:#?}"));
    c.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE });
    assert_eq!(c.foreground(), &Tab::Channel("fresh".into()));
    let rows = testkit::rows(&testkit::render_screen(&mut c, 120, 35));
    let feed: Vec<&str> = rows.iter().skip(1).map(|r| r.split_once('│').map_or(r.as_str(), |(_, rest)| rest)).collect();
    assert!(!feed.iter().any(|r| r.contains("→ you") || r.contains("an older dm")), "{rows:#?}");
}

/// `@Name` writes a direct message whose `to` is that session's id, in its
/// project's tray.
#[test]
fn an_at_name_send_is_a_direct_message_to_the_session() {
    base();
    let nick = agent_identity::Identity::for_cwd(PEER_CWD, agent_theme::ColorDepth::NoColor).nickname.to_string();
    let mut c = Chat::new(None, Palette::default(), Shape::PLAIN).heartbeat(false);
    testkit::render_screen(&mut c, 80, 25);
    let keys: Vec<KeyEvent> = format!("@{nick} are you there").chars().map(|ch| KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)).collect();
    testkit::drive(&mut c, &keys);
    testkit::drive(&mut c, &[KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)]);
    let tray = signal::cwd_dir(PEER_CWD);
    let file = std::fs::read_dir(&tray).unwrap().flatten().next().expect("a signal in the tray").path();
    let raw = std::fs::read_to_string(file).unwrap();
    let sig = agent_identity::parse_signal(agent_identity::wire_line(&raw)).unwrap();
    assert_eq!((sig.to, sig.from_kind, sig.from_id), (Some(PEER_SID), Some("human"), Some("aaron")));
    assert_eq!(sig.message, format!("@{nick} are you there"));
}

/// An attend-chat left open across an update reads the new wire wrongly
/// until it restarts, so it notices its own file changing.
#[test]
fn a_replaced_program_file_is_noticed() {
    let dir = std::env::temp_dir().join(format!("attend-chat-binary-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("attend-chat");
    std::fs::write(&path, "v1").unwrap();
    let at = std::fs::metadata(&path).unwrap().modified().unwrap();
    assert!(!attend_chat::app::updated_on_disk(&path, at));
    std::fs::write(dir.join("next"), "v2").unwrap();
    let f = std::fs::File::options().write(true).open(dir.join("next")).unwrap();
    f.set_modified(at + Duration::from_secs(5)).unwrap();
    std::fs::rename(dir.join("next"), &path).unwrap();
    assert!(attend_chat::app::updated_on_disk(&path, at));
    assert!(!attend_chat::app::updated_on_disk(&dir.join("gone"), at), "a missing file is no update");
    std::fs::remove_dir_all(&dir).ok();
}

/// `@Name` while a channel is shown posts in that channel, addressed to
/// the session (PR #974 review, O2): `#open` here, which every member reads.
/// The session's tray gets nothing.
#[test]
fn an_at_name_send_in_a_channel_view_stays_in_the_channel() {
    base();
    let nick = agent_identity::Identity::for_cwd(PEER_CWD, agent_theme::ColorDepth::NoColor).nickname.to_string();
    let tray = signal::cwd_dir(PEER_CWD);
    let mut c = Chat::new(None, Palette::default(), Shape::PLAIN).heartbeat(false);
    testkit::render_screen(&mut c, 80, 25);
    testkit::drive(&mut c, &[KeyEvent::new(KeyCode::Char('1'), KeyModifiers::ALT)]);
    assert_eq!(c.foreground(), &Tab::Channel("open".into()));
    let body = format!("@{nick} over to you");
    let keys: Vec<KeyEvent> = body.chars().map(|ch| KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)).collect();
    testkit::drive(&mut c, &keys);
    testkit::render_screen(&mut c, 80, 25);
    testkit::drive(&mut c, &[KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)]);
    let open = signal::broadcast_dir();
    let mine = std::fs::read_dir(&open)
        .unwrap()
        .flatten()
        .map(|e| std::fs::read_to_string(e.path()).unwrap())
        .find(|raw| raw.contains("over to you"))
        .expect("the post is in #open");
    let sig = agent_identity::parse_signal(agent_identity::wire_line(&mine)).unwrap();
    assert_eq!(sig.to, Some(PEER_SID));
    let in_tray = std::fs::read_dir(&tray).into_iter().flatten().flatten().any(|e| std::fs::read_to_string(e.path()).is_ok_and(|r| r.contains("over to you")));
    assert!(!in_tray, "nothing went to the session's tray");
}

/// `@Name` in a named channel's view, for an agent outside that channel, is
/// refused on the status row and the draft stays in the box: only members
/// read the channel (PR #974 re-check, E1). A member addressee posts.
#[test]
fn an_at_name_send_to_a_non_member_in_a_channel_view_is_refused() {
    let base = base();
    attend_groups::Groups::new(base, PEER_SID).join("crew", true).unwrap();
    attend_groups::Groups::new(base, "someone-else").create("lobby", None).unwrap();
    let nick = agent_identity::Identity::for_cwd(PEER_CWD, agent_theme::ColorDepth::NoColor).nickname.to_string();
    let send_in = |digit: char, channel: &str| {
        let mut c = Chat::new(None, Palette::default(), Shape::PLAIN).heartbeat(false);
        testkit::render_screen(&mut c, 80, 25);
        testkit::drive(&mut c, &[KeyEvent::new(KeyCode::Char(digit), KeyModifiers::ALT)]);
        assert_eq!(c.foreground(), &Tab::Channel(channel.into()));
        let body = format!("@{nick} in {channel}?");
        let keys: Vec<KeyEvent> = body.chars().map(|ch| KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)).collect();
        testkit::drive(&mut c, &keys);
        testkit::render_screen(&mut c, 80, 25);
        testkit::drive(&mut c, &[KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)]);
        let rows = testkit::rows(&testkit::render_screen(&mut c, 120, 30));
        (c, body, rows)
    };
    let (c, body, rows) = send_in('3', "lobby");
    assert!(rows.iter().any(|r| r.contains(&format!("@{nick} is not in #lobby; nothing sent"))), "{rows:#?}");
    assert_eq!(c.input().text(), body, "the draft stays in the box");
    assert_eq!(std::fs::read_dir(base.join("@lobby")).map(|d| d.count()).unwrap_or(0), 0, "nothing written");
    let (c, _, _) = send_in('2', "crew");
    assert!(c.input().is_empty(), "a member addressee posts");
    let raw = std::fs::read_dir(base.join("@crew")).unwrap().flatten().map(|e| std::fs::read_to_string(e.path()).unwrap()).find(|r| r.contains("in crew?")).expect("posted in #crew");
    assert_eq!(agent_identity::parse_signal(agent_identity::wire_line(&raw)).unwrap().to, Some(PEER_SID));
}
