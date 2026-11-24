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

use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;
use attend_chat::hidden::Hidden;
use attend_chat::last_tab::{self, Last};
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

/// A state directory of the test's own: tests run in parallel in one
/// fixture.
fn state(name: &str) -> PathBuf {
    root().join("state").join(name)
}

/// The record file in the state directory `dir`.
fn record(dir: &std::path::Path) -> String {
    std::fs::read_to_string(last_tab::path(dir)).unwrap()
}

/// A chat as `main` starts one: reopening and recording in `dir`.
/// Alt+digit on the channel shown shows it again rather than its menu, so
/// [`show`] can walk the numbers.
fn chat(dir: &PathBuf) -> Chat {
    bare().reopen_last_tab(dir)
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

#[test]
fn a_restart_reopens_the_channel_shown_last() {
    let dir = state("restart");
    let mut first = chat(&dir);
    assert_eq!(*first.foreground(), Tab::Merged, "a first start opens the operator view");
    show(&mut first, "kept");
    assert_eq!(record(&dir), "#kept\tmember\n", "recorded as it is shown, before any exit");
    show(&mut first, "viewed");
    assert_eq!(record(&dir), "#viewed\n", "a channel the operator only views");
    drop(first);
    assert_eq!(*chat(&dir).foreground(), channel("viewed"));
}

#[test]
fn the_operator_view_reopens_too() {
    let dir = state("all");
    let mut first = chat(&dir);
    show(&mut first, "kept");
    press(&mut first, KeyEvent::new(KeyCode::Char('0'), KeyModifiers::ALT));
    assert_eq!(*first.foreground(), Tab::Merged);
    assert_eq!(record(&dir), "all\n");
    assert_eq!(*chat(&dir).foreground(), Tab::Merged);
}

#[test]
fn a_channel_dissolved_since_opens_open() {
    let dir = state("dissolved");
    Last { tab: channel("doomed"), member: true }.save(&last_tab::path(&dir)).unwrap();
    attend_groups::Groups::new(&attend_chat::signal::signals_base(), "aaron").dissolve("doomed").unwrap();
    assert_eq!(*chat(&dir).foreground(), channel("open"));
}

#[test]
fn a_channel_left_since_opens_open() {
    let dir = state("left");
    Last { tab: channel("parting"), member: true }.save(&last_tab::path(&dir)).unwrap();
    attend_groups::Groups::new(&attend_chat::signal::signals_base(), "aaron").leave("parting").unwrap();
    assert_eq!(*chat(&dir).foreground(), channel("open"));
}

#[test]
fn a_channel_never_recorded_opens_open() {
    let dir = state("unknown");
    Last { tab: channel("nowhere"), member: false }.save(&last_tab::path(&dir)).unwrap();
    assert_eq!(*chat(&dir).foreground(), channel("open"));
}

/// The issue's case: a channel made with Ctrl+N is shown before the
/// operator is in it. A post joins it and a Leave takes them out, with no
/// change of tab; the restart opens #open.
#[test]
fn a_channel_joined_while_shown_and_then_left_opens_open() {
    let dir = state("joined");
    let mut first = chat(&dir);
    press(&mut first, KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
    type_line(&mut first, "deploy");
    assert_eq!(*first.foreground(), channel("deploy"), "{}", first.status());
    // A live agent joins, so the post has a reader.
    attend_groups::Groups::new(&attend_chat::signal::signals_base(), AGENT_SID).join("deploy", false).unwrap();
    type_line(&mut first, "anyone here?");
    assert_eq!(first.status(), "sent → #deploy");
    type_line(&mut first, "/leave");
    assert_eq!(first.status(), "left #deploy");
    testkit::render_screen(&mut first, 80, 25);
    assert_eq!(record(&dir), "#deploy\tmember\n", "a member while it was shown, though not now");
    drop(first);
    assert_eq!(*chat(&dir).foreground(), channel("open"));
}

#[test]
fn no_record_or_a_malformed_one_opens_the_operator_view() {
    let dir = state("missing");
    assert_eq!(*chat(&dir).foreground(), Tab::Merged, "no file");
    std::fs::create_dir_all(&dir).unwrap();
    for bad in ["", "kept\n", "#kept\tguest\n", "#kept\n#viewed\n"] {
        std::fs::write(last_tab::path(&dir), bad).unwrap();
        let c = chat(&dir);
        assert_eq!(*c.foreground(), Tab::Merged, "{bad:?}");
        assert_eq!(c.status(), "", "{bad:?}: nothing said");
    }
}

#[test]
fn a_chat_that_does_not_reopen_records_nothing() {
    let mut c = bare();
    show(&mut c, "kept");
    let dir = root().join("state").join("attend-chat");
    assert!(!last_tab::path(&dir).exists() && !last_tab::shown_path(&dir).exists(), "the default state files are left alone");
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

/// Whether the agent's row in the left column marks a new DM.
fn marked(c: &mut Chat) -> bool {
    let name = agent_identity::Identity::for_cwd("/work/alpha", agent_theme::ColorDepth::NoColor).nickname.to_string();
    let rows = testkit::rows(&testkit::render_screen(c, 120, 35));
    let row = rows.iter().skip(1).map(|r| r.split('│').next().unwrap_or("").to_string()).find(|r| r.contains(&name));
    row.unwrap_or_else(|| panic!("no row for {name}: {}", rows.join("\n"))).contains('✉')
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

/// With no operator view mark kept, a chat that opens on a channel counts
/// the DMs from before its start as shown, as it would had it opened on the
/// operator view; one sent since marks the agent's row.
#[test]
fn with_no_mark_kept_only_the_dms_sent_since_the_start_are_new() {
    let dir = state("dms");
    Last { tab: channel("kept"), member: true }.save(&last_tab::path(&dir)).unwrap();
    let mut c = chat(&dir).heartbeat(false).dry_run(true);
    assert_eq!(*c.foreground(), channel("kept"));
    c.push(dm("d-1", 1_790_949_000));
    assert!(!marked(&mut c), "a DM from before the start is not new");
    c.push(dm("d-2", now() + 60));
    assert!(marked(&mut c), "a DM since marks the agent");
}

/// The operator view's mark outlives the chat: a DM that came while no
/// chat ran, newer than the mark the last chat kept, marks the agent's row
/// in a chat that reopens a channel, and an older one does not.
#[test]
fn a_dm_that_came_while_the_chat_was_closed_is_new() {
    let dir = state("closed");
    Last { tab: channel("kept"), member: true }.save(&last_tab::path(&dir)).unwrap();
    let mark = now() - 3600;
    Hidden::hide_in(&last_tab::shown_path(&dir), &Tab::Merged, (mark, 0, 0)).unwrap();
    let mut c = chat(&dir).heartbeat(false).dry_run(true);
    c.push(dm("d-1", mark - 60));
    assert!(!marked(&mut c), "a DM the operator view showed is not new");
    c.push(dm("d-2", mark + 60));
    assert!(marked(&mut c), "a DM from while the chat was closed marks the agent");
}

/// Leaving the operator view keeps its mark for the next chat: the DMs it
/// held stay seen there, and the next one does not.
#[test]
fn leaving_the_operator_view_keeps_its_mark_for_the_next_chat() {
    let dir = state("kept-mark");
    let held = now() - 3600;
    let mut first = chat(&dir).heartbeat(false).dry_run(true);
    first.push(dm("d-1", held));
    show(&mut first, "kept");
    drop(first);
    let mut next = chat(&dir).heartbeat(false).dry_run(true);
    assert_eq!(*next.foreground(), channel("kept"));
    next.push(dm("d-1", held));
    assert!(!marked(&mut next), "the DM the operator view showed");
    next.push(dm("d-2", held + 60));
    assert!(marked(&mut next), "a DM after it, from before this start");
}

/// Click the agent's row in the left column, which shows its DM view.
fn open_agent_dms(c: &mut Chat) {
    let name = agent_identity::Identity::for_cwd("/work/alpha", agent_theme::ColorDepth::NoColor).nickname.to_string();
    let rows = testkit::rows(&testkit::render_screen(c, 120, 35));
    let (x, y) = rows
        .iter()
        .enumerate()
        .skip(1)
        .find_map(|(y, r)| {
            let col = r.split('│').next()?;
            col.find(&name).map(|b| (col[..b].chars().count() as u16, y as u16))
        })
        .unwrap_or_else(|| panic!("no {name} in the column: {rows:#?}"));
    c.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE });
    assert_eq!(*c.foreground(), Tab::Dm(AGENT_SID.into()), "the click shows the agent's DMs");
}

/// A DM read in the agent's DM view, without the operator view shown,
/// stays read after a restart that reopens a channel.
#[test]
fn a_dm_read_in_its_dm_view_stays_read_after_a_restart() {
    let dir = state("dm-view");
    Last { tab: channel("kept"), member: true }.save(&last_tab::path(&dir)).unwrap();
    let mark = now() - 3600;
    Hidden::hide_in(&last_tab::shown_path(&dir), &Tab::Merged, (mark, 0, 0)).unwrap();
    let mut first = chat(&dir).heartbeat(false).dry_run(true);
    first.push(dm("d-1", mark + 60));
    assert!(marked(&mut first), "the DM came after the operator view's mark");
    open_agent_dms(&mut first);
    show(&mut first, "kept");
    assert!(!marked(&mut first), "read in its DM view");
    drop(first);
    let mut next = chat(&dir).heartbeat(false).dry_run(true);
    assert_eq!(*next.foreground(), channel("kept"));
    next.push(dm("d-1", mark + 60));
    assert!(!marked(&mut next), "still read after the restart");
    next.push(dm("d-2", mark + 120));
    assert!(marked(&mut next), "a later DM still marks the agent");
}

/// A chat that quits, or is killed, while an agent's DM view is shown has
/// never left the view, so no change of tab kept its mark. The next read of
/// the world keeps it: the DM read there stays read after the restart.
#[test]
fn a_dm_read_in_a_dm_view_shown_at_the_quit_stays_read() {
    let dir = state("dm-view-quit");
    Last { tab: channel("kept"), member: true }.save(&last_tab::path(&dir)).unwrap();
    let mark = now() - 3600;
    Hidden::hide_in(&last_tab::shown_path(&dir), &Tab::Merged, (mark, 0, 0)).unwrap();
    let mut first = chat(&dir).heartbeat(false).dry_run(true);
    first.push(dm("d-1", mark + 60));
    assert!(marked(&mut first), "the DM came after the operator view's mark");
    open_agent_dms(&mut first);
    // The idle refresh, every few seconds while the chat runs.
    first.refresh();
    drop(first);
    let mut next = chat(&dir).heartbeat(false).dry_run(true);
    assert_eq!(*next.foreground(), channel("kept"));
    next.push(dm("d-1", mark + 60));
    assert!(!marked(&mut next), "still read after the restart");
    next.push(dm("d-2", mark + 120));
    assert!(marked(&mut next), "a later DM still marks the agent");
}

/// The operator view's mark marks the agents' DM-view marks it covers
/// as no longer needed, and the file drops them.
#[test]
fn the_operator_view_mark_drops_the_dm_marks_it_covers() {
    let dir = state("prune");
    let path = last_tab::shown_path(&dir);
    Hidden::shown_in(&path, &Tab::Dm("old".into()), (100, 0, 0)).unwrap();
    Hidden::shown_in(&path, &Tab::Dm("new".into()), (300, 0, 0)).unwrap();
    let kept = Hidden::shown_in(&path, &Tab::Merged, (200, 0, 0)).unwrap();
    assert!(kept.has(&Tab::Merged) && kept.has(&Tab::Dm("new".into())) && !kept.has(&Tab::Dm("old".into())), "{kept:?}");
    assert_eq!(Hidden::load(&path), kept, "as written");
}

/// With no mark kept, a chat that reopens a channel keeps the start as the
/// operator view's mark, so the next chat starts from it and not from its
/// own start.
#[test]
fn the_fallback_mark_is_kept_for_the_next_chat() {
    let dir = state("fallback");
    Last { tab: channel("kept"), member: true }.save(&last_tab::path(&dir)).unwrap();
    let before = now();
    let first = chat(&dir).heartbeat(false).dry_run(true);
    let after = now();
    drop(first);
    let kept = Hidden::load(&last_tab::shown_path(&dir));
    assert!(kept.has(&Tab::Merged), "the start is written: {kept:?}");
    assert!(kept.hides(&Tab::Merged, &dm("old", before - 1)) && !kept.hides(&Tab::Merged, &dm("later", after + 1)), "{kept:?}");
}
