//! A click on a channel in the left column, through the shell's real mouse
//! handler: the first click shows the channel, a click on the channel
//! shown opens its menu, as the first and second Ctrl+digit do. The menu
//! docks on the line under its row, or ends on the line above it where the
//! rows below cannot hold it. In its own file, so its own fixture: sixteen
//! channels put the last row near the foot of an 80x25 screen.

mod support;

use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;
use attend_chat::signal::{Channel, Signal};
use attend_chat::tabs::Tab;

const ALPHA_SID: &str = "aaaaaaaa-0000-4000-8000-000000000001";

/// The last channel in the column, its row near the foot of the screen.
const LAST: &str = "ch14";

fn chat() -> Chat {
    support::fixture("channel-click", |base| {
        support::pin_alive(ALPHA_SID);
        let g = attend_groups::Groups::new(base, "");
        g.create("infra", None).unwrap();
        for i in 1..=14 {
            g.create(&format!("ch{i:02}"), None).unwrap();
        }
    });
    let mut c = Chat::new(None, Palette::default(), Shape::PLAIN).sidebar(true).heartbeat(false).enhanced(true).dry_run(true);
    c.push(Signal {
        id: "t-1".into(),
        from: format!("claude:{ALPHA_SID}"),
        project: "alpha".into(),
        cwd: "/work/alpha".into(),
        reply_to: None,
        message: "a message".into(),
        ts: 1_790_949_000,
        channel: Channel::Open,
    });
    c
}

fn rows(c: &mut Chat) -> Vec<String> {
    testkit::rows(&testkit::render_screen(c, 80, 25))
}

/// The cell of `name` in the left column, left of its rule.
fn row_of(c: &mut Chat, name: &str) -> (u16, u16) {
    let rows = rows(c);
    for (y, r) in rows.iter().enumerate() {
        let rule = r.find('│').unwrap_or(0);
        if let Some(x) = r.find(name).filter(|x| *x < rule) {
            return (r[..x].chars().count() as u16, y as u16);
        }
    }
    panic!("no {name} in the column: {rows:#?}");
}

/// A left click, as the terminal reports it, through the shell.
fn click(c: &mut Chat, (x, y): (u16, u16)) {
    testkit::render_screen(c, 80, 25);
    c.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE });
}

fn press(c: &mut Chat, code: KeyCode, m: KeyModifiers) {
    testkit::render_screen(c, 80, 25);
    assert!(testkit::drive(c, &[KeyEvent::new(code, m)]));
}

fn menu_open(c: &mut Chat) -> bool {
    rows(c).iter().any(|r| r.contains("pick one"))
}

#[test]
fn a_click_on_a_channel_not_shown_shows_it_and_opens_no_menu() {
    let mut c = chat();
    let at = row_of(&mut c, "#infra");
    click(&mut c, at);
    assert_eq!(c.foreground(), &Tab::Channel("infra".into()), "the click shows the channel");
    assert!(!menu_open(&mut c), "and opens no menu, as the first Ctrl+digit");
}

#[test]
fn a_click_on_the_channel_shown_opens_its_menu_on_the_line_under_its_row() {
    let mut c = chat();
    let at = row_of(&mut c, "#ch02");
    click(&mut c, at);
    click(&mut c, at);
    let rows = rows(&mut c);
    let under = &rows[usize::from(at.1) + 1];
    assert!(under.starts_with("┌pick one: #ch02"), "docked under the row, at the column's edge: {rows:#?}");
    assert!(rows[usize::from(at.1)].contains("#ch02"), "the row stays in sight");
}

#[test]
fn ctrl_digit_twice_docks_the_menu_under_the_channels_row_too() {
    let mut c = chat();
    let (_, y) = row_of(&mut c, "#open");
    press(&mut c, KeyCode::Char('2'), KeyModifiers::CONTROL);
    assert!(!menu_open(&mut c));
    press(&mut c, KeyCode::Char('2'), KeyModifiers::CONTROL);
    let rows = rows(&mut c);
    assert!(rows[usize::from(y) + 1].starts_with("┌pick one: #open"), "the keyboard's menu docks where the click's does: {rows:#?}");
}

