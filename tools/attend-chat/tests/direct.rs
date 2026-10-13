//! What reaches the operator directly, and what the operator sends directly
//! (ADR-401 `to`): a reply or a direct message to the person lands in their
//! person tray, `_people/<member-id>/`, which the chat watches and shows as
//! direct in every view; an `@Name` send writes a direct message whose `to`
//! is that session's id. In its own file, so its own fixture.

mod support;

use std::path::Path;
use std::time::{Duration, Instant};

use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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
/// `Channel::Person`; another person's tray does not.
#[test]
fn the_person_tray_streams_into_the_chat() {
    let base = base();
    let (tx, rx) = std::sync::mpsc::channel::<Signal>();
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
        got.extend(rx.try_iter());
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(300));
    got.extend(rx.try_iter());
    let bodies: Vec<(&str, &Channel)> = got.iter().map(|s| (s.message.as_str(), &s.channel)).collect();
    assert_eq!(bodies, [("a reply for you", &Channel::Person)]);
}

/// A direct message to the person shows in every view, labelled `→ you`.
#[test]
fn a_direct_message_shows_in_every_view_labelled() {
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
    });
    for tab in [Tab::Merged, Tab::Channel("open".into())] {
        assert!(attend_chat::tabs::visible_in(&Channel::Person, &tab), "{tab:?}");
    }
    let rows = testkit::rows(&testkit::render_screen(&mut c, 80, 25));
    let header = rows.iter().find(|r| r.contains("→ you")).unwrap_or_else(|| panic!("{rows:#?}"));
    assert!(rows.iter().any(|r| r.contains("a reply for you")), "{rows:#?}");
    assert!(!header.contains("a reply for you"), "the label is on the header: {header}");
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
