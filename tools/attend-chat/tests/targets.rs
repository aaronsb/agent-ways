//! The chat's targets through the shell's real key and mouse handlers: a
//! tab, a column row, a sender's name and the destination flag each reach
//! one activation path. A left click (a digit, Enter on the column) selects
//! and on the target shown opens its menu; a right click opens the menu at
//! once, wherever a left click would eventually open one. In its own file,
//! so its own fixture: #deploy with alpha and the operator in it, #infra.

mod support;

use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;
use attend_chat::signal::{Channel, Signal};
use attend_chat::tabs::Tab;

const ALPHA_SID: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const SIZE: (u16, u16) = (80, 25);

fn chat() -> Chat {
    support::fixture("targets", |base| {
        support::pin_alive(ALPHA_SID);
        let alpha = attend_groups::Groups::new(base, ALPHA_SID);
        alpha.create("deploy", Some("rollout")).unwrap();
        alpha.join("deploy", true).unwrap();
        attend_groups::Groups::new(base, &attend_chat::signal::human_member_id()).join("deploy", true).unwrap();
        attend_groups::Groups::new(base, "").create("infra", None).unwrap();
    });
    let mut c = Chat::new(None, Palette::default(), Shape::PLAIN).sidebar(true).heartbeat(false).enhanced(true).dry_run(true);
    c.push(Signal {
        id: "t-1".into(),
        from: format!("claude:{ALPHA_SID}"),
        project: "alpha".into(),
        cwd: "/work/alpha".into(),
        reply_to: None,
        message: "a message in the feed".into(),
        ts: 1_790_949_000,
        channel: Channel::Open,
    });
    c
}

fn rows(c: &mut Chat) -> Vec<String> {
    testkit::rows(&testkit::render_screen(c, SIZE.0, SIZE.1))
}

/// The first cell showing `text`, on a fresh frame.
fn at(c: &mut Chat, text: &str) -> (u16, u16) {
    let buf = testkit::render_screen(c, SIZE.0, SIZE.1);
    testkit::find(&buf, text).unwrap_or_else(|| panic!("no {text:?} on the screen: {:#?}", testkit::rows(&buf)))
}

/// The cell of `name` left of the column's rule (`column`), or right of it.
fn cell(c: &mut Chat, name: &str, column: bool) -> (u16, u16) {
    for (y, r) in rows(c).iter().enumerate() {
        let Some(rule) = r.find('│') else { continue };
        let found = if column { r.find(name).filter(|x| *x < rule) } else { r[rule + '│'.len_utf8()..].find(name).map(|x| x + rule + '│'.len_utf8()) };
        if let Some(x) = found {
            return (r[..x].chars().count() as u16, y as u16);
        }
    }
    panic!("no {name} there: {:#?}", rows(c));
}

fn button(c: &mut Chat, (x, y): (u16, u16), b: MouseButton) {
    testkit::render_screen(c, SIZE.0, SIZE.1);
    c.mouse(MouseEvent { kind: MouseEventKind::Down(b), column: x, row: y, modifiers: KeyModifiers::NONE });
}

fn click(c: &mut Chat, at: (u16, u16)) {
    button(c, at, MouseButton::Left);
}

fn right(c: &mut Chat, at: (u16, u16)) {
    button(c, at, MouseButton::Right);
}

fn press(c: &mut Chat, code: KeyCode, m: KeyModifiers) {
    testkit::render_screen(c, SIZE.0, SIZE.1);
    testkit::drive(c, &[KeyEvent::new(code, m)]);
}

/// The open menu's title and the row it is drawn on, if one is open.
fn menu(c: &mut Chat) -> Option<(String, usize)> {
    rows(c).iter().enumerate().find_map(|(y, r)| {
        let i = r.find("pick one: ")?;
        let title: String = r[i + "pick one: ".len()..].chars().take_while(|ch| *ch != '─').collect();
        Some((title, y))
    })
}

/// The alpha agent's name as the column shows it.
fn alpha_name(c: &mut Chat) -> String {
    let shown = rows(c);
    let row = &shown[2];
    let rule = row.find('│').unwrap_or(row.len());
    row[..rule].split_whitespace().next().expect("the first agent row").to_string()
}

/// A right click on a channel's row opens its menu at once, under the row,
/// and shows nothing: as a right click on its tab does.
#[test]
fn a_right_click_on_a_channel_row_opens_its_menu_at_once() {
    let mut c = chat();
    let infra = cell(&mut c, "#infra", true);
    right(&mut c, infra);
    assert_eq!(menu(&mut c), Some(("#infra".into(), infra.1 as usize + 1)), "{:#?}", rows(&mut c));
    assert_eq!(c.foreground(), &Tab::Merged, "shown nothing");
}

