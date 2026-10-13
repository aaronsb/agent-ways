//! A send to an agent whose heartbeat lapsed while its Claude Code process
//! runs on and it listens goes through; a send to an agent whose process has
//! exited is refused. A session hosted by the attend mod has no `attend run`
//! touching its heartbeat, so it lapses during a turn longer than the grace,
//! and the chat refused a DM to it (the mod-session liveness defect of
//! 2026-10-13). Through the real Enter. In its own file, so its own fixture:
//! the test process stands in for the live agent's Claude Code process.

mod support;

use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;
use attend_chat::signal::{self, Channel, Signal};
use std::time::{Duration, SystemTime};

const WORKING_SID: &str = "dddddddd-0000-4000-8000-000000000004";
const WORKING_CWD: &str = "/work/delta";
const GONE_SID: &str = "eeeeeeee-0000-4000-8000-000000000005";
const GONE_CWD: &str = "/work/epsilon";
const W: u16 = 120;
const H: u16 = 35;

/// A heartbeat ten minutes old: well past the grace.
fn lapse(session: &str) {
    attend_presence::heartbeat::touch(session).unwrap();
    let file = std::fs::File::options().write(true).open(attend_presence::heartbeat::heartbeat_path(session)).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(600)).unwrap();
}

/// A pid no process holds now: a child that has exited and been reaped.
fn gone_pid() -> u32 {
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    pid
}

/// A Claude Code session record naming `pid` as `session`'s process.
fn session_record(home: &std::path::Path, pid: u32, session: &str, cwd: &str) {
    let dir = home.join(".claude/sessions");
    std::fs::create_dir_all(&dir).unwrap();
    let body = format!(r#"{{"pid":{pid},"sessionId":"{session}","cwd":"{cwd}","startedAt":0}}"#);
    std::fs::write(dir.join(format!("{pid}.json")), body).unwrap();
}

fn chat() -> Chat {
    support::fixture("stale-heartbeat", |_| {
        let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
        std::env::remove_var("CLAUDE_CONFIG_DIR");
        // The working agent listens from this process, which its session
        // record names, so its enrollment records this process as its
        // Claude Code process. The gone agent listened from a process that
        // has since exited.
        session_record(&home, std::process::id(), WORKING_SID, WORKING_CWD);
        attend_presence::enrollment::enroll(WORKING_SID, attend_presence::enrollment::Source::Listen, None).unwrap();
        let gone = gone_pid();
        session_record(&home, gone, GONE_SID, GONE_CWD);
        let enrolled = attend_presence::cache::dir().join("enrolled");
        std::fs::create_dir_all(&enrolled).unwrap();
        std::fs::write(enrolled.join(GONE_SID), format!("listen\nclaude {gone}-1\n")).unwrap();
        lapse(WORKING_SID);
        lapse(GONE_SID);
    });
    let mut c = Chat::new(None, Palette::default(), Shape::PLAIN).sidebar(true).heartbeat(false).enhanced(true);
    c.push(sig("s-1", WORKING_SID, WORKING_CWD));
    c.push(sig("s-2", GONE_SID, GONE_CWD));
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
    }
}

fn nick(cwd: &str) -> String {
    agent_identity::Identity::for_cwd(cwd, agent_theme::ColorDepth::NoColor).nickname.to_string()
}

fn rows(c: &mut Chat) -> Vec<String> {
    testkit::rows(&testkit::render_screen(c, W, H))
}

fn press(c: &mut Chat, code: KeyCode) {
    testkit::render_screen(c, W, H);
    testkit::drive(c, &[KeyEvent::new(code, KeyModifiers::NONE)]);
}

fn typed(c: &mut Chat, s: &str) {
    for ch in s.chars() {
        press(c, KeyCode::Char(ch));
    }
}

/// A left click on the first `text` in the left column, below the tab bar.
fn side_click(c: &mut Chat, text: &str) {
    let rows = rows(c);
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

/// The bodies in `cwd`'s tray.
fn tray(cwd: &str) -> Vec<String> {
    std::fs::read_dir(signal::cwd_dir(cwd)).into_iter().flatten().flatten().filter_map(|e| std::fs::read_to_string(e.path()).ok()).collect()
}

#[test]
fn a_dm_to_an_agent_whose_heartbeat_lapsed_mid_turn_is_sent() {
    let mut c = chat();
    side_click(&mut c, &nick(WORKING_CWD));
    press(&mut c, KeyCode::Char('h'));
    typed(&mut c, "ello mid-turn");
    press(&mut c, KeyCode::Enter);
    assert!(c.input().is_empty(), "sent: {}", c.status());
    assert!(tray(WORKING_CWD).iter().any(|r| r.contains("hello mid-turn")), "in its tray: {}", c.status());
}

#[test]
fn an_at_name_send_to_an_agent_whose_heartbeat_lapsed_mid_turn_is_sent() {
    let mut c = chat();
    typed(&mut c, &format!("@{} at-name mid-turn", nick(WORKING_CWD)));
    press(&mut c, KeyCode::Enter);
    assert!(c.input().is_empty(), "sent: {}", c.status());
    assert!(tray(WORKING_CWD).iter().any(|r| r.contains("at-name mid-turn")), "in its tray: {}", c.status());
}

#[test]
fn a_send_to_an_agent_whose_process_exited_is_refused() {
    let mut c = chat();
    let gone = nick(GONE_CWD);
    typed(&mut c, &format!("@{gone} are you there"));
    press(&mut c, KeyCode::Enter);
    assert!(c.status().starts_with(&format!("@{gone} is quiet")), "{}", c.status());
    assert!(!tray(GONE_CWD).iter().any(|r| r.contains("are you there")), "nothing was written");
}
