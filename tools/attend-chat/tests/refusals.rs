//! A send to a target Tab completes but nobody would read is refused in
//! its own words, through the real Enter: a quiet agent (no heartbeat),
//! and a channel whose directory was swept with no live member left. In
//! its own file, so its own fixture: nothing here may be written to the
//! screen suite's.

mod support;

use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;
use attend_chat::signal::{Channel, Signal};

const LIVE_SID: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const QUIET_SID: &str = "cccccccc-0000-4000-8000-000000000003";
const QUIET_CWD: &str = "/work/gamma";

fn sig(sid: &str, cwd: &str) -> Signal {
    Signal {
        id: format!("t-{sid}"),
        from: format!("claude:{sid}"),
        project: cwd.rsplit('/').next().unwrap_or("").into(),
        cwd: cwd.into(),
        reply_to: None,
        message: "a message".into(),
        ts: 1_790_949_000,
        channel: Channel::Open,
    }
}

fn chat() -> Chat {
    support::fixture("refusals", |base| {
        support::pin_alive(LIVE_SID);
        // `ghost` keeps its entry, with a quiet member, after its
        // directory was swept.
        attend_groups::Groups::new(base, QUIET_SID).join("ghost", false).unwrap();
        std::fs::remove_dir_all(base.join("@ghost")).unwrap();
    });
    let mut c = Chat::new(None, Palette::default(), Shape::PLAIN).heartbeat(false);
    c.push(sig(LIVE_SID, "/work/alpha"));
    c.push(sig(QUIET_SID, QUIET_CWD));
    c
}

fn send(c: &mut Chat, text: &str) -> String {
    let keys: Vec<KeyEvent> = text.chars().map(|ch| KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)).collect();
    assert!(testkit::drive(c, &keys));
    let flag = testkit::text(&testkit::render_screen(c, 80, 25));
    testkit::drive(c, &[KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)]);
    flag
}

#[test]
fn a_send_to_a_quiet_agent_says_it_may_not_be_read() {
    let mut c = chat();
    let quiet = agent_identity::Identity::for_cwd(QUIET_CWD, agent_theme::ColorDepth::Ansi16).nickname;
    let screen = send(&mut c, &format!("@{quiet} are you there"));
    assert!(screen.contains(&format!("@{quiet} (quiet)")), "the destination flag marks it: {screen}");
    assert_eq!(c.status(), format!("@{quiet} is quiet (no heartbeat) and may not read this; press Enter again to send"));
    assert_eq!(c.input().text(), format!("@{quiet} are you there"), "the draft is kept to send elsewhere");
}

#[test]
fn a_send_to_a_swept_channel_says_how_to_bring_it_back() {
    let mut c = chat();
    send(&mut c, "#ghost anyone");
    assert_eq!(c.status(), "#ghost was swept; /join #ghost and invite a live agent to bring it back, or send to #open");
    let base = attend_chat::signal::signals_base();
    assert!(!base.join("@ghost").exists(), "nothing was written, so the channel stays swept");
}
