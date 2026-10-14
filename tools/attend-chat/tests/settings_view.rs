//! `≡` → Settings ▸ through the shell's real mouse and key handlers: the
//! settings listed with their values and layers, the place a mod key is
//! written to chosen (the user, the chat's project, a live agent's, or one
//! typed), the value picked, and each write reaching the right file as
//! `ways settings set` writes it. Its own test binary and one test, since
//! the steps share the files and the working directory.

mod support;

use std::path::Path;

use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;
use attend_chat::signal::{Channel, Signal};

const AGENT: &str = "aaaaaaaa-0000-4000-8000-000000000007";
const SIZE: (u16, u16) = (120, 35);

fn rows(c: &mut Chat) -> Vec<String> {
    testkit::rows(&testkit::render_screen(c, SIZE.0, SIZE.1))
}

fn shown(c: &mut Chat) -> String {
    rows(c).join("\n")
}

fn click_on(c: &mut Chat, text: &str) {
    let buf = testkit::render_screen(c, SIZE.0, SIZE.1);
    let (x, y) = testkit::find(&buf, text).unwrap_or_else(|| panic!("no {text:?} on the screen: {:#?}", testkit::rows(&buf)));
    c.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE });
}

fn keys(c: &mut Chat, keys: &[KeyCode]) {
    testkit::render_screen(c, SIZE.0, SIZE.1);
    testkit::drive(c, &keys.iter().map(|k| KeyEvent::new(*k, KeyModifiers::NONE)).collect::<Vec<_>>());
}

