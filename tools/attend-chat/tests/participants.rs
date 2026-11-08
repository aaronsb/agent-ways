// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The participant strip over the feed (ADR-409's note), through the
//! shell's real key and mouse handlers. In its own file, so its own
//! fixture: eighteen live claudes, all in #crowd, enough for the strip's
//! three rows and its cut; #deploy with alpha in it and an invitee that
//! exited without a session record, named by its id; #quiet with no
//! members.

mod support;

use agent_theme::ColorDepth;
use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;
use attend_chat::signal::{Channel, Signal};
use attend_chat::tabs::Tab;

const ALPHA_SID: &str = "aaaaaaaa-0000-4000-8000-000000000001";
/// Invited to #deploy, it exited without listening: no session record,
/// no heartbeat. The strip names it by its id.
const GONE_SID: &str = "dddddddd-0000-4000-8000-000000000099";

fn sid(i: usize) -> String {
    if i == 0 {
        ALPHA_SID.to_string()
    } else {
        format!("eeeeeeee-0000-4000-8000-0000000000{i:02}")
    }
}

fn cwd(i: usize) -> String {
    if i == 0 {
        "/work/alpha".to_string()
    } else {
        format!("/work/crowd-{i:02}")
    }
}

fn fixture() {
    support::fixture("participants", |base| {
        let home = std::path::PathBuf::from(std::env::var("HOME").unwrap());
        std::fs::create_dir_all(home.join(".claude/sessions")).unwrap();
        for i in 0..18 {
            support::pin_alive(&sid(i));
            let record = format!("{{\"pid\":1,\"sessionId\":\"{}\",\"cwd\":\"{}\"}}\n", sid(i), cwd(i));
            std::fs::write(home.join(format!(".claude/sessions/{i}.json")), record).unwrap();
        }
        let none = attend_groups::Groups::new(base, "");
        for c in ["crowd", "deploy", "quiet"] {
            none.create(c, None).unwrap();
        }
        for i in 0..18 {
            attend_groups::Groups::new(base, &sid(i)).join("crowd", true).unwrap();
        }
        attend_groups::Groups::new(base, ALPHA_SID).join("deploy", true).unwrap();
        std::fs::write(base.join(format!("@deploy/{GONE_SID}.invite")), "by: aaron\n").unwrap();
    });
}

fn chat_at(depth: ColorDepth, shape: Shape) -> Chat {
    fixture();
    let (painter, _) = agent_theme::Painter::named_in(None, None, depth);
    let mut c = Chat::new(None, Palette { painter }, shape).sidebar(true).heartbeat(false).enhanced(true).dry_run(true);
    c.push(sig("s-1", &format!("claude:{ALPHA_SID}"), "alpha to you", Channel::Person));
    c
}

fn chat() -> Chat {
    chat_at(ColorDepth::Ansi16, Shape::PLAIN)
}

fn sig(id: &str, from: &str, message: &str, channel: Channel) -> Signal {
    Signal {
        id: id.into(),
        from: from.into(),
        project: "alpha".into(),
        cwd: cwd(0),
        reply_to: None,
        message: message.into(),
        ts: 1_790_949_000,
        channel,
        tray: String::new(),
        beside_post: false,
    }
}

fn rows_at(c: &mut Chat, (w, h): (u16, u16)) -> Vec<String> {
    testkit::rows(&testkit::render_screen(c, w, h))
}

/// The part of each row right of the left column's rule, the whole row
/// where the column is hidden.
fn content(rows: &[String]) -> Vec<String> {
    rows.iter().map(|r| r.split_once('│').map_or(r.as_str(), |(_, rest)| rest).to_string()).collect()
}

/// Show `tab` by the column's Ctrl+digit, whichever number it has.
fn show(c: &mut Chat, size: (u16, u16), tab: Tab) {
    for d in '1'..='9' {
        rows_at(c, size);
        testkit::drive(c, &[KeyEvent::new(KeyCode::Char(d), KeyModifiers::CONTROL)]);
        if *c.foreground() == tab {
            return;
        }
    }
    panic!("no Alt+digit shows {tab:?}: {:#?}", rows_at(c, size));
}

fn click(c: &mut Chat, size: (u16, u16), text: &str, button: MouseButton) {
    let rows = rows_at(c, size);
    let (x, y) = rows
        .iter()
        .enumerate()
        .skip(1)
        .find_map(|(y, r)| r.find(text).map(|b| (r[..b].chars().count() as u16, y as u16)))
        .unwrap_or_else(|| panic!("no {text}: {rows:#?}"));
    c.mouse(MouseEvent { kind: MouseEventKind::Down(button), column: x, row: y, modifiers: KeyModifiers::NONE });
}

fn nick(i: usize) -> String {
    agent_identity::Identity::for_cwd(&cwd(i), ColorDepth::NoColor).nickname.to_string()
}

/// The strip's rows: those between the tab bar and the feed's top border.
fn strip_rows(rows: &[String]) -> Vec<String> {
    content(rows).into_iter().skip(1).take_while(|r| !r.trim_start().starts_with('╭')).collect()
}

#[test]
fn a_crowded_channel_takes_three_rows_and_ends_the_third_with_the_cut_at_both_depths() {
    for (depth, mark) in [(ColorDepth::Ansi16, "..."), (ColorDepth::TrueColor, "…")] {
        let size = (80, 25);
        let mut c = chat_at(depth, Shape::PLAIN);
        show(&mut c, size, Tab::Channel("crowd".into()));
        let rows = rows_at(&mut c, size);
        let strip = strip_rows(&rows);
        assert_eq!(strip.len(), 3, "three strip rows at {depth:?}: {rows:#?}");
        assert!(strip[2].trim_end().ends_with(&format!(" {mark}")), "the mark on the third row at {depth:?}: {:?}", strip[2]);
        let other = if mark == "..." { "…" } else { "..." };
        assert!(!strip.iter().any(|r| r.contains(other)), "only {mark} at {depth:?}: {strip:#?}");
        assert!(content(&rows)[4].trim_start().starts_with('╭'), "the feed under the strip at {depth:?}: {rows:#?}");
    }
}

