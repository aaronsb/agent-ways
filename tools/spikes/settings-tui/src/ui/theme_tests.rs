//! The theme tab, driven through the real key and mouse handlers against a
//! themes directory of its own per test.

use std::path::PathBuf;

use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::style::Color;
use ratatui::Terminal;

use super::themestate::{Focus, Themes};
use super::*;
use crate::themes::{parse, ColorDepth, Rgb};
use crate::tree::Setting;

fn press(c: KeyCode) -> KeyEvent {
    KeyEvent::new(c, KeyModifiers::NONE)
}

fn keys(app: &mut App, ks: &[KeyCode]) {
    for k in ks {
        assert!(app.key(press(*k)));
    }
}

fn type_str(app: &mut App, s: &str) {
    for c in s.chars() {
        assert!(app.key(press(KeyCode::Char(c))));
    }
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

/// An empty themes directory of the test's own.
fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("themes-ui-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Two settings tabs, so the theme tab is `3`.
fn app_in(d: &std::path::Path, depth: ColorDepth) -> App {
    let flag = |n: &str| Node::leaf(n, "", Setting::new(Kind::Bool, "true", "default").default("true"));
    let roots = vec![Node::group("ways", "", vec![flag("alpha"), flag("beta")]).opened(), Node::group("gate", "", vec![flag("gamma")]).opened()];
    App::new("t", roots).themes(Themes::new(Some(d.to_path_buf()), depth))
}

fn draw(app: &mut App, w: u16, h: u16) -> Buffer {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    term.backend().buffer().clone()
}

fn text(buf: &Buffer) -> String {
    let w = buf.area.width as usize;
    buf.content().chunks(w).map(|r| r.iter().map(|c| c.symbol()).collect::<String>()).collect::<Vec<_>>().join("\n")
}

/// The truecolor bg of `name`'s bundled theme.
fn bg_of(name: &str) -> Color {
    let t = crate::themes::BUNDLED.iter().find(|(n, _)| *n == name).map(|(_, s)| parse(s).unwrap()).unwrap();
    Color::Rgb(t.slots.bg.0, t.slots.bg.1, t.slots.bg.2)
}

fn shown(app: &App) -> String {
    app.shown_theme().name.clone()
}

fn mouse(app: &mut App, kind: MouseEventKind, (x, y): (u16, u16)) {
    draw(app, 110, 30);
    app.mouse(MouseEvent { kind, column: x, row: y, modifiers: KeyModifiers::NONE });
}

fn find(app: &mut App, needle: &str) -> (u16, u16) {
    let buf = draw(app, 110, 30);
    for (y, line) in text(&buf).lines().enumerate() {
        if let Some(b) = line.find(needle) {
            return (line[..b].chars().count() as u16, y as u16);
        }
    }
    panic!("{needle} not on screen:\n{}", text(&buf));
}

#[test]
fn the_theme_tab_follows_the_settings_tabs() {
    let d = dir("tab");
    let mut app = app_in(&d, ColorDepth::TrueColor);
    let bar = text(&draw(&mut app, 100, 24)).lines().next().unwrap().to_string();
    assert!(bar.contains("1 ways") && bar.contains("2 gate") && bar.contains("3 theme"), "{bar}");
    keys(&mut app, &[KeyCode::Char('3')]);
    assert!(app.on_theme_tab());
    let f = text(&draw(&mut app, 100, 24));
    assert!(f.contains("agent-ways") && f.contains("nord") && f.contains("paper") && f.contains(&d.display().to_string()[..20]), "{f}");
    keys(&mut app, &[KeyCode::Tab]);
    assert_eq!(app.tab, 0, "Tab wraps from the theme tab to the first");
}

#[test]
fn preview_follows_the_cursor_and_reverts_on_leaving_or_esc() {
    let d = dir("preview");
    let mut app = app_in(&d, ColorDepth::TrueColor);
    keys(&mut app, &[KeyCode::Char('3'), KeyCode::Down]);
    assert_eq!(shown(&app), "nord");
    // Nord fills: the whole frame, tab bar and status line too, is on its bg.
    let buf = draw(&mut app, 100, 24);
    assert_eq!((buf[(99, 0)].bg, buf[(99, 23)].bg), (bg_of("nord"), bg_of("nord")));
    keys(&mut app, &[KeyCode::Down, KeyCode::Down]);
    assert_eq!(shown(&app), "dracula");
    keys(&mut app, &[KeyCode::Char('1')]);
    assert_eq!(shown(&app), "agent-ways", "leaving the tab shows the active theme");
    assert_eq!(draw(&mut app, 100, 24)[(99, 0)].bg, Color::Reset, "agent-ways leaves the terminal's background");
    keys(&mut app, &[KeyCode::Char('3')]);
    assert_eq!(shown(&app), "dracula", "the cursor is kept");
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!((shown(&app), app.themes.cursor), ("agent-ways".to_string(), 0), "Esc returns to the active theme");
    assert!(!d.join("active").exists(), "previewing writes nothing");
    assert!(!app.key(press(KeyCode::Esc)), "a second Esc quits, nothing pending");
}

#[test]
fn enter_makes_a_theme_active_and_writes_the_choice() {
    let d = dir("enter");
    let mut app = app_in(&d, ColorDepth::TrueColor);
    keys(&mut app, &[KeyCode::Char('3'), KeyCode::Down, KeyCode::Enter]);
    assert_eq!(std::fs::read_to_string(d.join("active")).unwrap(), "nord\n");
    assert_eq!(app.pending(), 0, "it is not a pending setting");
    keys(&mut app, &[KeyCode::Char('1')]);
    assert_eq!(shown(&app), "nord");
    assert_eq!(Themes::new(Some(d.clone()), ColorDepth::TrueColor).active, "nord", "read back at startup");
    // A click selects, and a second click on the selected row makes it active.
    keys(&mut app, &[KeyCode::Char('3')]);
    let at = find(&mut app, "dracula");
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), at);
    assert_eq!((shown(&app), app.themes.active.as_str()), ("dracula".to_string(), "nord"));
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), at);
    assert_eq!(std::fs::read_to_string(d.join("active")).unwrap(), "dracula\n");
}

