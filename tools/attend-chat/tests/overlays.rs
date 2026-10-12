//! The chat's questions and the rule that closes every overlay, through
//! the shell's real key and mouse handlers.
//!
//! A step that drops something asks on the shell's bottom bar, with its
//! two answers drawn as lozenges: `y` or a click on the first goes ahead,
//! `n`, Esc or a click on the second keeps things as they are, and a click
//! anywhere else answers no. Esc or a click outside closes the overlay on
//! top and says nothing; a click that closes one on the tab bar or the
//! left column then acts, as it would with nothing open. In its own file,
//! so its own fixture: #deploy with alpha and the operator in it, and
//! #infra.

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
    support::fixture("overlays", |base| {
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

fn bar(c: &mut Chat) -> String {
    rows(c).pop().expect("a bottom bar")
}

/// The first cell showing `text`, on a fresh frame.
fn at(c: &mut Chat, text: &str) -> (u16, u16) {
    let buf = testkit::render_screen(c, SIZE.0, SIZE.1);
    testkit::find(&buf, text).unwrap_or_else(|| panic!("no {text:?} on the screen: {:#?}", testkit::rows(&buf)))
}

/// The cell of `name` in the left column, left of its rule.
fn row_of(c: &mut Chat, name: &str) -> (u16, u16) {
    for (y, r) in rows(c).iter().enumerate() {
        let rule = r.find('│').unwrap_or(0);
        if let Some(x) = r.find(name).filter(|x| *x < rule) {
            return (r[..x].chars().count() as u16, y as u16);
        }
    }
    panic!("no {name} in the column: {:#?}", rows(c));
}

fn button(c: &mut Chat, (x, y): (u16, u16), b: MouseButton) {
    testkit::render_screen(c, SIZE.0, SIZE.1);
    c.mouse(MouseEvent { kind: MouseEventKind::Down(b), column: x, row: y, modifiers: KeyModifiers::NONE });
}

fn click(c: &mut Chat, at: (u16, u16)) {
    button(c, at, MouseButton::Left);
}

/// A key through the shell; false when the session ended.
fn press(c: &mut Chat, code: KeyCode, m: KeyModifiers) -> bool {
    testkit::render_screen(c, SIZE.0, SIZE.1);
    testkit::drive(c, &[KeyEvent::new(code, m)])
}

fn key(c: &mut Chat, code: KeyCode) -> bool {
    press(c, code, KeyModifiers::NONE)
}

fn menu_open(c: &mut Chat) -> bool {
    rows(c).iter().any(|r| r.contains("pick one"))
}

/// A cell inside the feed's box, right of the column, above the compose box.
fn feed(c: &mut Chat) -> (u16, u16) {
    let shown = rows(c);
    assert!(shown[10].chars().nth(50).is_some_and(|ch| ch == ' '), "inside the feed: {shown:#?}");
    (50, 10)
}

/// A step that asks first, and how it is reached: the tab whose menu offers
/// it (Ctrl+digit twice, or two clicks on the tab), the item, what its
/// question says, its answers, and what yes runs. An item that opens a
/// picker (Remove agent) asks once its first entry is picked.
struct Step {
    key: char,
    tab: &'static str,
    item: &'static str,
    question: &'static str,
    yes: &'static str,
    no: &'static str,
    ran: &'static str,
}

const STEPS: &[Step] = &[
    Step { key: '1', tab: "1 merged", item: "Clear view", question: "clear the view?", yes: " y clear ", no: " n keep ", ran: "dry run: /clear not run" },
    Step { key: '2', tab: "2 #open", item: "Clear history", question: "clear #open history?", yes: " y clear ", no: " n keep ", ran: "dry run: /purge not run" },
    Step { key: '3', tab: "3 #deploy", item: "Leave", question: "leave #deploy?", yes: " y leave ", no: " n stay ", ran: "dry run: /leave not run" },
    Step { key: '3', tab: "3 #deploy", item: "Delete channel", question: "delete #deploy and its history?", yes: " y delete ", no: " n keep ", ran: "dry run: /dissolve not run" },
    Step { key: '3', tab: "3 #deploy", item: "Remove agent", question: "from #deploy?", yes: " y remove ", no: " n keep ", ran: "dry run: /kick not run" },
];

/// The row of the open menu's title, if a menu is open.
fn menu_title(c: &mut Chat) -> Option<(usize, usize)> {
    rows(c).iter().enumerate().find_map(|(y, r)| r.find("pick one").map(|x| (r[..x].chars().count(), y)))
}

/// Open `tab`'s menu by Ctrl+digit and pick `item` by the keyboard; a
/// picker it opens gets Enter on its first entry.
fn ask_by_keys(c: &mut Chat, tab: char, item: &str) {
    press(c, KeyCode::Char(tab), KeyModifiers::CONTROL);
    if !menu_open(c) {
        press(c, KeyCode::Char(tab), KeyModifiers::CONTROL);
    }
    // The item's place in the menu: its row less the title's.
    let shown = rows(c);
    let title = shown.iter().position(|r| r.contains("pick one")).expect("the menu is open");
    let row = shown.iter().skip(title + 1).position(|r| r.contains(item)).unwrap_or_else(|| panic!("no {item} in {shown:#?}"));
    key(c, KeyCode::Home);
    for _ in 0..row {
        key(c, KeyCode::Down);
    }
    key(c, KeyCode::Enter);
    if menu_open(c) {
        key(c, KeyCode::Enter);
    }
}

/// Open `tab`'s menu by clicks on its tab and pick `item` by a click; a
/// picker it opens gets a click on its first entry.
fn ask_by_clicks(c: &mut Chat, tab: &str, item: &str) {
    let t = at(c, tab);
    click(c, t);
    if !menu_open(c) {
        click(c, t);
    }
    let i = at(c, item);
    click(c, i);
    if let Some((x, y)) = menu_title(c) {
        click(c, (x as u16 + 2, y as u16 + 1));
    }
}

#[test]
fn a_question_draws_its_answers_as_lozenges() {
    let mut c = chat();
    ask_by_clicks(&mut c, "1 merged", "Clear view");
    let shown = bar(&mut c);
    assert!(shown.contains("clear the view?") && shown.contains(" y clear ") && shown.contains(" n keep "), "{shown}");
    assert!(!c.signals().is_empty(), "nothing cleared before the answer");
}

#[test]
fn a_click_on_the_yes_answer_runs_the_step() {
    let mut c = chat();
    ask_by_clicks(&mut c, "1 merged", "Clear view");
    let yes = at(&mut c, " y clear ");
    click(&mut c, yes);
    assert_eq!(c.status(), "dry run: /clear not run", "the step ran, as far as a dry run runs one");
}

/// Only a left click gives an answer: a middle or a right click on the yes
/// lozenge answers no, as a click of any button elsewhere does.
#[test]
fn a_middle_or_right_click_on_the_yes_answer_answers_no() {
    for b in [MouseButton::Middle, MouseButton::Right] {
        let mut c = chat();
        ask_by_clicks(&mut c, "1 merged", "Clear view");
        let yes = at(&mut c, " y clear ");
        button(&mut c, yes, b);
        assert_eq!(c.status(), "kept", "{b:?} on yes ran nothing");
        assert!(!bar(&mut c).contains("clear the view?"), "{b:?}: nothing left asking");
    }
}

#[test]
fn a_click_on_the_no_answer_keeps_things() {
    let mut c = chat();
    ask_by_clicks(&mut c, "1 merged", "Clear view");
    let no = at(&mut c, " n keep ");
    click(&mut c, no);
    assert_eq!(c.status(), "kept");
    assert!(!c.signals().is_empty());
    assert!(!bar(&mut c).contains("clear the view?"));
}

/// A click in the feed answers no, so a later `y` types and answers nothing.
#[test]
fn a_click_in_the_feed_answers_no() {
    let mut c = chat();
    ask_by_clicks(&mut c, "1 merged", "Clear view");
    let f = feed(&mut c);
    click(&mut c, f);
    assert_eq!(c.status(), "kept");
    key(&mut c, KeyCode::Char('y'));
    assert_eq!(c.input().text(), "y", "the question is gone: y is typing");
    assert!(!c.signals().is_empty());
}

/// Ctrl-C over a question with a draft open: the exit guard asks, and the
/// question is answered no first, so going back from the guard finds it
/// gone and said, and a `y` types.
#[test]
fn ctrl_c_over_a_question_answers_it_no_before_the_guard_asks() {
    let mut c = chat();
    for ch in "half".chars() {
        key(&mut c, KeyCode::Char(ch));
    }
    ask_by_clicks(&mut c, "1 merged", "Clear view");
    assert!(bar(&mut c).contains("clear the view?"));
    assert!(press(&mut c, KeyCode::Char('c'), KeyModifiers::CONTROL), "a draft keeps the chat open");
    assert!(c.app().guarding());
    key(&mut c, KeyCode::Esc);
    assert!(!c.app().guarding());
    assert_eq!(c.status(), "kept", "the question was answered, and said so");
    key(&mut c, KeyCode::Char('y'));
    assert_eq!(c.input().text(), "halfy", "a y types: nothing is asking");
    assert!(!c.signals().is_empty());
}

/// Every step that asks, answered once by key and once by click, lands in
/// the same state: the same status, the same draft, nothing left asking.
#[test]
fn keys_and_clicks_answer_every_question_alike() {
    for s in STEPS {
        for yes in [true, false] {
            let mut by_key = chat();
            ask_by_keys(&mut by_key, s.key, s.item);
            assert!(bar(&mut by_key).contains(s.question), "{} by keys asks: {}", s.item, bar(&mut by_key));
            key(&mut by_key, KeyCode::Char(if yes { 'y' } else { 'n' }));

            let mut by_click = chat();
            ask_by_clicks(&mut by_click, s.tab, s.item);
            assert!(bar(&mut by_click).contains(s.question), "{} by clicks asks: {}", s.item, bar(&mut by_click));
            let answer = at(&mut by_click, if yes { s.yes } else { s.no });
            click(&mut by_click, answer);

            let want = if yes { s.ran } else { "kept" };
            assert_eq!(by_key.status(), want, "{}, yes {yes}, by keys", s.item);
            assert_eq!(by_click.status(), want, "{}, yes {yes}, by clicks", s.item);
            assert_eq!(by_key.input().text(), by_click.input().text(), "{}: the draft", s.item);
            assert_eq!(by_key.signals().len(), by_click.signals().len(), "{}: the feed", s.item);
            assert!(!bar(&mut by_key).contains(s.question) && !bar(&mut by_click).contains(s.question), "{}: nothing left asking", s.item);
        }
    }
}

/// A click on what a menu docks under closes it and does not open it
/// again: on a channel's row in the column, and on a tab.
#[test]
fn a_click_on_a_menus_own_anchor_only_closes_it() {
    let mut c = chat();
    let infra = row_of(&mut c, "#infra");
    click(&mut c, infra);
    click(&mut c, infra);
    assert!(menu_open(&mut c), "the second click opens #infra's menu under its row");
    click(&mut c, infra);
    assert!(!menu_open(&mut c), "a click on the row closes the menu and opens it no more");
    assert_eq!(c.foreground(), &Tab::Channel("infra".into()));

    let tab = at(&mut c, "3 #deploy");
    click(&mut c, tab);
    click(&mut c, tab);
    assert!(menu_open(&mut c), "the second click opens #deploy's menu under its tab");
    click(&mut c, tab);
    assert!(!menu_open(&mut c), "a click on the tab closes the menu and opens it no more");
}

/// A right click outside a lent compose box gives it back, as a left one
/// does.
#[test]
fn a_right_click_in_the_feed_gives_a_lent_compose_box_back() {
    let mut c = chat();
    for ch in "draft".chars() {
        key(&mut c, KeyCode::Char(ch));
    }
    press(&mut c, KeyCode::Char('n'), KeyModifiers::CONTROL);
    key(&mut c, KeyCode::Char('x'));
    let f = feed(&mut c);
    button(&mut c, f, MouseButton::Right);
    assert!(!bar(&mut c).contains("new channel"), "the box is back: {}", bar(&mut c));
    assert_eq!(c.input().text(), "draft");
}

/// Esc on a menu, or a click outside it, closes it and says nothing: the
/// bar does not read "unchanged".
#[test]
fn a_menu_closed_by_esc_or_a_click_outside_says_nothing() {
    let mut c = chat();
    ask_by_keys(&mut c, '3', "Leave");
    key(&mut c, KeyCode::Esc);
    press(&mut c, KeyCode::Char('3'), KeyModifiers::CONTROL);
    assert!(menu_open(&mut c));
    key(&mut c, KeyCode::Esc);
    assert!(!menu_open(&mut c));
    assert!(!bar(&mut c).contains("unchanged"), "{}", bar(&mut c));
    press(&mut c, KeyCode::Char('3'), KeyModifiers::CONTROL);
    let f = feed(&mut c);
    click(&mut c, f);
    assert!(!menu_open(&mut c));
    assert!(!bar(&mut c).contains("unchanged"), "{}", bar(&mut c));
}

/// With a menu open, a click on another channel's row closes the menu and
/// shows that channel, as it would with nothing open.
#[test]
fn with_a_menu_open_a_click_on_a_column_row_closes_it_and_acts() {
    let mut c = chat();
    let infra = row_of(&mut c, "#infra");
    click(&mut c, infra);
    click(&mut c, infra);
    assert!(menu_open(&mut c), "the second click opens #infra's menu");
    let deploy = row_of(&mut c, "#deploy");
    click(&mut c, deploy);
    assert!(!menu_open(&mut c), "the menu closed");
    assert_eq!(c.foreground(), &Tab::Channel("deploy".into()), "and the click showed #deploy");
}

/// The same on the tab bar.
#[test]
fn with_a_menu_open_a_click_on_a_tab_closes_it_and_acts() {
    let mut c = chat();
    let infra = row_of(&mut c, "#infra");
    click(&mut c, infra);
    click(&mut c, infra);
    assert!(menu_open(&mut c));
    let tab = at(&mut c, "2 #open");
    click(&mut c, tab);
    assert!(!menu_open(&mut c));
    assert_eq!(c.foreground(), &Tab::Channel("open".into()));
}

/// A right click outside a menu closes it too, and on a tab opens that
/// tab's menu, as with nothing open.
#[test]
fn with_a_menu_open_a_right_click_on_a_tab_opens_that_tabs_menu() {
    let mut c = chat();
    let infra = row_of(&mut c, "#infra");
    click(&mut c, infra);
    click(&mut c, infra);
    let tab = at(&mut c, "3 #deploy");
    button(&mut c, tab, MouseButton::Right);
    assert!(rows(&mut c).iter().any(|r| r.contains("pick one: #deploy")), "{:#?}", rows(&mut c));
}

/// Elsewhere, a click outside a menu only closes it.
#[test]
fn with_a_menu_open_a_click_in_the_feed_only_closes_it() {
    let mut c = chat();
    let infra = row_of(&mut c, "#infra");
    click(&mut c, infra);
    click(&mut c, infra);
    let f = feed(&mut c);
    click(&mut c, f);
    assert!(!menu_open(&mut c));
    assert_eq!(c.foreground(), &Tab::Channel("infra".into()), "nothing more happened");
}

/// With a question waiting, a click on a channel's row answers no and
/// shows the channel: one rule for every overlay.
#[test]
fn with_a_question_waiting_a_click_on_a_row_answers_no_and_acts() {
    let mut c = chat();
    ask_by_clicks(&mut c, "1 merged", "Clear view");
    let infra = row_of(&mut c, "#infra");
    click(&mut c, infra);
    assert_eq!(c.status(), "kept");
    assert_eq!(c.foreground(), &Tab::Channel("infra".into()));
    assert!(!c.signals().is_empty());
}

/// The key help closes on a click outside it, and a click on a tab then
/// shows the tab.
#[test]
fn with_the_key_help_open_a_click_on_a_tab_closes_it_and_acts() {
    let mut c = chat();
    key(&mut c, KeyCode::F(1));
    assert!(rows(&mut c).iter().any(|r| r.contains("any other key closes")));
    let tab = at(&mut c, "3 #deploy");
    click(&mut c, tab);
    assert!(!rows(&mut c).iter().any(|r| r.contains("any other key closes")));
    assert_eq!(c.foreground(), &Tab::Channel("deploy".into()));
}

/// The compose box lent to a new channel's name gives itself back, with
/// the draft it held, on a click outside it, and says nothing; Enter then
/// sends nothing to the name.
#[test]
fn a_click_outside_a_lent_compose_box_gives_it_back() {
    let mut c = chat();
    for ch in "draft".chars() {
        key(&mut c, KeyCode::Char(ch));
    }
    press(&mut c, KeyCode::Char('n'), KeyModifiers::CONTROL);
    assert!(bar(&mut c).contains("new channel"), "{}", bar(&mut c));
    for ch in "abc".chars() {
        key(&mut c, KeyCode::Char(ch));
    }
    let f = feed(&mut c);
    click(&mut c, f);
    assert!(!bar(&mut c).contains("new channel"), "the box is back: {}", bar(&mut c));
    assert_eq!(c.input().text(), "draft", "with its draft");
    assert!(!bar(&mut c).contains("cancelled"), "closing says nothing: {}", bar(&mut c));
}

/// The same for a click on the tab bar, which then acts.
#[test]
fn a_click_on_a_tab_gives_a_lent_compose_box_back_and_shows_the_tab() {
    let mut c = chat();
    press(&mut c, KeyCode::Char('n'), KeyModifiers::CONTROL);
    key(&mut c, KeyCode::Char('x'));
    let tab = at(&mut c, "3 #deploy");
    click(&mut c, tab);
    assert!(!bar(&mut c).contains("new channel"), "{}", bar(&mut c));
    assert_eq!(c.foreground(), &Tab::Channel("deploy".into()));
    assert_eq!(c.input().text(), "");
}

/// Esc gives a lent box back saying nothing, as it closes a menu.
#[test]
fn esc_gives_a_lent_compose_box_back_saying_nothing() {
    let mut c = chat();
    press(&mut c, KeyCode::Char('n'), KeyModifiers::CONTROL);
    key(&mut c, KeyCode::Esc);
    assert!(!bar(&mut c).contains("new channel") && !bar(&mut c).contains("cancelled"), "{}", bar(&mut c));
}

/// Where Ctrl+3 arrives as Esc, Esc on an empty box asks before quitting;
/// a click on the yes answer quits, as `y` does.
#[test]
fn a_click_on_quit_ends_the_session_as_y_does() {
    let mut c = chat().enhanced(false);
    key(&mut c, KeyCode::Esc);
    assert!(bar(&mut c).contains("quit the chat?"), "{}", bar(&mut c));
    let yes = at(&mut c, " y quit ");
    click(&mut c, yes);
    assert!(c.app().ended(), "the click quit");

    let mut c = chat().enhanced(false);
    key(&mut c, KeyCode::Esc);
    assert!(!key(&mut c, KeyCode::Char('y')), "y quits");

    let mut c = chat().enhanced(false);
    key(&mut c, KeyCode::Esc);
    let no = at(&mut c, " n stay ");
    click(&mut c, no);
    assert!(!c.app().ended());
    assert_eq!(c.status(), "stayed");
}

/// A paste over a question answers it no, then lands in the compose box:
/// the `y` in it never answers.
#[test]
fn a_paste_over_a_question_answers_no_then_pastes() {
    let mut c = chat();
    ask_by_clicks(&mut c, "1 merged", "Clear view");
    testkit::render_screen(&mut c, SIZE.0, SIZE.1);
    agent_tui::screen::Screen::paste(&mut c, "yes please");
    assert_eq!(c.status(), "kept");
    assert_eq!(c.input().text(), "yes please");
    assert!(!c.signals().is_empty());
}