#[test]
fn a_short_pane_keeps_the_compose_box_whole_and_the_cut_on_the_last_strip_row() {
    // 80x11: the shell's bars take two rows, the compose box four and the
    // feed its least of three, so #crowd's strip has two rows of its
    // three, and the second ends with the cut mark.
    let size = (80, 11);
    let mut c = chat();
    show(&mut c, size, Tab::Channel("crowd".into()));
    let rows = rows_at(&mut c, size);
    assert!(rows.iter().any(|r| r.contains("│ > ")), "the compose box's text row: {rows:#?}");
    let strip = strip_rows(&rows);
    assert_eq!(strip.len(), 2, "{rows:#?}");
    assert!(strip[1].trim_end().ends_with(" ..."), "{rows:#?}");
    // #deploy's two rows, its member and its invitee, fit beside it.
    show(&mut c, size, Tab::Channel("deploy".into()));
    let rows = rows_at(&mut c, size);
    assert!(rows.iter().any(|r| r.contains("│ > ")), "{rows:#?}");
    assert_eq!(strip_rows(&rows).len(), 2, "{rows:#?}");
}

#[test]
fn an_exited_invitee_named_by_its_id_keeps_its_whole_mark_in_forty_columns() {
    // The column is hidden for width: the strip has 39 columns, and the
    // invitee's 36-column id is cut so its chip, round caps and all, fits.
    for depth in [ColorDepth::Ansi16, ColorDepth::TrueColor] {
        let size = (40, 25);
        let mut c = chat_at(depth, Shape::ROUND);
        show(&mut c, size, Tab::Channel("deploy".into()));
        let rows = rows_at(&mut c, size);
        let strip = strip_rows(&rows);
        assert!(strip.iter().any(|r| r.contains("◌ invited ○")), "the invited chip whole at {depth:?}: {rows:#?}");
        assert!(strip.iter().any(|r| r.contains("dddddddd")), "named by its id at {depth:?}: {rows:#?}");
    }
}

#[test]
fn a_channel_with_no_members_has_no_strip() {
    let size = (80, 25);
    let mut c = chat();
    show(&mut c, size, Tab::Channel("quiet".into()));
    let rows = rows_at(&mut c, size);
    assert!(content(&rows)[1].trim_start().starts_with('╭'), "the feed under the tab bar: {rows:#?}");
}

#[test]
fn open_lists_the_live_identities() {
    let size = (120, 35);
    let mut c = chat();
    show(&mut c, size, Tab::Channel("open".into()));
    let strip = strip_rows(&rows_at(&mut c, size)).join("\n");
    assert!(strip.contains(&nick(0)) && strip.contains(&nick(1)), "live claudes on #open's strip: {strip}");
    assert!(!strip.contains("invited"), "{strip}");
}

#[test]
fn a_dm_view_lists_its_agent_and_the_person_and_a_review_lists_the_agents_counterparties() {
    let size = (120, 35);
    let mut c = chat();
    // The person's DM with alpha: alpha, and the person at the chat,
    // named by the username, as a message from them would be.
    click(&mut c, size, &nick(0), MouseButton::Left);
    assert_eq!(c.foreground(), &Tab::Dm(ALPHA_SID.into()));
    let strip = strip_rows(&rows_at(&mut c, size)).join("\n");
    assert!(strip.contains(&nick(0)) && strip.contains("aaron"), "alpha and the person: {strip}");
    // The DM view of an agent that holds no message: the agent, and the
    // person added, the operator's DMs being theirs before a message.
    click(&mut c, size, &nick(1), MouseButton::Left);
    assert_eq!(c.foreground(), &Tab::Dm(sid(1)));
    let strip = strip_rows(&rows_at(&mut c, size)).join("\n");
    assert!(strip.contains(&nick(1)) && strip.contains("aaron"), "the agent and the person: {strip}");
    // alpha's own view, Review DMs: alpha, and each counterparty of its
    // DMs, the person it wrote to and crowd-02.
    c.push(sig("s-2", &format!("claude:{ALPHA_SID}"), "alpha to crowd-02", Channel::Elsewhere(sid(2))));
    review(&mut c, size, 0);
    let strip = strip_rows(&rows_at(&mut c, size)).join("\n");
    assert!(strip.contains(&nick(0)) && strip.contains("aaron") && strip.contains(&nick(2)), "alpha and its counterparties: {strip}");
    // An agent's own view with no DM in it lists the agent alone: the
    // person is added only to the operator's DMs.
    review(&mut c, size, 3);
    let strip = strip_rows(&rows_at(&mut c, size)).join("\n");
    assert!(strip.contains(&nick(3)) && !strip.contains("aaron"), "the agent alone: {strip}");
}

/// Open agent `i`'s own view, Review DMs, from its row's menu.
fn review(c: &mut Chat, size: (u16, u16), i: usize) {
    click(c, size, &format!("{}  ", nick(i)), MouseButton::Right);
    click(c, size, "Review DMs", MouseButton::Left);
    let rows = rows_at(c, size);
    assert_eq!(c.foreground(), &Tab::Review(sid(i)), "{rows:#?}");
}