/// Open the menu on the theme under the cursor and pick `label`.
fn act(app: &mut App, label: &str) {
    keys(app, &[KeyCode::Char('a')]);
    let i = app.theme_acts().iter().position(|a| a.label() == label).unwrap_or_else(|| panic!("no {label} in the menu"));
    keys(app, &vec![KeyCode::Down; i]);
    keys(app, &[KeyCode::Enter]);
}

#[test]
fn new_copy_rename_and_delete_write_the_themes_dir() {
    let d = dir("files");
    let mut app = app_in(&d, ColorDepth::TrueColor);
    keys(&mut app, &[KeyCode::Char('3')]);
    assert_eq!(app.theme_acts().len(), 3, "a bundled theme offers new, copy and edit");
    act(&mut app, "new");
    type_str(&mut app, "mine");
    keys(&mut app, &[KeyCode::Enter]);
    let mine = parse(&std::fs::read_to_string(d.join("mine.toml")).unwrap()).unwrap();
    assert_eq!((mine.name.as_str(), mine.slots), ("mine", app.themes.set.get("agent-ways").unwrap().slots));
    assert_eq!(shown(&app), "mine", "the cursor moves to the new theme");

    // Copy nord; a bad and a taken name are refused and the prompt stays.
    keys(&mut app, &[KeyCode::Home, KeyCode::Down]);
    act(&mut app, "copy");
    type_str(&mut app, "Nord 2");
    keys(&mut app, &[KeyCode::Enter]);
    assert!(app.msg.contains("lowercase") && matches!(app.mode, Mode::ThemeName { .. }), "{}", app.msg);
    for _ in 0..6 {
        keys(&mut app, &[KeyCode::Backspace]);
    }
    type_str(&mut app, "mine");
    keys(&mut app, &[KeyCode::Enter]);
    assert!(app.msg.contains("taken"), "{}", app.msg);
    keys(&mut app, &[KeyCode::Backspace, KeyCode::Backspace, KeyCode::Backspace, KeyCode::Backspace]);
    type_str(&mut app, "nord-2");
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(parse(&std::fs::read_to_string(d.join("nord-2.toml")).unwrap()).unwrap().slots, app.themes.set.get("nord").unwrap().slots);

    // Rename the active theme: the file moves and the active choice follows.
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.themes.active, "nord-2");
    assert_eq!(app.theme_acts().len(), 5, "a user theme adds rename and delete");
    act(&mut app, "rename");
    type_str(&mut app, "arctic");
    keys(&mut app, &[KeyCode::Enter]);
    assert!(!d.join("nord-2.toml").exists() && d.join("arctic.toml").exists());
    assert_eq!(std::fs::read_to_string(d.join("active")).unwrap(), "arctic\n");
    assert_eq!((app.themes.active.as_str(), shown(&app).as_str()), ("arctic", "arctic"));

    // Delete the active theme: y/n first, then agent-ways takes over.
    act(&mut app, "delete");
    keys(&mut app, &[KeyCode::Char('n')]);
    assert!(d.join("arctic.toml").exists());
    act(&mut app, "delete");
    keys(&mut app, &[KeyCode::Char('y')]);
    assert!(!d.join("arctic.toml").exists());
    assert_eq!(std::fs::read_to_string(d.join("active")).unwrap(), "agent-ways\n");
    assert_eq!(app.themes.active, "agent-ways");
    assert!(app.themes.set.get("arctic").is_none());
}

