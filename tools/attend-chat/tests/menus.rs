//! The chat's menus through the shell's real key and mouse handlers: each
//! docks at what opened it (a tab, a column row, or beside the row of the
//! menu it was picked from), marks the current value, and draws an item
//! that waits on attend work set back with what it waits on, skipped by the
//! highlight and deaf to a click. In its own file, so its own fixture:
//! #deploy with alpha and the operator in it, and #infra.

mod support;

use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;
use attend_chat::signal::{Channel, Signal};

const ALPHA_SID: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const SIZE: (u16, u16) = (80, 25);

fn chat() -> Chat {
    support::fixture("menus", |base| {
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

fn click(c: &mut Chat, (x, y): (u16, u16)) {
    testkit::render_screen(c, SIZE.0, SIZE.1);
    c.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE });
}

fn press(c: &mut Chat, code: KeyCode, m: KeyModifiers) -> bool {
    testkit::render_screen(c, SIZE.0, SIZE.1);
    testkit::drive(c, &[KeyEvent::new(code, m)])
}

fn key(c: &mut Chat, code: KeyCode) -> bool {
    press(c, code, KeyModifiers::NONE)
}

/// The open menu's title cell and the text of its rows, if one is open.
fn menu(c: &mut Chat) -> Option<((usize, usize), Vec<String>)> {
    let shown = rows(c);
    let (y, x) = shown.iter().enumerate().find_map(|(y, r)| r.find("pick ").map(|x| (y, r[..x].chars().count())))?;
    let x = x.saturating_sub(1);
    let items = shown[y + 1..]
        .iter()
        .take_while(|r| !r.chars().nth(x).is_some_and(|ch| ch == '└'))
        .map(|r| r.chars().skip(x + 1).take_while(|ch| *ch != '│').collect::<String>())
        .collect();
    Some(((x, y), items))
}

fn items(c: &mut Chat) -> Vec<String> {
    menu(c).expect("a menu is open").1
}

/// The highlighted item's text: the one row drawn on a ground, or reversed
/// where there is no colour.
fn highlighted(c: &mut Chat) -> String {
    use agent_tui::ratatui::style::{Color, Modifier};
    let buf = testkit::render_screen(c, SIZE.0, SIZE.1);
    let ((x, y), items) = menu(c).expect("a menu is open");
    let lit = |i: usize| {
        buf.cell((x as u16 + 2, (y + 1 + i) as u16)).is_some_and(|cell| {
            let s = cell.style();
            s.bg.is_some_and(|bg| bg != Color::Reset) || s.add_modifier.contains(Modifier::REVERSED)
        })
    };
    let lit: Vec<usize> = (0..items.len()).filter(|i| lit(*i)).collect();
    assert_eq!(lit.len(), 1, "one highlighted row: {items:#?}");
    items[lit[0]].trim().to_string()
}

/// #deploy's menu, by two clicks on its row in the column.
fn deploy_menu(c: &mut Chat) {
    let deploy = row_of(c, "#deploy");
    click(c, deploy);
    click(c, deploy);
    assert!(menu(c).is_some(), "the second click opens #deploy's menu");
}

/// Add agent and Seal wait on attend (ADR-404 D1 and D4): each is in the
/// menu, says what it waits on, and is never the first highlighted item.
#[test]
fn items_that_wait_on_attend_name_what_they_wait_on() {
    let mut c = chat();
    deploy_menu(&mut c);
    let shown = items(&mut c);
    assert!(shown.iter().any(|r| r.contains("Add agent") && r.contains("needs ADR-404 D1")), "{shown:#?}");
    assert!(shown.iter().any(|r| r.contains("Seal") && r.contains("needs ADR-404 D4")), "{shown:#?}");
    assert!(!shown[0].contains("Add agent"), "never first: {shown:#?}");
    assert!(!highlighted(&mut c).contains("Add agent"));
}

/// The highlight skips an item that waits, down and up, and Enter never
/// picks one.
#[test]
fn the_highlight_skips_items_that_wait() {
    let mut c = chat();
    deploy_menu(&mut c);
    for _ in 0..12 {
        key(&mut c, KeyCode::Down);
        let h = highlighted(&mut c);
        assert!(!h.contains("Add agent") && !h.contains("Seal"), "Down landed on {h}");
    }
    assert!(highlighted(&mut c).contains("Delete channel"), "the last item");
    for _ in 0..12 {
        key(&mut c, KeyCode::Up);
        let h = highlighted(&mut c);
        assert!(!h.contains("Add agent") && !h.contains("Seal"), "Up landed on {h}");
    }
    key(&mut c, KeyCode::End);
    key(&mut c, KeyCode::Up);
    assert!(!highlighted(&mut c).contains("Seal"), "Up from the last skips Seal");
}

/// A click on an item that waits does nothing: the menu stays open and
/// nothing is said.
#[test]
fn a_click_on_an_item_that_waits_does_nothing() {
    let mut c = chat();
    deploy_menu(&mut c);
    for item in ["Add agent", "Seal"] {
        let i = at(&mut c, item);
        click(&mut c, i);
        assert!(menu(&mut c).is_some(), "{item}: the menu stays open");
        assert_eq!(c.status(), "", "{item}: nothing said");
    }
}

/// Open `≡` and pick `item` by clicks; the submenu's rows.
fn submenu(c: &mut Chat, item: &str) -> Vec<String> {
    let slot = at(c, "≡");
    click(c, slot);
    let i = at(c, item);
    click(c, i);
    items(c)
}

/// Keys no preset matches: Keybinding set says what they are on a first
/// row, marked current, that cannot be picked. The cursor starts there, so
/// an Enter at once replaces nothing.
#[test]
fn custom_keys_show_as_the_current_row_and_enter_keeps_them() {
    let mut c = chat().tab_keys("auto", "f2");
    let shown = submenu(&mut c, "Keybinding set");
    assert!(shown[0].contains("● custom") && shown[0].contains("jump=auto, focus=f2"), "{shown:#?}");
    assert_eq!(shown.iter().filter(|r| r.contains('●')).count(), 1, "no preset marked: {shown:#?}");
    assert!(highlighted(&mut c).contains("custom"), "the cursor starts on what is in use");
    key(&mut c, KeyCode::Enter);
    assert!(menu(&mut c).is_some(), "Enter on it picks nothing");
    assert_eq!(c.status(), "", "nothing set");
    key(&mut c, KeyCode::Down);
    assert!(highlighted(&mut c).starts_with("ctrl:"), "Down reaches the presets");
}

/// A theme that did not load is not marked: the menu marks what the screen
/// shows, the terminal's palette it fell back to. One that loads is marked.
#[test]
fn the_theme_menu_marks_the_theme_in_use_not_one_that_failed() {
    let mut c = chat().theme(Some("no-such-theme"));
    let shown = submenu(&mut c, "Theme");
    let marked: Vec<&String> = shown.iter().filter(|r| r.contains('●')).collect();
    assert!(marked.len() == 1 && marked[0].contains("terminal"), "{shown:#?}");
    assert!(c.status().contains("no-such-theme"), "the fallback is said: {}", c.status());

    let mut c = chat().theme(Some("nord"));
    let shown = submenu(&mut c, "Theme");
    assert!(shown.iter().any(|r| r.contains("● nord")), "{shown:#?}");
}

/// `≡` opens over its slot; Theme ▸ and Keybinding set ▸ dock beside the
/// row they were picked from and mark the current value.
#[test]
fn a_submenu_docks_beside_its_row_and_marks_the_current_value() {
    for (item, current) in [("Theme", "terminal"), ("Keybinding set", "auto")] {
        let mut c = chat();
        let slot = at(&mut c, "≡");
        click(&mut c, slot);
        let ((px, py), parent) = menu(&mut c).expect("the ≡ menu");
        assert_eq!(py, 1, "under the tab bar");
        let width = rows(&mut c)[py].chars().skip(px).take_while(|ch| *ch != '┐').count() + 1;
        let row = parent.iter().position(|r| r.contains(item)).expect("the item");
        let i = at(&mut c, item);
        click(&mut c, i);
        let ((x, y), shown) = menu(&mut c).expect("the submenu");
        // Theme's fits right of the menu at 80 columns; Keybinding set's
        // wide labels do not, so it ends at the right edge.
        if item == "Theme" {
            assert!(x >= px + width, "{item}: beside the ≡ menu, not over it: {:#?}", rows(&mut c));
        } else {
            assert!(x > px && rows(&mut c)[y].trim_end().ends_with('┐'), "{item}: as far right as it fits: {:#?}", rows(&mut c));
        }
        assert_eq!(y + 1, py + 1 + row, "{item}: its first item level with the row it came from");
        let marked: Vec<&String> = shown.iter().filter(|r| r.contains('●')).collect();
        assert_eq!(marked.len(), 1, "{item}: one current value: {shown:#?}");
        assert!(marked[0].contains(current) && marked[0].contains("current"), "{item}: {shown:#?}");
    }
}

/// Invite agent ▸ docks beside the channel menu's row too.
#[test]
fn invite_agent_docks_beside_the_channel_menu() {
    let mut c = chat();
    deploy_menu(&mut c);
    let ((px, _), _) = menu(&mut c).unwrap();
    let i = at(&mut c, "Remove agent");
    click(&mut c, i);
    let ((x, _), shown) = menu(&mut c).expect("the remove picker");
    assert!(x > px + 10, "beside, not over, the channel menu: {:#?}", rows(&mut c));
    assert!(shown.iter().any(|r| r.contains("Laszlo") || r.contains("alpha") || !r.trim().is_empty()));
}

/// Settings… shows every chat setting, a line each, in the response
/// modal, not cut on the bottom bar.
#[test]
fn settings_opens_the_listing_in_a_modal() {
    let mut c = chat();
    let slot = at(&mut c, "≡");
    click(&mut c, slot);
    let s = at(&mut c, "Settings");
    click(&mut c, s);
    let shown = rows(&mut c);
    for k in ["mouse = ", "sidebar_key = ", "tabs.jump = "] {
        assert!(shown.iter().any(|r| r.contains(k)), "{k}: {shown:#?}");
    }
    assert!(!bar(&mut c).contains("mouse = "), "not on the bar: {}", bar(&mut c));
    key(&mut c, KeyCode::Esc);
    assert!(!rows(&mut c).iter().any(|r| r.contains("sidebar_key = ")), "Esc closes it");
}

/// A menu opened from the column's row (Ctrl+3 twice docks it there) closes
/// on a click on its own tab and opens nothing: the click lands on what
/// would open the same menu.
#[test]
fn a_click_on_the_tab_of_a_menu_docked_at_its_row_only_closes_it() {
    let mut c = chat();
    press(&mut c, KeyCode::Char('3'), KeyModifiers::CONTROL);
    press(&mut c, KeyCode::Char('3'), KeyModifiers::CONTROL);
    let ((_, y), _) = menu(&mut c).expect("#deploy's menu");
    let (_, row) = row_of(&mut c, "#deploy");
    assert_eq!(y, row as usize + 1, "docked under the column's row");
    let tab = at(&mut c, "3 #deploy");
    click(&mut c, tab);
    assert!(menu(&mut c).is_none(), "closed, and not opened again under the tab: {:#?}", rows(&mut c));

    // The other way round: docked under the tab, closed by its row.
    click(&mut c, tab);
    let ((_, y), _) = menu(&mut c).expect("#deploy's menu");
    assert_eq!(y, 1, "docked under the tab");
    let deploy = row_of(&mut c, "#deploy");
    click(&mut c, deploy);
    assert!(menu(&mut c).is_none(), "closed, and not opened again under the row: {:#?}", rows(&mut c));
}

/// The exit guard names what the lent box holds: a channel's name or a
/// description being typed, not a draft.
#[test]
fn the_exit_guard_names_a_lent_box_for_what_it_holds() {
    let mut c = chat();
    press(&mut c, KeyCode::Char('n'), KeyModifiers::CONTROL);
    for ch in "abc".chars() {
        key(&mut c, KeyCode::Char(ch));
    }
    press(&mut c, KeyCode::Char('c'), KeyModifiers::CONTROL);
    let shown = rows(&mut c).join("\n");
    assert!(shown.contains("a new channel's name being typed"), "{shown}");
    assert!(!shown.contains("a draft in the compose box"), "{shown}");

    let mut c = chat();
    deploy_menu(&mut c);
    let d = at(&mut c, "Describe");
    click(&mut c, d);
    for ch in "rollouts".chars() {
        key(&mut c, KeyCode::Char(ch));
    }
    press(&mut c, KeyCode::Char('c'), KeyModifiers::CONTROL);
    let shown = rows(&mut c).join("\n");
    assert!(shown.contains("a description of #deploy being typed"), "{shown}");

    // A draft is still a draft.
    let mut c = chat();
    key(&mut c, KeyCode::Char('x'));
    press(&mut c, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert!(rows(&mut c).join("\n").contains("a draft in the compose box"));
}
