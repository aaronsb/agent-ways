//! `/config`: the chat's settings (`attend.chat.*`) listed, set through
//! attend's writer into its user file, and applied at once. Its own test
//! binary, so the file it writes changes no other test's screen.

use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;

fn chat() -> Chat {
    let root = std::env::temp_dir().join(format!("attend-chat-config-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for d in ["home", "config", "cache", "data"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    std::env::set_var("HOME", root.join("home"));
    std::env::set_var("XDG_CONFIG_HOME", root.join("config"));
    std::env::set_var("XDG_CACHE_HOME", root.join("cache"));
    std::env::set_var("XDG_DATA_HOME", root.join("data"));
    Chat::new(None, Palette::default(), Shape::PLAIN).heartbeat(false).enhanced(true)
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn enter(c: &mut Chat, line: &str) {
    let keys: Vec<KeyEvent> = line.chars().map(|ch| key(KeyCode::Char(ch))).chain([key(KeyCode::Enter)]).collect();
    assert!(testkit::drive(c, &keys));
}

fn bar(c: &mut Chat) -> String {
    testkit::rows(&testkit::render_screen(c, 400, 25)).pop().expect("a bottom bar")
}

/// One test: the steps share the user file.
#[test]
fn config_lists_sets_refuses_and_applies_at_once() {
    let mut c = chat();
    enter(&mut c, "/config");
    assert_eq!(
        c.status(),
        "tabs.jump = auto (default) · tabs.menu_on_repeat = true (default) · tabs.focus_key = both (default) · mouse = false (default) · /config <key> <value> sets one"
    );
    assert!(bar(&mut c).contains("Ctrl+1-9 tab (again: menu)"), "{}", bar(&mut c));

    enter(&mut c, "/config tabs.jump alt");
    assert_eq!(c.status(), "tabs.jump = alt");
    let file = std::fs::read_to_string(attend_config::user_path()).unwrap();
    assert!(file.contains("chat:\n  tabs:\n    jump: alt\n"), "{file}");
    assert!(bar(&mut c).contains("Alt+1-9 tab (again: menu)"), "applied at once: {}", bar(&mut c));
    enter(&mut c, "/config");
    assert!(c.status().starts_with("tabs.jump = alt (user)"), "{}", c.status());

    enter(&mut c, "/config tabs.jump sideways");
    assert!(c.status().contains("sideways"), "{}", c.status());
    assert_eq!(c.input().text(), "/config tabs.jump sideways", "a refused value keeps the line");
    assert!(std::fs::read_to_string(attend_config::user_path()).unwrap().contains("jump: alt"), "nothing written");

    let clear: Vec<KeyEvent> = std::iter::repeat_n(key(KeyCode::Backspace), 40).collect();
    testkit::drive(&mut c, &clear);
    enter(&mut c, "/config mouse true");
    assert_eq!(c.status(), "mouse = true; mouse takes effect on restart");

    // The keys and their values complete as any subcommand does.
    let keys: Vec<KeyEvent> = "/config tabs.f".chars().map(|ch| key(KeyCode::Char(ch))).chain([key(KeyCode::Tab)]).collect();
    testkit::drive(&mut c, &keys);
    assert_eq!(c.input().text(), "/config tabs.focus_key ");
    testkit::drive(&mut c, &[key(KeyCode::Char('f')), key(KeyCode::Tab)]);
    assert_eq!(c.input().text(), "/config tabs.focus_key f2 ");

    // A terminal that does not report Ctrl+digits: asking for them warns,
    // and the bar names the keys that work there.
    let mut c = Chat::new(None, Palette::default(), Shape::PLAIN).heartbeat(false);
    enter(&mut c, "/config tabs.jump ctrl");
    assert_eq!(c.status(), "tabs.jump = ctrl · Ctrl+digits do not arrive in this terminal; F2 reaches the tabs");
    let b = bar(&mut c);
    assert!(!b.contains("Ctrl+1-9") && b.contains("F2 tabs"), "{b}");
    enter(&mut c, "/config tabs.jump both");
    assert!(c.status().ends_with("Alt+1-9 and F2 do"), "{}", c.status());
    assert!(bar(&mut c).contains("Alt+1-9 tab (again: menu)"));

    // The ≡ menu's keybinding set writes its keys in one go.
    for k in [KeyCode::F(2), KeyCode::Left, KeyCode::Enter, KeyCode::Down, KeyCode::Enter, KeyCode::Down, KeyCode::Enter] {
        testkit::drive(&mut c, &[key(k)]);
    }
    assert!(c.status().starts_with("keybinding set: alt"), "{}", c.status());
    let file = std::fs::read_to_string(attend_config::user_path()).unwrap();
    assert!(file.contains("jump: alt") && file.contains("focus_key: both"), "{file}");
}
