// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! A restart reopens the tab the last chat showed (#1349): the chat records
//! each tab it shows, and the next chat opens on it, or on `#open` when the
//! channel is gone or the operator has left it. With no record, or one
//! that does not parse, the chat opens on the operator view, as it always
//! has. In its own file, so its own fixture.

mod support;

use std::path::PathBuf;

use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;
use attend_chat::last_tab::Last;
use attend_chat::signal::{Channel, Signal};
use attend_chat::tabs::Tab;

const AGENT_SID: &str = "aaaaaaaa-0000-4000-8000-000000000001";

/// The fixture: the operator, `aaron`, in #kept, #doomed and #parting; an
/// agent alone in #viewed.
fn root() -> &'static std::path::Path {
    support::fixture("last-tab", |base| {
        support::pin_alive(AGENT_SID);
        let me = attend_groups::Groups::new(base, "aaron");
        for room in ["kept", "doomed", "parting"] {
            me.join(room, true).unwrap();
        }
        attend_groups::Groups::new(base, AGENT_SID).join("viewed", true).unwrap();
    })
}

/// A state file of the test's own: tests run in parallel in one fixture.
fn state(name: &str) -> PathBuf {
    root().join("state").join("attend-chat").join(name)
}

/// A chat as `main` starts one: reopening and recording through `path`.
/// Alt+digit on the channel shown shows it again rather than its menu, so
/// [`show`] can walk the numbers.
fn chat(path: &PathBuf) -> Chat {
    bare().reopen_last_tab(path)
}

fn bare() -> Chat {
    root();
    Chat::new(None, Palette::default(), Shape::PLAIN).enhanced(false).menu_on_repeat(false)
}

fn channel(name: &str) -> Tab {
    Tab::Channel(name.into())
}

/// Show the channel `name` with Alt+digit, whichever number the column
/// gives it.
fn show(c: &mut Chat, name: &str) {
    for d in '2'..='9' {
        testkit::render_screen(c, 80, 25);
        assert!(testkit::drive(c, &[KeyEvent::new(KeyCode::Char(d), KeyModifiers::ALT)]));
        if *c.foreground() == channel(name) {
            return;
        }
    }
    panic!("no Alt+digit showed #{name}");
}

#[test]
fn a_restart_reopens_the_channel_shown_last() {
    let path = state("restart");
    let mut first = chat(&path);
    assert_eq!(*first.foreground(), Tab::Merged, "a first start opens the operator view");
    show(&mut first, "kept");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "#kept\tmember\n", "recorded as it is shown, before any exit");
    show(&mut first, "viewed");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "#viewed\n", "a channel the operator only views");
    drop(first);
    assert_eq!(*chat(&path).foreground(), channel("viewed"));
}

#[test]
fn the_operator_view_reopens_too() {
    let path = state("all");
    let mut first = chat(&path);
    show(&mut first, "kept");
    testkit::render_screen(&mut first, 80, 25);
    assert!(testkit::drive(&mut first, &[KeyEvent::new(KeyCode::Char('0'), KeyModifiers::ALT)]));
    assert_eq!(*first.foreground(), Tab::Merged);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "all\n");
    assert_eq!(*chat(&path).foreground(), Tab::Merged);
}

#[test]
fn a_channel_dissolved_since_opens_open() {
    let path = state("dissolved");
    Last { tab: channel("doomed"), member: true }.save(&path).unwrap();
    attend_groups::Groups::new(&attend_chat::signal::signals_base(), "aaron").dissolve("doomed").unwrap();
    assert_eq!(*chat(&path).foreground(), channel("open"));
}

#[test]
fn a_channel_left_since_opens_open() {
    let path = state("left");
    Last { tab: channel("parting"), member: true }.save(&path).unwrap();
    attend_groups::Groups::new(&attend_chat::signal::signals_base(), "aaron").leave("parting").unwrap();
    assert_eq!(*chat(&path).foreground(), channel("open"));
}

#[test]
fn a_channel_never_recorded_opens_open() {
    let path = state("unknown");
    Last { tab: channel("nowhere"), member: false }.save(&path).unwrap();
    assert_eq!(*chat(&path).foreground(), channel("open"));
}

#[test]
fn no_record_or_a_malformed_one_opens_the_operator_view() {
    let path = state("missing");
    assert_eq!(*chat(&path).foreground(), Tab::Merged, "no file");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    for bad in ["", "kept\n", "#kept\tguest\n", "#kept\n#viewed\n"] {
        std::fs::write(&path, bad).unwrap();
        let c = chat(&path);
        assert_eq!(*c.foreground(), Tab::Merged, "{bad:?}");
        assert_eq!(c.status(), "", "{bad:?}: nothing said");
    }
}

#[test]
fn a_chat_that_does_not_reopen_records_nothing() {
    let mut c = bare();
    show(&mut c, "kept");
    assert!(!state("last-tab").exists(), "the default state file is left alone");
}

/// A DM to the operator from the fixture's agent, sent at `ts`.
fn dm(id: &str, ts: u64) -> Signal {
    Signal {
        id: id.into(),
        from: format!("claude:{AGENT_SID}"),
        project: "alpha".into(),
        cwd: "/work/alpha".into(),
        reply_to: None,
        message: "a word for you".into(),
        ts,
        channel: Channel::Person,
        tray: String::new(),
        beside_post: false,
    }
}

/// The agent's row in the left column.
fn agent_row(c: &mut Chat) -> String {
    let name = agent_identity::Identity::for_cwd("/work/alpha", agent_theme::ColorDepth::NoColor).nickname.to_string();
    let rows = testkit::rows(&testkit::render_screen(c, 120, 35));
    rows.iter().skip(1).map(|r| r.split('│').next().unwrap_or("").to_string()).find(|r| r.contains(&name)).unwrap_or_else(|| panic!("no row for {name}: {}", rows.join("\n")))
}

/// A chat that opens on a channel has not shown the operator view, which
/// shows every DM. The DMs sent before it started still count as seen, as
/// they would had it opened there; one sent since marks the agent's row.
#[test]
fn a_reopened_channel_marks_only_the_dms_sent_since() {
    let path = state("dms");
    Last { tab: channel("kept"), member: true }.save(&path).unwrap();
    let mut c = chat(&path).heartbeat(false).dry_run(true);
    assert_eq!(*c.foreground(), channel("kept"));
    c.push(dm("d-1", 1_790_949_000));
    assert!(!agent_row(&mut c).contains('✉'), "a DM from before the start is not new: {}", agent_row(&mut c));
    let later = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() + 60;
    c.push(dm("d-2", later));
    assert!(agent_row(&mut c).contains('✉'), "a DM since marks the agent: {}", agent_row(&mut c));
}
