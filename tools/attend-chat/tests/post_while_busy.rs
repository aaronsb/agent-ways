//! A post in a channel makes the person its member (ADR-404 D1). When the
//! join that makes them one is refused because another writer holds the
//! channels past the chat's wait, the post still goes out and the status
//! row says the join did not happen. In its own file, so its own fixture:
//! the held lock would refuse other tests' writes.

mod support;

use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;

const AGENT_SID: &str = "aaaaaaaa-0000-4000-8000-000000000002";

#[test]
fn a_post_whose_join_is_refused_says_so() {
    support::fixture("post-while-busy", |base| {
        support::pin_alive(AGENT_SID);
        attend_groups::Groups::new(base, AGENT_SID).join("room", false).unwrap();
    });
    let mut c = Chat::new(None, Palette::default(), Shape::PLAIN).enhanced(false);
    let base = attend_chat::signal::signals_base();
    let held = agent_settings::writer::Lock::acquire(&base.join("_groups.yaml")).unwrap();
    for ch in "#room are you there".chars() {
        testkit::render_screen(&mut c, 80, 25);
        assert!(testkit::drive(&mut c, &[KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)]));
    }
    testkit::render_screen(&mut c, 80, 25);
    assert!(testkit::drive(&mut c, &[KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)]));
    drop(held);
    assert_eq!(c.status(), format!("sent → #room (not joined: {})", attend_groups::BUSY));
    let members = attend_groups::Groups::new(&base, "").members("room").unwrap();
    assert!(!members.iter().any(|m| m == "aaron"), "the refused join wrote nothing: {members:?}");
}
