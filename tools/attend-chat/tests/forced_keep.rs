//! A forced exit keeps what the chat had noted, and nothing it had sent or
//! kept already (#1240 items 1 and 2). agent-tui's watch thread makes the
//! exit when the run loop never takes a signal up, as when crossterm's read
//! spins on a terminal that hung up; each case runs in a child of this test
//! binary whose main thread spins in its place, and the parent reads the
//! unsent directory.

#![cfg(unix)]

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, UNIX_EPOCH};

use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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

fn chat(dir: &Path) -> Chat {
    support::fixture("forced", |_| {});
    Chat::new(None, Palette::default(), Shape::PLAIN)
        .heartbeat(false)
        .pinned(UNIX_EPOCH + Duration::from_secs(1_790_949_600), 0)
        .send_delay(Duration::from_secs(5))
        .unsent_dir(dir)
}

/// A hangup the loop never takes up, and a loop that never returns.
fn wedge_after_a_hangup() -> ! {
    let _signals = agent_tui::Signals::install().expect("signals");
    attend_chat::app::keep_on_forced_exit();
    // SAFETY: a signal to this process, whose handler is installed.
    unsafe { libc::kill(libc::getpid(), libc::SIGHUP) };
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(20) {
        std::hint::spin_loop();
    }
    std::process::exit(0)
}

#[test]
#[ignore = "run by a_forced_exit_keeps_what_was_noted_and_nothing_sent"]
fn child_forced_out() {
    let (Some(dir), Ok(case)) = (std::env::var_os("ATTEND_CHAT_FORCED_DIR").map(PathBuf::from), std::env::var("ATTEND_CHAT_FORCED_CASE")) else { return };
    let mut c = chat(&dir);
    typed(&mut c, "held as the loop wedged");
    key(&mut c, KeyCode::Enter);
    typed(&mut c, "a draft as the loop wedged");
    testkit::render_screen(&mut c, 120, 35);
    if case == "after-ending" {
        // The pane's ending sends what waits and keeps the draft; a forced
        // exit after it keeps nothing more.
        let _ = c.into_app().end_unasked(libc::SIGHUP, "stopped: the terminal hung up");
    }
    wedge_after_a_hangup()
}

fn run(case: &str) -> (Option<i32>, Vec<String>) {
    let dir = std::env::temp_dir().join(format!("attend-chat-forced-{case}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["child_forced_out", "--exact", "--ignored", "--test-threads=1"])
        .env("ATTEND_CHAT_FORCED_DIR", &dir)
        .env("ATTEND_CHAT_FORCED_CASE", case)
        .output()
        .unwrap();
    let kept: Vec<String> = std::fs::read_dir(&dir).into_iter().flatten().flatten().filter_map(|e| std::fs::read_to_string(e.path()).ok()).collect();
    let _ = std::fs::remove_dir_all(&dir);
    (out.status.code(), kept)
}

#[test]
fn a_forced_exit_keeps_what_was_noted_and_nothing_sent() {
    let (code, kept) = run("wedged");
    assert_eq!(code, Some(128 + libc::SIGHUP), "the watch thread ended the wedged chat");
    assert!(kept.iter().any(|t| t.contains("kind: held") && t.ends_with("held as the loop wedged")), "the held message kept: {kept:?}");
    assert!(kept.iter().any(|t| t.contains("kind: draft") && t.ends_with("a draft as the loop wedged")), "the draft kept: {kept:?}");
    assert_eq!(kept.len(), 2, "each text once: {kept:?}");

    let (code, kept) = run("after-ending");
    assert_eq!(code, Some(128 + libc::SIGHUP));
    assert!(!kept.iter().any(|t| t.contains("held as the loop wedged")), "the ending sent the held message; the forced exit must not keep it: {kept:?}");
    assert_eq!(kept.len(), 1, "the draft the ending kept, once: {kept:?}");
    assert!(kept[0].contains("kind: draft") && kept[0].ends_with("a draft as the loop wedged"), "{kept:?}");
}