/// Where Ctrl+digits do not arrive, Alt+digit stands in for them, again
/// on the tab shown for its menu, as the footer says.
#[test]
fn alt_digit_again_opens_the_menu_where_ctrl_digits_do_not_arrive() {
    let mut c = chat().enhanced(false);
    let (_, y) = row_of(&mut c, "#open");
    press(&mut c, KeyCode::Char('2'), KeyModifiers::ALT);
    assert_eq!(c.foreground(), &Tab::Channel("open".into()));
    assert!(!menu_open(&mut c));
    press(&mut c, KeyCode::Char('2'), KeyModifiers::ALT);
    let rows = rows(&mut c);
    assert!(rows[usize::from(y) + 1].starts_with("┌pick one: #open"), "{rows:#?}");
}

#[test]
fn near_the_foot_the_menu_ends_on_the_line_above_the_row_and_stays_on_screen() {
    let mut c = chat();
    let at = row_of(&mut c, &format!("#{LAST}"));
    assert!(at.1 >= 18, "the fixture puts the row near the foot: row {}", at.1);
    click(&mut c, at);
    assert_eq!(c.foreground(), &Tab::Channel(LAST.into()));
    click(&mut c, at);
    let rows = rows(&mut c);
    let y = usize::from(at.1);
    assert!(rows[y - 1].starts_with('└'), "the menu ends on the line above the row: {rows:#?}");
    let top = rows.iter().position(|r| r.contains(&format!("pick one: #{LAST}"))).expect("the menu is drawn");
    assert!(top >= 1, "below the tab bar: {rows:#?}");
    assert!(rows[y].contains(&format!("#{LAST}")), "the row stays in sight");
    assert!(rows.iter().any(|r| r.contains("Delete channel")), "every item on screen: {rows:#?}");
}

#[test]
fn a_click_on_an_agent_row_opens_its_menu_under_the_row_at_once() {
    let mut c = chat();
    let at = row_of(&mut c, "alpha");
    click(&mut c, at);
    let rows = rows(&mut c);
    assert!(rows[usize::from(at.1) + 1].starts_with("┌pick one: @"), "an agent has no foreground: its menu, docked: {rows:#?}");
}

#[test]
fn esc_and_a_click_elsewhere_close_the_menu() {
    let mut c = chat();
    let at = row_of(&mut c, "#ch02");
    click(&mut c, at);
    click(&mut c, at);
    assert!(menu_open(&mut c));
    press(&mut c, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!menu_open(&mut c), "Esc closes it");
    click(&mut c, at);
    assert!(menu_open(&mut c));
    click(&mut c, (60, 5));
    assert!(!menu_open(&mut c), "a click outside closes it");
    assert_eq!(c.foreground(), &Tab::Channel("ch02".into()));
}

/// A question waiting for `y`, then a click that opens a menu: the
/// question goes, so a `y` after the menu closes types.
#[test]
fn a_click_on_the_channel_shown_drops_a_waiting_question() {
    let mut c = chat();
    press(&mut c, KeyCode::Char('2'), KeyModifiers::CONTROL);
    press(&mut c, KeyCode::Char('2'), KeyModifiers::CONTROL);
    press(&mut c, KeyCode::Down, KeyModifiers::NONE);
    press(&mut c, KeyCode::Enter, KeyModifiers::NONE);
    assert!(rows(&mut c).last().is_some_and(|r| r.contains("clear #open history?")), "the question waits");
    let at = row_of(&mut c, "#open");
    click(&mut c, at);
    assert!(menu_open(&mut c));
    assert_eq!(c.status(), "kept", "the question is dropped, and says so");
    press(&mut c, KeyCode::Esc, KeyModifiers::NONE);
    press(&mut c, KeyCode::Char('y'), KeyModifiers::NONE);
    assert_eq!(c.input().text(), "y", "a y types: nothing answers it");
}
