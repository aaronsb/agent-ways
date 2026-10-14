//! A click on a channel in the left column, through the shell's real mouse
//! handler: the first click shows the channel, a click on the channel
//! shown opens its menu, as the first and second Ctrl+digit do. The column
//! numbers the channels from 1, `#open` first, so `#ch01` is Ctrl+2. The menu
//! docks on the line under its row, or ends on the line above it where the
//! rows below cannot hold it. In its own file, so its own fixture: 28
//! channels fill the column at 80x25, and at 120x35 put #infra's row below
//! where an 80x25 screen ends.

mod support;

use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;
use attend_chat::signal::{Channel, Signal};
use attend_chat::tabs::Tab;

const ALPHA_SID: &str = "aaaaaaaa-0000-4000-8000-000000000001";

fn chat() -> Chat {
    support::fixture("channel-click", |base| {
        support::pin_alive(ALPHA_SID);
        let g = attend_groups::Groups::new(base, "");
        g.create("infra", None).unwrap();
        for i in 1..=26 {
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
        tray: String::new(),
        beside_post: false,
    });
    c
}

fn rows(c: &mut Chat) -> Vec<String> {
    rows_at(c, (80, 25))
}

fn rows_at(c: &mut Chat, (w, h): (u16, u16)) -> Vec<String> {
    testkit::rows(&testkit::render_screen(c, w, h))
}

/// The cell of `name` in the left column, left of its rule.
fn row_of(c: &mut Chat, name: &str) -> (u16, u16) {
    row_at(c, name, (80, 25))
}

fn row_at(c: &mut Chat, name: &str, size: (u16, u16)) -> (u16, u16) {
    let rows = rows_at(c, size);
    for (y, r) in rows.iter().enumerate() {
        let rule = r.find('│').unwrap_or(0);
        if let Some(x) = r.find(name).filter(|x| *x < rule) {
            return (r[..x].chars().count() as u16, y as u16);
        }
    }
    panic!("no {name} in the column: {rows:#?}");
}

/// A left click, as the terminal reports it, through the shell.
fn click(c: &mut Chat, at: (u16, u16)) {
    click_at(c, at, (80, 25));
}

fn click_at(c: &mut Chat, (x, y): (u16, u16), (w, h): (u16, u16)) {
    testkit::render_screen(c, w, h);
    c.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE });
}

/// The column's last channel row on an 80x25 screen, and its name.
fn last_channel(c: &mut Chat) -> ((u16, u16), String) {
    let rows = rows(c);
    let (y, r) = rows.iter().enumerate().rev().find(|(_, r)| r.find('#').is_some_and(|x| x < r.find('│').unwrap_or(0))).expect("a channel row");
    let name: String = r[r.find('#').unwrap() + 1..].chars().take_while(|ch| !ch.is_whitespace()).collect();
    ((r[..r.find('#').unwrap()].chars().count() as u16, y as u16), name)
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
    let at = row_of(&mut c, "#ch03");
    click(&mut c, at);
    assert_eq!(c.foreground(), &Tab::Channel("ch03".into()), "the click shows the channel");
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
    let (_, y) = row_of(&mut c, "#ch01");
    press(&mut c, KeyCode::Char('2'), KeyModifiers::CONTROL);
    assert!(!menu_open(&mut c));
    press(&mut c, KeyCode::Char('2'), KeyModifiers::CONTROL);
    let rows = rows(&mut c);
    assert!(rows[usize::from(y) + 1].starts_with("┌pick one: #ch01"), "the keyboard's menu docks where the click's does: {rows:#?}");
}

/// Where Ctrl+digits do not arrive, Alt+digit stands in for them, again
/// on the channel shown for its menu, as the footer says.
#[test]
fn alt_digit_again_opens_the_menu_where_ctrl_digits_do_not_arrive() {
    let mut c = chat().enhanced(false);
    let (_, y) = row_of(&mut c, "#ch01");
    press(&mut c, KeyCode::Char('2'), KeyModifiers::ALT);
    assert_eq!(c.foreground(), &Tab::Channel("ch01".into()));
    assert!(!menu_open(&mut c));
    press(&mut c, KeyCode::Char('2'), KeyModifiers::ALT);
    let rows = rows(&mut c);
    assert!(rows[usize::from(y) + 1].starts_with("┌pick one: #ch01"), "{rows:#?}");
}

#[test]
fn near_the_foot_the_menu_ends_on_the_line_above_the_row_and_stays_on_screen() {
    let mut c = chat();
    let (at, last) = last_channel(&mut c);
    assert!(at.1 >= 20, "the fixture puts the row at the foot: row {}", at.1);
    click(&mut c, at);
    assert_eq!(c.foreground(), &Tab::Channel(last.clone()));
    click(&mut c, at);
    let rows = rows(&mut c);
    let y = usize::from(at.1);
    assert!(rows[y - 1].starts_with('└'), "the menu ends on the line above the row: {rows:#?}");
    let top = rows.iter().position(|r| r.contains(&format!("pick one: #{last}"))).expect("the menu is drawn");
    assert!(top >= 1, "below the tab bar: {rows:#?}");
    assert!(rows[y].contains(&format!("#{last}")), "the row stays in sight");
    assert!(rows.iter().any(|r| r.contains("Delete channel")), "every item on screen: {rows:#?}");
}