fn typed(c: &mut Chat, text: &str) {
    keys(c, &text.chars().map(KeyCode::Char).collect::<Vec<_>>());
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// The rows on the screen that hold `text`.
fn lines_with(c: &mut Chat, text: &str) -> Vec<String> {
    rows(c).into_iter().filter(|r| r.contains(text)).collect()
}

/// Open Settings ▸ from `≡`.
fn open_settings(c: &mut Chat) {
    click_on(c, "≡");
    click_on(c, "Settings ▸");
}

#[test]
fn settings_list_choose_a_place_and_write_to_its_file() {
    let root = support::fixture("settings-view", |_| support::pin_alive(AGENT));
    let home = root.join("home");
    let proj = home.join("work/proj");
    let other = home.join("work/other");
    std::fs::create_dir_all(proj.join(".git")).unwrap();
    std::fs::create_dir_all(proj.join("src")).unwrap();
    std::fs::create_dir_all(other.join(".git")).unwrap();
    std::fs::create_dir_all(other.join("deep")).unwrap();
    std::env::set_current_dir(proj.join("src")).unwrap();

    let mut c = Chat::new(None, Palette::default(), Shape::PLAIN).sidebar(true).heartbeat(false).enhanced(true);
    // A live agent working in the other project.
    c.push(Signal {
        id: "s-1".into(),
        from: format!("claude:{AGENT}"),
        project: "other".into(),
        cwd: other.display().to_string(),
        reply_to: None,
        message: "working in other".into(),
        ts: 1_790_949_000,
        channel: Channel::Open,
        tray: String::new(),
        beside_post: false,
    });

    // The list: every setting with its value and the layer it comes from.
    open_settings(&mut c);
    let s = shown(&mut c);
    for line in [
        "attend_mod.listening = dormant (default)",
        "attend_mod.keepwarm = on (default)",
        "attend.chat.tabs.jump = auto (default)",
        "attend.chat.mouse = true (default)",
        "attend.chat.sidebar_key = f3 (default)",
    ] {
        assert!(s.contains(line), "{line}: {s}");
    }

    // A mod key asks where to write it: the user, the chat's project, the
    // live agent's project, or another.
    click_on(&mut c, "attend_mod.listening =");
    let s = shown(&mut c);
    for place in ["user: not set", "project ~/work/proj: not set", "project ~/work/other: not set", "Another project…"] {
        assert!(s.contains(place), "{place}: {s}");
    }
    click_on(&mut c, "project ~/work/proj:");
    let marked = lines_with(&mut c, "● ");
    let marked: Vec<String> = marked.into_iter().filter(|r| r.contains("current")).collect();
    assert!(marked.len() == 1 && marked[0].contains("dormant"), "the value in effect is marked: {marked:#?}");
    click_on(&mut c, "active");
    let file = proj.join(".claude/ways.yaml");
    assert_eq!(read(&file), format!("{}attend_mod:\n  listening: active\n", ways_core::settings::PROJECT_HEADER));
    assert!(!ways_core::paths::user_config().exists(), "the user file is not touched");
    assert!(c.status() == "attend_mod.listening = active · proj/.claude/ways.yaml", "{}", c.status());
    open_settings(&mut c);
    assert!(shown(&mut c).contains("attend_mod.listening = active (project)"), "the new value, from the project: {}", shown(&mut c));

    // To the user file.
    click_on(&mut c, "attend_mod.keepwarm =");
    click_on(&mut c, "user: not set");
    click_on(&mut c, "off");
    assert_eq!(read(&ways_core::paths::user_config()), "attend_mod:\n  keepwarm: off\n");
    open_settings(&mut c);
    assert!(shown(&mut c).contains("attend_mod.keepwarm = off (user)"), "{}", shown(&mut c));

    // To the live agent's project, which the chat does not run in.
    click_on(&mut c, "attend_mod.listening =");
    click_on(&mut c, "project ~/work/other:");
    click_on(&mut c, "active");
    assert!(read(&other.join(".claude/ways.yaml")).contains("listening: active"), "{}", read(&other.join(".claude/ways.yaml")));
    open_settings(&mut c);

    // A chat key is the user file's alone: no place to choose, and it
    // applies at once.
    click_on(&mut c, "attend.chat.tabs.jump =");
    assert!(!shown(&mut c).contains("user: not set"), "no layer picker: {}", shown(&mut c));
    click_on(&mut c, " alt ");
    assert!(read(&attend_config::user_path()).contains("jump: alt"), "{}", read(&attend_config::user_path()));
    assert!(c.status().starts_with("attend.chat.tabs.jump = alt · "), "{}", c.status());
    keys(&mut c, &[KeyCode::Esc]);
    assert!(rows(&mut c).last().unwrap().contains("Alt+0-9"), "applied at once: {:?}", rows(&mut c).last());
    open_settings(&mut c);
    assert!(shown(&mut c).contains("attend.chat.tabs.jump = alt (user)"), "{}", shown(&mut c));

    // Unset here removes the key from the place's file.
    click_on(&mut c, "attend_mod.listening =");
    click_on(&mut c, "project ~/work/proj: active");
    click_on(&mut c, "Unset here");
    assert!(!read(&file).contains("listening"), "{}", read(&file));
    assert!(c.status().contains("dormant (default) in effect"), "{}", c.status());
    open_settings(&mut c);

    // Another project, typed: a directory no project encloses is refused
    // inside the dialog; one a project encloses goes on to its values.
    click_on(&mut c, "attend_mod.keepwarm =");
    click_on(&mut c, "Another project…");
    typed(&mut c, &home.join("loose").display().to_string());
    keys(&mut c, &[KeyCode::Enter]);
    assert!(shown(&mut c).contains("no project encloses that directory"), "{}", shown(&mut c));
    keys(&mut c, &[KeyCode::Backspace; 200]);
    typed(&mut c, &other.join("deep").display().to_string());
    keys(&mut c, &[KeyCode::Enter]);
    assert!(shown(&mut c).contains("attend_mod.keepwarm · project other"), "{}", shown(&mut c));
    keys(&mut c, &[KeyCode::Up, KeyCode::Enter]);
    assert!(read(&other.join(".claude/ways.yaml")).contains("keepwarm: on"), "{}", read(&other.join(".claude/ways.yaml")));
}
