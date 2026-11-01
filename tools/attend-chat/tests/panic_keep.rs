//! A panic while the chat runs keeps what waits and the draft, as the
//! unasked end does: the hook `main` installs writes what the last tick
//! noted. The panic happens in a child of this test binary, so the hook
//! runs as it does in the chat, and the parent reads the unsent directory.

mod support;

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, UNIX_EPOCH};

use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use agent_tui::screen::Screen;
use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;

fn key(c: &mut Chat, code: KeyCode) {
    testkit::render_screen(c, 120, 35);
    testkit::drive(c, &[KeyEvent::new(code, KeyModifiers::NONE)]);
}

fn typed(c: &mut Chat, s: &str) {
    for ch in s.chars() {
        key(c, KeyCode::Char(ch));
    }
}

#[test]
#[ignore = "run by a_panic_keeps_what_waits_and_the_draft"]
fn child_panics_with_a_message_held_and_a_draft() {
    let Some(dir) = std::env::var_os("ATTEND_CHAT_PANIC_DIR").map(PathBuf::from) else { return };
    support::fixture("panic", |_| {});
    let mut c = Chat::new(None, Palette::default(), Shape::PLAIN)
        .heartbeat(false)
        .pinned(UNIX_EPOCH + Duration::from_secs(1_790_949_600), 0)
        .send_delay(Duration::from_secs(5))
        .unsent_dir(&dir);
    typed(&mut c, "held as the chat panicked");
    key(&mut c, KeyCode::Enter);
    typed(&mut c, "a draft as the chat panicked");
    Screen::tick(&mut c);
    attend_chat::app::keep_on_panic();
    panic!("a panic in the chat");
}

#[test]
fn a_panic_keeps_what_waits_and_the_draft() {
    let dir = std::env::temp_dir().join(format!("attend-chat-panic-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["child_panics_with_a_message_held_and_a_draft", "--exact", "--ignored", "--test-threads=1"])
        .env("ATTEND_CHAT_PANIC_DIR", &dir)
        .output()
        .unwrap();
    assert!(!out.status.success(), "the child panicked");
    let kept: Vec<String> = std::fs::read_dir(&dir).into_iter().flatten().flatten().filter_map(|e| std::fs::read_to_string(e.path()).ok()).collect();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(kept.iter().any(|t| t.contains("kind: held") && t.ends_with("held as the chat panicked")), "the held message kept: {kept:?}");
    assert!(kept.iter().any(|t| t.contains("kind: draft") && t.ends_with("a draft as the chat panicked")), "the draft kept: {kept:?}");
}

#[test]
#[ignore = "run by a_panic_right_after_a_send_keeps_nothing_sent"]
fn child_panics_right_after_an_immediate_send() {
    let Some(dir) = std::env::var_os("ATTEND_CHAT_PANIC_DIR").map(PathBuf::from) else { return };
    support::fixture("panic", |_| {});
    let mut c = Chat::new(None, Palette::default(), Shape::PLAIN)
        .heartbeat(false)
        .pinned(UNIX_EPOCH + Duration::from_secs(1_790_949_600), 0)
        .send_delay(Duration::ZERO)
        .unsent_dir(&dir);
    attend_chat::app::keep_on_panic();
    typed(&mut c, "a draft typed before");
    // The tick notes the draft in the box, as every tick does.
    Screen::tick(&mut c);
    for _ in 0.."a draft typed before".len() {
        key(&mut c, KeyCode::Backspace);
    }
    typed(&mut c, "sent at once");
    Screen::tick(&mut c);
    // Enter with no delay sends it, and nothing ticks before the panic.
    testkit::drive(&mut c, &[KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)]);
    assert!(c.input().text().is_empty(), "the send took the text: {:?}", c.status());
    panic!("a panic right after the send");
}

/// A panic right after a send never keeps the text that went (#1240 item
/// 2): what is noted follows the send, not the last tick.
#[test]
fn a_panic_right_after_a_send_keeps_nothing_sent() {
    let dir = std::env::temp_dir().join(format!("attend-chat-panic-sent-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["child_panics_right_after_an_immediate_send", "--exact", "--ignored", "--test-threads=1"])
        .env("ATTEND_CHAT_PANIC_DIR", &dir)
        .output()
        .unwrap();
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(said.contains("a panic right after the send"), "the child reached its panic: {said}");
    let kept: Vec<String> = std::fs::read_dir(&dir).into_iter().flatten().flatten().filter_map(|e| std::fs::read_to_string(e.path()).ok()).collect();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!kept.iter().any(|t| t.contains("sent at once")), "the sent text was kept again: {kept:?}");
    assert!(kept.is_empty(), "nothing was left to keep: {kept:?}");
}
