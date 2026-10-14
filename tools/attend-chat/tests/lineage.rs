//! The left column reads the session lineage and the tombstones from disk
//! (ADR-404 D3e): a dead sighting under an id a live session had before its
//! `/clear` is that session's row, and a tombstoned session makes no row.
//! In its own file, so its own fixture.

mod support;

use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;
use attend_chat::signal::{Channel, Signal};

const OLD_SID: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const NEW_SID: &str = "bbbbbbbb-0000-4000-8000-000000000002";
const DEAD_SID: &str = "cccccccc-0000-4000-8000-000000000003";
const OTHER_SID: &str = "dddddddd-0000-4000-8000-000000000004";
// Each sighting under a cwd of its own, so no two share a name and only
// the lineage can fold them.
const OLD_CWD: &str = "/work/before";
const NEW_CWD: &str = "/work/after";
const DEAD_CWD: &str = "/work/silenced";
const OTHER_CWD: &str = "/work/bystander";

fn sig(id: &str, sid: &str, cwd: &str) -> Signal {
    Signal {
        id: id.into(),
        from: format!("claude:{sid}"),
        project: cwd.rsplit('/').next().unwrap().into(),
        cwd: cwd.into(),
        reply_to: None,
        message: "a message".into(),
        ts: 1_790_949_000,
        channel: Channel::Open,
        tray: String::new(),
        beside_post: false,
    }
}

fn nick(cwd: &str) -> String {
    agent_identity::Identity::for_cwd(cwd, agent_theme::ColorDepth::NoColor).nickname.to_string()
}

/// The left column's text, row by row.
fn column(c: &mut Chat) -> String {
    let rows = testkit::rows(&testkit::render_screen(c, 120, 35));
    rows.iter().skip(1).filter_map(|r| r.split('│').next()).collect::<Vec<_>>().join("\n")
}

#[test]
fn the_column_folds_an_earlier_id_and_leaves_out_a_tombstoned_session() {
    support::fixture("lineage", |base| {
        support::pin_alive(NEW_SID);
        attend_presence::enrollment::enroll(OLD_SID, attend_presence::enrollment::Source::Listen, None).unwrap();
        attend_presence::enrollment::carry(OLD_SID, NEW_SID).unwrap();
        attend_groups::tombstones::mark_session(base, DEAD_SID).unwrap();
    });
    let mut c = Chat::new(None, Palette::default(), Shape::PLAIN).sidebar(true).heartbeat(false).enhanced(true);
    c.push(sig("s-1", NEW_SID, NEW_CWD));
    c.push(sig("s-2", OTHER_SID, OTHER_CWD));
    c.push(sig("s-3", DEAD_SID, DEAD_CWD));
    c.push(sig("s-4", OLD_SID, OLD_CWD));
    let col = column(&mut c);
    assert!(col.contains(&nick(NEW_CWD)), "the live row: {col}");
    assert!(col.contains(&nick(OTHER_CWD)), "a dead session with no lineage keeps its row: {col}");
    assert!(!col.contains(&nick(OLD_CWD)), "the earlier id folds into the live row: {col}");
    assert!(!col.contains(&nick(DEAD_CWD)), "the tombstoned session makes no row: {col}");
}