#[test]
fn editing_a_bundled_theme_starts_a_copy() {
    let d = dir("editcopy");
    let mut app = app_in(&d, ColorDepth::TrueColor);
    keys(&mut app, &[KeyCode::Char('3'), KeyCode::Down, KeyCode::Char('e')]);
    assert!(matches!(&app.mode, Mode::ThemeName { op: NameOp::EditCopy(f), .. } if f == "nord"));
    type_str(&mut app, "my-nord");
    keys(&mut app, &[KeyCode::Enter]);
    let e = app.themes.editor.as_ref().expect("the editor is open");
    assert_eq!((e.theme.name.as_str(), e.written, e.dirty()), ("my-nord", false, true));
    assert!(app.key(ctrl('s')));
    assert!(d.join("my-nord.toml").exists() && !d.join("nord.toml").exists(), "the bundled theme is never written");
    assert!(!app.theme_dirty());
}

/// The editor open on a user copy of nord, on the `err` row.
fn editing_err(tag: &str) -> (App, PathBuf) {
    let d = dir(tag);
    let mut app = app_in(&d, ColorDepth::TrueColor);
    keys(&mut app, &[KeyCode::Char('3'), KeyCode::Down, KeyCode::Char('e')]);
    type_str(&mut app, "ed");
    keys(&mut app, &[KeyCode::Enter]);
    assert!(app.key(ctrl('s')));
    let err = Slots::NAMES.iter().position(|s| *s == "err").unwrap();
    keys(&mut app, &vec![KeyCode::Down; err]);
    assert_eq!(app.themes.editor.as_ref().unwrap().slot(), Some("err"));
    (app, d)
}

use crate::themes::Slots;

fn err_slot(app: &App) -> Rgb {
    app.themes.editor.as_ref().unwrap().theme.slots.err
}

