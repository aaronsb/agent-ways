//! The person at attend-chat is a member of a channel they post in or
//! open, as of one they `/join`, so an agent alone in the channel with them
//! can answer them (ADR-404 D1). Observed on attend 0.21.0: the operator
//! posted in a channel from attend-chat, was never written as a member,
//! and the agent's `attend reply` was refused as "you are the only
//! listener". In its own file, so its own fixture.

mod support;

use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;
use attend_chat::tabs::Tab;

const AGENT_SID: &str = "aaaaaaaa-0000-4000-8000-000000000001";

fn chat() -> Chat {
    support::fixture("human-member", |base| {
        support::pin_alive(AGENT_SID);
        let agent = attend_groups::Groups::new(base, AGENT_SID);
        agent.join("room", false).unwrap();
        agent.join("hall", false).unwrap();
    });
    Chat::new(None, Palette::default(), Shape::PLAIN).enhanced(false)
}

fn members(name: &str) -> Vec<String> {
    attend_groups::Groups::new(&attend_chat::signal::signals_base(), "").members(name).unwrap_or_default()
}

fn press(c: &mut Chat, key: KeyEvent) {
    testkit::render_screen(c, 80, 25);
    assert!(testkit::drive(c, &[key]));
}

#[test]
fn posting_in_a_channel_makes_the_human_a_member() {
    let mut c = chat();
    for ch in "#room are you there".chars() {
        press(&mut c, KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    press(&mut c, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(c.status(), "sent → #room");
    assert!(members("room").contains(&"aaron".to_string()), "{:?}", members("room"));
}

#[test]
fn opening_a_channel_makes_the_human_a_member() {
    let mut c = chat();
    assert!(!members("hall").contains(&"aaron".to_string()));
    for digit in ['3', '4'] {
        press(&mut c, KeyEvent::new(KeyCode::Char(digit), KeyModifiers::ALT));
        if c.foreground() == &Tab::Channel("hall".into()) {
            break;
        }
    }
    assert_eq!(c.foreground(), &Tab::Channel("hall".into()));
    assert!(members("hall").contains(&"aaron".to_string()), "{:?}", members("hall"));
}

/// A chat that is not a live person (`--snap`, a dry run) opens a channel
/// without joining it.
#[test]
fn a_dry_run_opens_a_channel_without_joining_it() {
    let mut c = chat().dry_run(true);
    press(&mut c, KeyEvent::new(KeyCode::Char('2'), KeyModifiers::ALT));
    assert_eq!(c.foreground(), &Tab::Channel("open".into()));
    let base = attend_chat::signal::signals_base();
    let yaml = std::fs::read_to_string(base.join("_groups.yaml")).unwrap();
    assert!(!yaml.contains("open:"), "#open is never written as a channel: {yaml}");
}
