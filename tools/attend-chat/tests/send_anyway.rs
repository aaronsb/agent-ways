//! A send refused because its agent is quiet can be sent anyway, as Claude
//! Code's `chat:sendNow` sends a message at once: Ctrl+Enter where the
//! terminal reports it, and Enter pressed again on the unchanged draft
//! everywhere. An edit to the draft cancels the offer. An agent whose Claude
//! Code process has exited is refused whatever the key. Through the real keys.
//! In its own file, so its own fixture.

mod support;

use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;
use attend_chat::signal::{self, Channel, Signal};
use std::time::{Duration, SystemTime};

/// Quiet: a stale heartbeat, and nothing on this host says its process
/// exited (it may run on another host, or not listen yet).
const QUIET_SID: &str = "ffffffff-0000-4000-8000-000000000006";
const QUIET_CWD: &str = "/work/zeta";
/// A second quiet agent, for a send-now offer that must not follow the
/// draft to it.
const QUIET2_SID: &str = "dddddddd-0000-4000-8000-000000000007";
const QUIET2_CWD: &str = "/work/delta";
/// On another host: its enrollment names a process key of another host,
/// which this host cannot judge, so it reads as quiet.
const ABROAD_SID: &str = "aaaaaaaa-0000-4000-8000-000000000008";
const ABROAD_CWD: &str = "/work/theta";
/// Gone: its enrollment names a Claude Code process that has exited.
const GONE_SID: &str = "eeeeeeee-0000-4000-8000-000000000005";
const GONE_CWD: &str = "/work/epsilon";
const W: u16 = 120;
const H: u16 = 35;

fn lapse(session: &str) {
    attend_presence::heartbeat::touch(session).unwrap();
    let file = std::fs::File::options().write(true).open(attend_presence::heartbeat::heartbeat_path(session)).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(600)).unwrap();
}

fn gone_pid() -> u32 {
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    pid
}

fn chat(enhanced: bool) -> Chat {
    support::fixture("send-anyway", |_| {
        std::env::remove_var("CLAUDE_CONFIG_DIR");
        let enrolled = attend_presence::cache::dir().join("enrolled");
        std::fs::create_dir_all(&enrolled).unwrap();
        std::fs::write(enrolled.join(GONE_SID), format!("listen\nclaude {}-1@{}\n", gone_pid(), attend_presence::process::host_name())).unwrap();
        lapse(QUIET_SID);
        lapse(QUIET2_SID);
        std::fs::write(enrolled.join(ABROAD_SID), format!("listen\nclaude {}-1@another-host\n", gone_pid())).unwrap();
        lapse(ABROAD_SID);
        lapse(GONE_SID);
    });
    let mut c = Chat::new(None, Palette::default(), Shape::PLAIN).sidebar(true).heartbeat(false).enhanced(enhanced);
    c.push(sig("s-1", QUIET_SID, QUIET_CWD));
    c.push(sig("s-2", GONE_SID, GONE_CWD));
    c.push(sig("s-3", QUIET2_SID, QUIET2_CWD));
    c.push(sig("s-4", ABROAD_SID, ABROAD_CWD));
    c
}

fn sig(id: &str, sid: &str, cwd: &str) -> Signal {
    Signal {
        id: id.into(),
        from: format!("claude:{sid}"),
        project: cwd.rsplit('/').next().unwrap().into(),
        cwd: cwd.into(),
        reply_to: None,
        message: "a message".into(),
        ts: 1_790_949_000,
        channel: Channel::Open,
        tray: String::new(),
        beside_post: false,
    }
}

fn nick(cwd: &str) -> String {
    agent_identity::Identity::for_cwd(cwd, agent_theme::ColorDepth::NoColor).nickname.to_string()
}

fn key(c: &mut Chat, code: KeyCode, modifiers: KeyModifiers) {
    testkit::render_screen(c, W, H);
    testkit::drive(c, &[KeyEvent::new(code, modifiers)]);
}

fn press(c: &mut Chat, code: KeyCode) {
    key(c, code, KeyModifiers::NONE);
}

fn typed(c: &mut Chat, s: &str) {
    for ch in s.chars() {
        press(c, KeyCode::Char(ch));
    }
}