#[test]
fn slider_keys_change_the_slot_and_the_preview() {
    let (mut app, _d) = editing_err("slider");
    let before = err_slot(&app);
    let h0 = before.to_hsl()[0];
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.themes.editor.as_ref().unwrap().focus, Focus::Slider(0));
    keys(&mut app, &[KeyCode::Right]);
    assert!((err_slot(&app).to_hsl()[0] - (h0.round() + 1.0)).abs() < 1.5, "→ steps the hue by 1°");
    app.key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
    let h = app.themes.editor.as_ref().unwrap().hsl()[0];
    assert_eq!(h, (h0.round() + 16.0).rem_euclid(360.0), "⇧→ steps by 15°");
    // Lightness down by 30: the err role the frame draws with follows.
    let role = |app: &App| app.themes.palette(app.shown_theme()).roles.err;
    let lifted = role(&app);
    keys(&mut app, &[KeyCode::Down, KeyCode::Down]);
    for _ in 0..3 {
        app.key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));
    }
    assert_ne!(err_slot(&app), before);
    assert_ne!(role(&app), lifted, "the preview is derived from the edited slot");
    let buf = draw(&mut app, 100, 30);
    assert!(text(&buf).contains("▸L"), "the focused slider is marked");
    assert!(app.theme_dirty());
    let bar = text(&buf).lines().next().unwrap().to_string();
    assert!(bar.contains("3 theme") && bar.contains("●1"), "unsaved edits badge the theme tab: {bar}");
}

#[test]
fn mouse_sets_drags_and_wheels_a_slider() {
    let (mut app, _d) = editing_err("mouse");
    draw(&mut app, 110, 30);
    let (track, ch) = app.hits.sliders[2];
    assert_eq!(ch, 2);
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), (track.x, track.y));
    assert_eq!(app.themes.editor.as_ref().unwrap().hsl()[2], 0.0, "a click at the left end is 0");
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), (track.x + track.width + 5, track.y + 3));
    assert_eq!(app.themes.editor.as_ref().unwrap().hsl()[2], 100.0, "a drag keeps hold past the end");
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), (0, 0));
    mouse(&mut app, MouseEventKind::ScrollUp, (track.x + 2, track.y));
    assert_eq!(app.themes.editor.as_ref().unwrap().hsl()[2], 99.0, "the wheel steps it");
}

#[test]
fn hex_entry_takes_only_rrggbb() {
    let (mut app, _d) = editing_err("hex");
    keys(&mut app, &[KeyCode::Char('#')]);
    type_str(&mut app, "zz");
    assert!(app.msg.contains("not a hex digit"));
    type_str(&mut app, "abc");
    keys(&mut app, &[KeyCode::Enter]);
    assert!(app.msg.contains("not #rrggbb") && app.themes.editor.as_ref().unwrap().hex.is_some(), "{}", app.msg);
    type_str(&mut app, "def0123");
    assert_eq!(app.themes.editor.as_ref().unwrap().hex.as_deref(), Some("abcdef"), "six digits at most");
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(err_slot(&app), Rgb(0xab, 0xcd, 0xef));
    keys(&mut app, &[KeyCode::Char('e')]);
    type_str(&mut app, "#ff0000");
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!(err_slot(&app), Rgb(0xab, 0xcd, 0xef), "Esc drops the entry");
}

#[test]
fn save_writes_a_file_that_round_trips() {
    let (mut app, d) = editing_err("save");
    keys(&mut app, &[KeyCode::Enter, KeyCode::Right, KeyCode::Down, KeyCode::Left, KeyCode::Esc, KeyCode::End, KeyCode::Enter]);
    let edited = app.themes.editor.as_ref().unwrap().theme.clone();
    assert!(app.theme_dirty());
    let at = find(&mut app, "Save ^S");
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), at);
    assert!(!app.theme_dirty(), "{}", app.msg);
    assert_eq!(parse(&std::fs::read_to_string(d.join("ed.toml")).unwrap()).unwrap(), edited);
    assert_eq!(app.themes.set.get("ed"), Some(&edited));
}

