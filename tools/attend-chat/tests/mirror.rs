//! Tab and Shift+Tab as mirrors through the shell's real keys, in a chat
//! with no agents and a single channel, `#open` (the review of #972). In its
//! own file, so its own fixture: an empty bus.

mod support;

use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;

const W: u16 = 100;
const H: u16 = 25;

fn chat() -> Chat {
    support::fixture("mirror", |_| {});
    Chat::new(None, Palette::default(), Shape::PLAIN).sidebar(true).heartbeat(false).enhanced(true).dry_run(true)
}

fn press(c: &mut Chat, code: KeyCode) {
    testkit::render_screen(c, W, H);
    testkit::drive(c, &[KeyEvent::new(code, KeyModifiers::NONE)]);
}

/// What has the focus: the tab behind the bar's `▸`, the column row behind
/// its `▸`, else the bottom bar's lozenge.
fn focused(c: &mut Chat) -> String {
    let rows = testkit::rows(&testkit::render_screen(c, W, H));
    if let Some(i) = rows[0].find('▸') {
        return format!("tab {}", rows[0][i + '▸'.len_utf8()..].split_whitespace().next().unwrap_or(""));
    }
    for r in &rows[1..rows.len() - 1] {
        if let Some(rest) = r.strip_prefix('▸') {
            return format!("row {}", rest.split('│').next().unwrap().trim());
        }
    }
    format!("mode {}", rows.last().unwrap().split_whitespace().next().unwrap())
}

#[test]
fn tab_and_shift_tab_are_mirrors_with_no_agents_and_one_channel() {
    let mut c = chat();
    press(&mut c, KeyCode::Tab);
    let mut ring = vec![focused(&mut c)];
    for _ in 0..20 {
        press(&mut c, KeyCode::Tab);
        let f = focused(&mut c);
        ring.push(f.clone());
        if f == "mode chat" {
            break;
        }
    }
    let rows: Vec<&String> = ring.iter().filter(|f| f.starts_with("row ")).collect();
    assert_eq!(rows.len(), 1, "one row, #open's, and no agent: {ring:?}");
    assert!(rows[0].contains("#open"), "{ring:?}");
    assert_eq!(ring.len(), 7, "≡, +, #open, ⋯all, the row, the feed, the box: {ring:?}");
    for start in &ring {
        for n in 1..=ring.len() {
            let mut c = chat();
            for _ in 0..20 {
                if &focused(&mut c) == start {
                    break;
                }
                press(&mut c, KeyCode::Tab);
            }
            for _ in 0..n {
                press(&mut c, KeyCode::Tab);
            }
            for _ in 0..n {
                press(&mut c, KeyCode::BackTab);
            }
            assert_eq!(&focused(&mut c), start, "{n} × Tab, then {n} × Shift+Tab: {ring:?}");
        }
    }
}