fn side_click(c: &mut Chat, text: &str) {
    let rows = testkit::rows(&testkit::render_screen(c, W, H));
    let (x, y) = rows
        .iter()
        .enumerate()
        .skip(1)
        .find_map(|(y, r)| {
            let col = r.split('│').next()?;
            col.find(text).map(|b| (col[..b].chars().count() as u16, y as u16))
        })
        .unwrap_or_else(|| panic!("no {text} in the column: {rows:#?}"));
    c.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE });
}

/// A left click on the compose box's line that shows `text`.
fn compose_click(c: &mut Chat, text: &str) {
    let buf = testkit::render_screen(c, W, H);
    let (x, y) = testkit::find(&buf, text).unwrap_or_else(|| panic!("no {text}: {}", testkit::rows(&buf).join("\n")));
    c.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x + 2, row: y, modifiers: KeyModifiers::NONE });
}

fn in_tray(cwd: &str, body: &str) -> bool {
    std::fs::read_dir(signal::cwd_dir(cwd)).into_iter().flatten().flatten().any(|e| std::fs::read_to_string(e.path()).is_ok_and(|r| r.contains(body)))
}

#[test]
fn enter_again_sends_a_refused_message_to_a_quiet_agent() {
    let mut c = chat(false);
    let quiet = nick(QUIET_CWD);
    typed(&mut c, &format!("@{quiet} first try"));
    press(&mut c, KeyCode::Enter);
    assert_eq!(c.status(), format!("press Enter again to send: @{quiet} is quiet (no heartbeat)"));
    assert!(!in_tray(QUIET_CWD, "first try"), "the first Enter sends nothing");
    press(&mut c, KeyCode::Enter);
    assert!(c.input().is_empty(), "sent: {}", c.status());
    assert!(in_tray(QUIET_CWD, "first try"), "Enter again sent it: {}", c.status());
}

#[test]
fn an_edit_to_the_draft_cancels_the_offer() {
    let mut c = chat(false);
    let quiet = nick(QUIET_CWD);
    typed(&mut c, &format!("@{quiet} edited"));
    press(&mut c, KeyCode::Enter);
    typed(&mut c, "x");
    press(&mut c, KeyCode::Backspace);
    press(&mut c, KeyCode::Enter);
    assert!(c.status().ends_with(&format!("@{quiet} is quiet (no heartbeat)")), "refused again: {}", c.status());
    assert!(!in_tray(QUIET_CWD, "edited"), "an edit between the Enters cancels the send");
    press(&mut c, KeyCode::Enter);
    assert!(in_tray(QUIET_CWD, "edited"), "the offer stands again after the new refusal: {}", c.status());
}

#[test]
fn ctrl_enter_sends_now_where_the_terminal_reports_it() {
    let mut c = chat(true);
    let quiet = nick(QUIET_CWD);
    typed(&mut c, &format!("@{quiet} refused once"));
    press(&mut c, KeyCode::Enter);
    assert_eq!(c.status(), format!("Ctrl+Enter or Enter again to send now: @{quiet} is quiet (no heartbeat)"));
    key(&mut c, KeyCode::Enter, KeyModifiers::CONTROL);
    assert!(in_tray(QUIET_CWD, "refused once"), "Ctrl+Enter sent it: {}", c.status());
    typed(&mut c, &format!("@{quiet} straight away"));
    key(&mut c, KeyCode::Enter, KeyModifiers::CONTROL);
    assert!(in_tray(QUIET_CWD, "straight away"), "Ctrl+Enter sends now without a refusal first: {}", c.status());
}

/// Ctrl+X Ctrl+S in the box sends now, as Claude Code's chord does, in a
/// terminal that does not report Ctrl+Enter too. The chord's first key
/// followed by any other key drops it, and that key does what it does.
#[test]
fn ctrl_x_ctrl_s_sends_now_in_any_terminal() {
    let mut c = chat(false);
    let quiet = nick(QUIET_CWD);
    typed(&mut c, &format!("@{quiet} chorded"));
    key(&mut c, KeyCode::Char('x'), KeyModifiers::CONTROL);
    assert_eq!(c.input().text(), format!("@{quiet} chorded"), "the first key edits nothing");
    key(&mut c, KeyCode::Char('s'), KeyModifiers::CONTROL);
    assert!(c.input().is_empty() && in_tray(QUIET_CWD, "chorded"), "sent now, no refusal first: {}", c.status());
    typed(&mut c, &format!("@{quiet} broken chord"));
    key(&mut c, KeyCode::Char('x'), KeyModifiers::CONTROL);
    typed(&mut c, "!");
    key(&mut c, KeyCode::Char('s'), KeyModifiers::CONTROL);
    assert_eq!(c.input().text(), format!("@{quiet} broken chord!"), "a key between drops the chord and is typed");
    assert!(!in_tray(QUIET_CWD, "broken chord"), "Ctrl+S alone sends nothing");
}