/// A right click on an agent's row opens its menu under the row.
#[test]
fn a_right_click_on_an_agent_row_opens_its_menu() {
    let mut c = chat();
    let name = alpha_name(&mut c);
    let row = cell(&mut c, &name, true);
    right(&mut c, row);
    assert_eq!(menu(&mut c), Some((format!("@{name}"), row.1 as usize + 1)), "{:#?}", rows(&mut c));
}

/// A right click on a sender's name in the feed opens that agent's menu,
/// the column's own, under the name.
#[test]
fn a_right_click_on_a_senders_name_opens_the_agents_menu() {
    let mut c = chat();
    let name = alpha_name(&mut c);
    let sender = cell(&mut c, &name, false);
    right(&mut c, sender);
    let (title, y) = menu(&mut c).unwrap_or_else(|| panic!("no menu: {:#?}", rows(&mut c)));
    assert_eq!(title, format!("@{name}"));
    assert_eq!(y, sender.1 as usize + 1, "under the name");
    assert!(rows(&mut c).iter().any(|r| r.contains(&format!("Address @{name}"))));
}

/// A click on the destination flag opens the menu of the channel it names,
/// docked at the flag; a right click does the same. A clicked element acts
/// on what it shows.
#[test]
fn a_click_on_the_destination_flag_opens_the_menu_of_the_channel_it_names() {
    for b in [MouseButton::Left, MouseButton::Right] {
        let mut c = chat();
        press(&mut c, KeyCode::Char('3'), KeyModifiers::CONTROL);
        assert_eq!(c.foreground(), &Tab::Channel("deploy".into()));
        let flag = cell(&mut c, "#deploy ", false);
        let flag = rows(&mut c).iter().enumerate().rev().find_map(|(y, r)| r.rfind("#deploy").map(|x| (r[..x].chars().count() as u16, y as u16))).unwrap_or(flag);
        button(&mut c, flag, b);
        let (title, y) = menu(&mut c).unwrap_or_else(|| panic!("{b:?}: no menu: {:#?}", rows(&mut c)));
        assert_eq!(title, "#deploy", "{b:?}");
        assert!(y < flag.1 as usize, "{b:?}: docked at the flag, above it where the foot has no room");
        assert_eq!(c.foreground(), &Tab::Channel("deploy".into()));
    }
    // On merged the flag reads #open: #open's menu, and merged stays shown.
    let mut c = chat();
    let flag = rows(&mut c).iter().enumerate().rev().find_map(|(y, r)| r.rfind("#open").map(|x| (r[..x].chars().count() as u16, y as u16))).expect("the flag");
    click(&mut c, flag);
    assert_eq!(menu(&mut c).map(|(t, _)| t), Some("#open".into()));
    assert_eq!(c.foreground(), &Tab::Merged);
}

/// No visible change: the tab, its digit and the column row reach the same
/// state by one path. The first activation shows #deploy, the second opens
/// its menu; a right click opens the menu without showing.
#[test]
fn every_input_on_a_channel_reaches_the_same_state() {
    type Input = fn(&mut Chat);
    let inputs: [(&str, Input); 3] = [
        ("tab", |c| {
            let t = at(c, "3 #deploy");
            click(c, t)
        }),
        ("digit", |c| press(c, KeyCode::Char('3'), KeyModifiers::CONTROL)),
        ("row", |c| {
            let r = cell(c, "#deploy", true);
            click(c, r)
        }),
    ];
    for (how, input) in inputs {
        let mut c = chat();
        input(&mut c);
        let shown = menu(&mut c).map(|(t, _)| t);
        assert_eq!((c.foreground(), shown), (&Tab::Channel("deploy".into()), None), "{how}: the first shows it");
        input(&mut c);
        assert_eq!(menu(&mut c).map(|(t, _)| t), Some("#deploy".into()), "{how}: the second opens its menu");
    }
    for (how, at_) in [("tab", true), ("row", false)] {
        let mut c = chat();
        let target = if at_ { at(&mut c, "3 #deploy") } else { cell(&mut c, "#deploy", true) };
        right(&mut c, target);
        let shown = menu(&mut c).map(|(t, _)| t);
        assert_eq!((c.foreground(), shown), (&Tab::Merged, Some("#deploy".into())), "{how}: a right click opens the menu, showing nothing");
    }
}