#[test]
fn esc_with_unsaved_edits_asks_save_discard_or_back() {
    let (mut app, d) = editing_err("unsaved");
    keys(&mut app, &[KeyCode::Esc]);
    assert!(app.themes.editor.is_none(), "nothing unsaved closes at once");
    keys(&mut app, &[KeyCode::Char('e')]);
    keys(&mut app, &[KeyCode::Char('#')]);
    type_str(&mut app, "123456");
    keys(&mut app, &[KeyCode::Enter, KeyCode::Esc]);
    assert!(matches!(app.mode, Mode::ThemeUnsaved));
    assert!(text(&draw(&mut app, 100, 30)).contains("Discard (d)"));
    keys(&mut app, &[KeyCode::Esc]);
    assert!(matches!(app.mode, Mode::Browse) && app.theme_dirty(), "Back keeps the edits");
    keys(&mut app, &[KeyCode::Esc, KeyCode::Char('d')]);
    assert!(app.themes.editor.is_none());
    assert_eq!(app.themes.set.get("ed").unwrap().slots.bg, parse(&std::fs::read_to_string(d.join("ed.toml")).unwrap()).unwrap().slots.bg);
    keys(&mut app, &[KeyCode::Char('e'), KeyCode::Char('#')]);
    type_str(&mut app, "123456");
    keys(&mut app, &[KeyCode::Enter, KeyCode::Esc, KeyCode::Char('s')]);
    assert!(app.themes.editor.is_none());
    assert_eq!(app.themes.set.get("ed").unwrap().slots.bg, Rgb(0x12, 0x34, 0x56));
}

#[test]
fn the_exit_guard_lists_unsaved_theme_edits_and_review_opens_the_editor() {
    let (mut app, _d) = editing_err("guard");
    keys(&mut app, &[KeyCode::Char('#')]);
    type_str(&mut app, "123456");
    keys(&mut app, &[KeyCode::Enter, KeyCode::Char('1')]);
    assert!(app.key(press(KeyCode::Char('q'))), "unsaved theme edits stop quit");
    let f = text(&draw(&mut app, 100, 30));
    for t in ["1 unsaved in 1 tabs", "theme", "edits to ed", "Review theme (r)"] {
        assert!(f.contains(t), "{t} missing:\n{f}");
    }
    keys(&mut app, &[KeyCode::Char('r')]);
    assert!(app.on_theme_tab() && app.themes.editor.is_some());
    // With settings pending too, both are listed.
    keys(&mut app, &[KeyCode::Char('1'), KeyCode::Enter, KeyCode::Char('q')]);
    let f = text(&draw(&mut app, 100, 30));
    assert!(f.contains("2 unsaved in 2 tabs") && f.contains("ways") && f.contains("edits to ed"), "{f}");
    keys(&mut app, &[KeyCode::Char('D')]);
    assert!(!app.key(press(KeyCode::Char('y'))));
    assert!(app.themes.editor.is_none());
}

/// A frame in each kind of screen: browse, the pending pane, the menu, a
/// confirm, help, the guard, review, a flow-less theme tab, its menu, the
/// editor and the unsaved prompt.
fn every_screen(app: &mut App, mut check: impl FnMut(&str, &Buffer)) {
    let mut shot = |app: &mut App, name: &str| {
        let b = draw(app, 100, 30);
        check(name, &b);
    };
    shot(app, "browse");
    keys(app, &[KeyCode::Enter, KeyCode::Char('c')]);
    shot(app, "pending");
    keys(app, &[KeyCode::Char('c'), KeyCode::Char('?')]);
    shot(app, "help");
    keys(app, &[KeyCode::Esc, KeyCode::Char('w')]);
    shot(app, "review");
    keys(app, &[KeyCode::Esc, KeyCode::Char('q')]);
    shot(app, "guard");
    keys(app, &[KeyCode::Esc, KeyCode::Char('3')]);
    shot(app, "theme");
    keys(app, &[KeyCode::Char('a')]);
    shot(app, "theme-menu");
    keys(app, &[KeyCode::Esc, KeyCode::Down, KeyCode::Char('e')]);
    shot(app, "theme-name");
    type_str(app, "x");
    keys(app, &[KeyCode::Enter, KeyCode::Enter]);
    shot(app, "editor");
    keys(app, &[KeyCode::Esc, KeyCode::Esc]);
    shot(app, "unsaved");
}

