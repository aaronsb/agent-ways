//! An idle refresh of an unchanged world draws nothing new. In its own
//! file, so its own process and fixture: the screen suite's other tests
//! write to their shared fixture (a join, a send) while they run, and a
//! world that moved between two refreshes is a redraw this test would
//! count. The heartbeats are dated ahead, so no agent ages to quiet
//! between refreshes either.

mod support;

use std::time::{Duration, UNIX_EPOCH};

use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;
use attend_chat::signal::{Channel, Signal};

/// 2026-10-02 14:00 UTC.
const NOW: u64 = 1_790_949_600;
const ALPHA_SID: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const BETA_SID: &str = "bbbbbbbb-0000-4000-8000-000000000002";

fn sig(sid: &str, cwd: &str, ago: u64, channel: Channel) -> Signal {
    Signal {
        id: format!("t-{ago}"),
        from: format!("claude:{sid}"),
        project: cwd.rsplit('/').next().unwrap_or("").into(),
        cwd: cwd.into(),
        reply_to: None,
        message: "a message".into(),
        ts: NOW - ago,
        channel,
    }
}

fn chat(sidebar: bool) -> Chat {
    support::fixture("idle-refresh", |base| {
        let g = attend_groups::Groups::new(base, BETA_SID);
        g.join("deploy", true).unwrap();
        attend_groups::Groups::new(base, "").create("infra", None).unwrap();
        for sid in [ALPHA_SID, BETA_SID] {
            support::pin_alive(sid);
        }
    });
    let mut c = Chat::new(None, Palette::default(), Shape::PLAIN).sidebar(sidebar).heartbeat(false).pinned(UNIX_EPOCH + Duration::from_secs(NOW), 0);
    c.push(sig(ALPHA_SID, "/work/alpha", 600, Channel::Open));
    c.push(sig(BETA_SID, "/work/beta", 300, Channel::Group("deploy".into())));
    c
}

#[test]
fn an_idle_refresh_of_an_unchanged_world_draws_nothing_new() {
    for sidebar in [false, true] {
        let mut c = chat(sidebar);
        testkit::render_screen(&mut c, 80, 25);
        for _ in 0..3 {
            c.refresh();
            testkit::render_screen(&mut c, 80, 25);
        }
        assert_eq!(c.draws(), 1, "column {sidebar}: the peers and channels read the same: no re-layout, no redraw");
    }
}