/// The chord's first key is dropped by any key the operator presses
/// between, those the shell or the column key take included: F1 and the
/// key that closes the help, or the column hidden and shown.
#[test]
fn a_key_the_shell_takes_drops_the_chord() {
    let mut c = chat(false);
    let quiet = nick(QUIET_CWD);
    typed(&mut c, &format!("@{quiet} not yet"));
    for between in [vec![KeyCode::F(1), KeyCode::Esc], vec![KeyCode::F(3), KeyCode::F(3)]] {
        key(&mut c, KeyCode::Char('x'), KeyModifiers::CONTROL);
        for code in between.clone() {
            press(&mut c, code);
        }
        key(&mut c, KeyCode::Char('s'), KeyModifiers::CONTROL);
        assert!(!in_tray(QUIET_CWD, "not yet"), "{between:?} between: Ctrl+S alone sends nothing");
        assert_eq!(c.input().text(), format!("@{quiet} not yet"), "{between:?}: the draft stays");
    }
}

#[test]
fn enter_again_sends_a_refused_dm() {
    let mut c = chat(false);
    side_click(&mut c, &nick(QUIET_CWD));
    typed(&mut c, "dm to a quiet agent");
    press(&mut c, KeyCode::Enter);
    assert!(c.status().starts_with("press Enter again to send: @"), "{}", c.status());
    press(&mut c, KeyCode::Enter);
    assert!(in_tray(QUIET_CWD, "dm to a quiet agent"), "{}", c.status());
}

#[test]
fn an_agent_whose_process_exited_is_refused_whatever_the_key() {
    let mut c = chat(true);
    let gone = nick(GONE_CWD);
    typed(&mut c, &format!("@{gone} hello"));
    press(&mut c, KeyCode::Enter);
    assert_eq!(c.status(), format!("@{gone} has exited and would not read this"));
    press(&mut c, KeyCode::Enter);
    key(&mut c, KeyCode::Enter, KeyModifiers::CONTROL);
    assert_eq!(c.status(), format!("@{gone} has exited and would not read this"));
    assert!(!in_tray(GONE_CWD, "hello"), "nothing was written");
}

// Review of #998, finding 1: the offer is for the draft and the place it
// was refused, so another agent's DM view gets a fresh check.
#[test]
fn the_offer_does_not_follow_the_draft_to_another_dm_view() {
    let mut c = chat(false);
    side_click(&mut c, &nick(QUIET_CWD));
    typed(&mut c, "hello for one agent");
    press(&mut c, KeyCode::Enter);
    assert!(c.status().starts_with("press Enter again to send: @"), "{}", c.status());
    side_click(&mut c, &nick(QUIET2_CWD));
    assert_eq!(c.input().text(), "hello for one agent", "the draft is kept across the switch");
    compose_click(&mut c, "> hello for one agent");
    press(&mut c, KeyCode::Enter);
    assert!(!in_tray(QUIET2_CWD, "hello for one agent"), "the first Enter in the other view sends nothing: {}", c.status());
    assert!(c.status().ends_with(&format!("@{} is quiet (no heartbeat)", nick(QUIET2_CWD))), "a fresh check refused it: {}", c.status());
}

// Review of #998, finding 2: a cache shared with another host holds that
// host's process keys; one whose pid is free here is no sign the session
// exited, so it is quiet and the offer stands.
#[test]
fn an_agent_enrolled_on_another_host_is_quiet_not_exited() {
    let mut c = chat(false);
    let abroad = nick(ABROAD_CWD);
    typed(&mut c, &format!("@{abroad} across hosts"));
    press(&mut c, KeyCode::Enter);
    assert_eq!(c.status(), format!("press Enter again to send: @{abroad} is quiet (no heartbeat)"));
    press(&mut c, KeyCode::Enter);
    assert!(in_tray(ABROAD_CWD, "across hosts"), "{}", c.status());
}