#[test]
fn a_click_on_an_agent_row_shows_its_dms_and_a_second_opens_its_menu_under_the_row() {
    let mut c = chat();
    let at = row_of(&mut c, "alpha");
    click(&mut c, at);
    assert_eq!(c.foreground(), &Tab::Dm(ALPHA_SID.into()), "the first click shows its DMs");
    assert!(!menu_open(&mut c));
    click(&mut c, at);
    let rows = rows(&mut c);
    assert!(rows[usize::from(at.1) + 1].starts_with("┌pick one: @"), "the second, on the agent shown, its menu, docked: {rows:#?}");
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
    press(&mut c, KeyCode::End, KeyModifiers::NONE);
    press(&mut c, KeyCode::Enter, KeyModifiers::NONE);
    assert!(rows(&mut c).last().is_some_and(|r| r.contains("delete #ch01")), "the question waits");
    let at = row_of(&mut c, "#ch01");
    click(&mut c, at);
    assert!(menu_open(&mut c));
    assert_eq!(c.status(), "kept", "the question is dropped, and says so");
    press(&mut c, KeyCode::Esc, KeyModifiers::NONE);
    press(&mut c, KeyCode::Char('y'), KeyModifiers::NONE);
    assert_eq!(c.input().text(), "y", "a y types: nothing answers it");
}

/// With `menu_on_repeat` off, a second Ctrl+digit only shows the tab; a
/// click or Enter on the channel shown in the column still opens its menu.
#[test]
fn the_channel_shown_opens_its_menu_from_the_column_with_repeat_off() {
    let mut c = chat().menu_on_repeat(false);
    let at = row_of(&mut c, "#ch02");
    click(&mut c, at);
    assert!(!menu_open(&mut c));
    click(&mut c, at);
    let rows = rows(&mut c);
    assert!(rows[usize::from(at.1) + 1].starts_with("┌pick one: #ch02"), "a click opens it: {rows:#?}");
    press(&mut c, KeyCode::Esc, KeyModifiers::NONE);
    press(&mut c, KeyCode::Char('3'), KeyModifiers::CONTROL);
    press(&mut c, KeyCode::Char('3'), KeyModifiers::CONTROL);
    assert!(!menu_open(&mut c), "the key follows the setting");
    // F2, Down: the column's cursor on #ch02, the channel shown; Enter
    // opens its menu, as Enter on any element does.
    press(&mut c, KeyCode::F(2), KeyModifiers::NONE);
    press(&mut c, KeyCode::Down, KeyModifiers::NONE);
    press(&mut c, KeyCode::Enter, KeyModifiers::NONE);
    assert!(rows_at(&mut c, (80, 25)).iter().any(|r| r.contains("pick one: #ch02")), "Enter on the row opens it");
}

/// The column has the keyboard; a click on another row moves its cursor.
#[test]
fn a_click_in_the_focused_column_moves_its_cursor_to_the_row() {
    let mut c = chat();
    press(&mut c, KeyCode::F(2), KeyModifiers::NONE);
    press(&mut c, KeyCode::Down, KeyModifiers::NONE);
    let cursor = |c: &mut Chat| rows(c).into_iter().find(|r| r.starts_with('▸')).expect("the cursor is drawn");
    assert!(cursor(&mut c).contains("alpha"), "Down from the operator view's tab: the column's first row: {}", cursor(&mut c));
    let at = row_of(&mut c, "#ch05");
    click(&mut c, at);
    assert!(cursor(&mut c).contains("#ch05"), "the cursor follows the click: {}", cursor(&mut c));
}

/// A menu opened at 120x35 under a row that an 80x25 screen does not reach
/// stays on that screen when the terminal shrinks.
#[test]
fn a_menu_open_through_a_resize_stays_on_screen() {
    let mut c = chat();
    let big = (120, 35);
    let at = row_at(&mut c, "#infra", big);
    assert!(at.1 >= 24, "the row is below where 80x25 ends: row {}", at.1);
    click_at(&mut c, at, big);
    click_at(&mut c, at, big);
    assert!(rows_at(&mut c, big).iter().any(|r| r.contains("pick one: #infra")));
    let rows = rows(&mut c);
    let top = rows.iter().position(|r| r.contains("pick one: #infra")).expect("the menu is drawn at 80x25");
    let foot = rows.iter().rposition(|r| r.starts_with('└')).expect("its bottom border");
    assert!(top >= 1 && foot < 24 && foot > top, "inside the pane, above the bottom bar: {rows:#?}");
    assert!(rows.iter().any(|r| r.contains("Delete channel")), "every item on screen: {rows:#?}");
}
