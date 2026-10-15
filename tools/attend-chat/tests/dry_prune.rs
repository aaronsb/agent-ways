//! A dry run reads the bus and changes nothing on it: the prune of
//! invitations to sessions that have gone, which a world read runs, is
//! skipped. In its own file, so its own fixture: no other test's world
//! read can prune the record it looks for.

mod support;

use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;

/// Invited to #infra, it exited without listening: no session record, no
/// heartbeat.
const GONE_SID: &str = "dddddddd-0000-4000-8000-000000000004";

#[test]
fn a_dry_run_leaves_an_invitation_to_a_session_that_has_gone() {
    support::fixture("dry-prune", |base| {
        attend_groups::Groups::new(base, "").create("infra", None).unwrap();
        attend_groups::Groups::new(base, "aaron").invite("infra", GONE_SID).unwrap();
    });
    let invite = attend_chat::signal::signals_base().join(format!("@infra/{GONE_SID}.invite"));
    let mut c = Chat::new(None, Palette::default(), Shape::PLAIN).sidebar(true).heartbeat(false).dry_run(true);
    testkit::render_screen(&mut c, 120, 35);
    c.refresh();
    testkit::render_screen(&mut c, 120, 35);
    assert!(invite.exists(), "a dry run prunes nothing");
}