#[test]
fn fill_leaves_no_cell_on_the_terminal_default() {
    let d = dir("fill");
    std::fs::write(d.join("active"), "nord\n").unwrap();
    let mut app = app_in(&d, ColorDepth::TrueColor);
    every_screen(&mut app, |name, b| {
        for (i, c) in b.content().iter().enumerate() {
            assert_ne!(c.bg, Color::Reset, "{name}: cell {} of row {} has the terminal's background", i % 100, i / 100);
            assert_ne!(c.fg, Color::Reset, "{name}: cell {} of row {} has the terminal's foreground", i % 100, i / 100);
        }
    });
}

#[test]
fn no_color_draws_no_colour_anywhere() {
    let d = dir("nocolor");
    std::fs::write(d.join("active"), "nord\n").unwrap();
    let mut app = app_in(&d, ColorDepth::from_env(Some("1"), Some("truecolor"), Some("xterm-256color")));
    every_screen(&mut app, |name, b| {
        for (i, c) in b.content().iter().enumerate() {
            assert!(c.fg == Color::Reset && c.bg == Color::Reset, "{name}: cell {} of row {} is coloured: {:?} on {:?}", i % 100, i / 100, c.fg, c.bg);
        }
        let t = text(b);
        assert!(!t.contains('\u{e0b6}') && !t.contains('\u{e0b4}'), "{name}: lozenge glyphs without colour");
    });
}

#[test]
fn depth_brings_roles_down_to_the_terminal() {
    let d = dir("depth");
    let mut app = app_in(&d, ColorDepth::Ansi256);
    let buf = draw(&mut app, 100, 24);
    assert!(buf.content().iter().all(|c| matches!(c.fg, Color::Reset | Color::Indexed(_)) && matches!(c.bg, Color::Reset | Color::Indexed(_))));
}

/// Frames of the theme tab and of the ways tab under four themes, on the
/// real tree: `SNAP_DIR=dir cargo test snap_themes -- --ignored`.
#[test]
#[ignore]
fn snap_themes() {
    let out = PathBuf::from(std::env::var("SNAP_DIR").expect("SNAP_DIR"));
    let here = std::env::current_dir().unwrap();
    let d = dir("snap");
    let real = |active: &str| {
        std::fs::write(d.join("active"), format!("{active}\n")).unwrap();
        App::new(" ways settings ", crate::ways::build(&crate::ways::Paths::resolve(&here), &here)).themes(Themes::new(Some(d.clone()), ColorDepth::TrueColor))
    };
    let shot = |app: &mut App, name: &str, w: u16, h: u16| {
        let b = draw(app, w, h);
        let cells: Vec<String> = b.content().iter().map(|c| format!("{}\t{:?}\t{:?}\t{:?}", c.symbol(), c.fg, c.bg, c.modifier)).collect();
        std::fs::write(out.join(format!("{name}.cells")), format!("{w} {h}\n{}\n", cells.join("\n"))).unwrap();
    };
    for t in ["agent-ways", "nord", "dracula", "paper"] {
        let mut app = real(t);
        keys(&mut app, &[KeyCode::Down, KeyCode::Down, KeyCode::Right, KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
        shot(&mut app, &format!("theme-ways-{t}-100x30"), 100, 30);
    }
    let mut app = real("agent-ways");
    let tab = app.theme_tab();
    keys(&mut app, &[KeyCode::Char(char::from(b'1' + tab as u8)), KeyCode::Down]);
    shot(&mut app, "theme-tab-80x25", 80, 25);
    shot(&mut app, "theme-tab-100x30", 100, 30);
    keys(&mut app, &[KeyCode::Char('e')]);
    type_str(&mut app, "my-nord");
    keys(&mut app, &[KeyCode::Enter]);
    keys(&mut app, &[KeyCode::Down; 8]);
    keys(&mut app, &[KeyCode::Enter, KeyCode::Down, KeyCode::Down]);
    app.key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
    shot(&mut app, "theme-editor-err-100x30", 100, 30);
    shot(&mut app, "theme-editor-err-80x25", 80, 25);
    keys(&mut app, &[KeyCode::Esc, KeyCode::Esc]);
    shot(&mut app, "theme-unsaved-100x30", 100, 30);
}
