//! The tab bar's focus kept by what it is while channels come and go, and
//! Add agent to chat's live path: the pick writes the invitation a live
//! peer reads. In its own file, so its own fixture, which the tests change:
//! a live claude with a sessions record, #deploy and #infra.

mod support;

use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;

const BETA_SID: &str = "bbbbbbbb-0000-4000-8000-000000000002";
const W: u16 = 120;
const H: u16 = 35;

fn chat(dry: bool) -> Chat {
    support::fixture("focus-anchor", |base| {
        support::pin_alive(BETA_SID);
        let home = std::path::PathBuf::from(std::env::var("HOME").unwrap());
        let work = home.join("work/beta");
        std::fs::create_dir_all(&work).unwrap();
        std::fs::create_dir_all(home.join(".claude/sessions")).unwrap();
        let record = format!("{{\"pid\":1,\"sessionId\":\"{BETA_SID}\",\"cwd\":\"{}\"}}\n", work.display());
        std::fs::write(home.join(".claude/sessions/1.json"), record).unwrap();
        let g = attend_groups::Groups::new(base, "");
        g.create("deploy", None).unwrap();
        g.create("infra", None).unwrap();
    });
    Chat::new(None, Palette::default(), Shape::PLAIN).sidebar(true).heartbeat(false).enhanced(true).dry_run(dry)
}

fn rows(c: &mut Chat) -> Vec<String> {
    testkit::rows(&testkit::render_screen(c, W, H))
}

fn press(c: &mut Chat, code: KeyCode) {
    testkit::render_screen(c, W, H);
    testkit::drive(c, &[KeyEvent::new(code, KeyModifiers::NONE)]);
}

fn base() -> std::path::PathBuf {
    attend_chat::signal::signals_base()
}

/// A channel created under the focus leaves it on the same tab, and the
/// channel shown dissolved under it moves it to the tab shown in its place.
/// One test: both change the shared fixture.
#[test]
fn the_focused_tab_is_kept_by_what_it_is() {
    let mut c = chat(true);
    press(&mut c, KeyCode::F(2));
    press(&mut c, KeyCode::Left);
    assert!(rows(&mut c)[0].contains("▸#open"), "{}", rows(&mut c)[0]);
    attend_groups::Groups::new(&base(), "").create("aaa-new", None).unwrap();
    c.refresh();
    let top = rows(&mut c)[0].clone();
    assert!(!top.contains("aaa-new") && top.contains("▸#open"), "the focus stays on #open, and the bar gains no tab: {top}");
    // #infra shown (the column's fourth channel, after #open, #aaa-new and
    // #deploy), the focus on its tab, then #infra dissolves: the focus goes
    // to the tab shown, #open's fallback.
    testkit::render_screen(&mut c, W, H);
    testkit::drive(&mut c, &[KeyEvent::new(KeyCode::Char('4'), KeyModifiers::CONTROL)]);
    for _ in 0..8 {
        if rows(&mut c)[0].contains("▸#infra") {
            break;
        }
        press(&mut c, KeyCode::Left);
    }
    assert!(rows(&mut c)[0].contains("▸#infra"), "{}", rows(&mut c)[0]);
    attend_groups::Groups::new(&base(), "").dissolve("infra").unwrap();
    let _ = std::fs::remove_dir_all(base().join("@infra"));
    c.refresh();
    let top = rows(&mut c)[0].clone();
    assert!(!top.contains("#infra") && top.contains("▸#open"), "the focus falls to the tab shown: {top}");
    press(&mut c, KeyCode::Enter);
    assert!(rows(&mut c).iter().any(|r| r.contains("pick one: #open")), "Enter acts on it: {:#?}", rows(&mut c));
}

/// Add agent to chat, live: the pick writes the invitation into the
/// agent's project tray, as `/invite` does, and says the agent joins itself.
#[test]
fn add_agent_to_chat_invites_the_agent_for_real() {
    let mut c = chat(false);
    let shown = rows(&mut c);
    let (y, r) = shown.iter().enumerate().find(|(_, r)| r.split('│').next().is_some_and(|l| l.contains("beta"))).unwrap_or_else(|| panic!("beta's row: {shown:#?}"));
    let x = r[..r.find("beta").unwrap()].chars().count() as u16;
    let click = |c: &mut Chat, x: u16, y: u16| {
        testkit::render_screen(c, W, H);
        c.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE });
    };
    // The first click shows beta's DMs, the second opens its menu.
    click(&mut c, x, y as u16);
    click(&mut c, x, y as u16);
    let buf = testkit::render_screen(&mut c, W, H);
    let (x, y) = testkit::find(&buf, "Add agent to chat").expect("the item");
    click(&mut c, x, y);
    assert!(rows(&mut c).iter().any(|r| r.contains("pick one: add to channel")), "{:#?}", rows(&mut c));
    press(&mut c, KeyCode::Enter);
    assert!(c.status().starts_with("invited @") && c.status().ends_with("they join themselves"), "{}", c.status());
}
