//! The person at attend-chat is a member of a channel they post in, as of
//! one they `/join`, so an agent alone in the channel with them can answer
//! them (ADR-404 D1). This is the reproduction of the bug observed on
//! attend 0.21.0: the operator posted in a channel from attend-chat, was
//! never written as a member, and the agent's `attend message reply` was refused
//! as "you are the only listener". Showing a channel joins nothing: the tab
//! bar is walked and cycled past channels nobody chose, and a Leave holds.
//! In its own file, so its own fixture.

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
        for room in ["room", "hall", "yard", "porch"] {
            agent.join(room, false).unwrap();
        }
    });
    Chat::new(None, Palette::default(), Shape::PLAIN).enhanced(false)
}

fn is_member(name: &str) -> bool {
    attend_groups::Groups::new(&attend_chat::signal::signals_base(), "")
        .members(name)
        .unwrap_or_default()
        .iter()
        .any(|m| m == "aaron")
}

fn press(c: &mut Chat, key: KeyEvent) {
    testkit::render_screen(c, 80, 25);
    assert!(testkit::drive(c, &[key]));
}

fn type_line(c: &mut Chat, line: &str) {
    for ch in line.chars() {
        press(c, KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    press(c, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
}

/// Show every tab on the bar, by Alt+digit and by Tab on an empty line,
/// and return the channels shown.
fn walk_the_tabs(c: &mut Chat) -> Vec<String> {
    let mut shown = Vec::new();
    let keys = ('2'..='7').map(|d| KeyEvent::new(KeyCode::Char(d), KeyModifiers::ALT));
    let tabs = std::iter::repeat_n(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), 7);
    for key in keys.chain(tabs) {
        press(c, key);
        if let Tab::Channel(name) = c.foreground() {
            shown.push(name.clone());
        }
    }
    shown
}

#[test]
fn posting_in_a_channel_makes_the_human_a_member() {
    let mut c = chat();
    type_line(&mut c, "#room are you there");
    assert_eq!(c.status(), "sent → #room");
    assert!(is_member("room"));
}

#[test]
fn walking_the_tab_strip_joins_nothing() {
    let mut c = chat();
    let shown = walk_the_tabs(&mut c);
    assert!(["hall", "yard"].iter().all(|n| shown.iter().any(|s| s == n)), "the walk shows them: {shown:?}");
    assert!(!is_member("hall"), "shown, not joined");
    assert!(!is_member("yard"), "shown, not joined");
}

#[test]
fn after_a_leave_showing_the_channel_does_not_rejoin_it() {
    let mut c = chat();
    type_line(&mut c, "#porch hello");
    assert!(is_member("porch"));
    type_line(&mut c, "/leave porch");
    assert_eq!(c.status(), "left #porch");
    assert!(!is_member("porch"));
    let shown = walk_the_tabs(&mut c);
    assert!(shown.iter().any(|s| s == "porch"), "the walk shows it: {shown:?}");
    assert!(!is_member("porch"), "the Leave holds");
}
