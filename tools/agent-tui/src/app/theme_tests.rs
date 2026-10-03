//! The theme tab, driven through the real key and mouse handlers against a
//! themes directory of its own per test. The active choice is kept by the
//! adapter; here it is a `choice` file beside the themes, standing in for
//! the settings key an application keeps it in.

use std::path::PathBuf;

use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::style::Color;
use ratatui::Terminal;

use super::themestate::{Focus, Themes};
use super::*;
use crate::adapter::Write;
use crate::named::ItemAct;
use crate::tree::Setting;
use agent_theme::{parse, ColorDepth, Rgb};

/// Keeps the theme choice in `<dir>/choice`; writes and runs nothing.
struct Choice(PathBuf);

impl Adapter for Choice {
    fn write(&mut self, _: &std::path::Path, _: &[Write]) -> Result<(), String> {
        Ok(())
    }
    fn run(&mut self, _: &Queued) -> Result<(), String> {
        Ok(())
    }
    fn choose_theme(&mut self, name: &str) -> Result<(), String> {
        std::fs::write(self.0.join("choice"), format!("{name}\n")).map_err(|e| e.to_string())
    }
    fn choose_shape(&mut self, name: &str) -> Result<(), String> {
        std::fs::write(self.0.join("shape"), name).map_err(|e| e.to_string())
    }
}

#[test]
fn the_theme_tab_shows_the_shape_and_its_menu_cycles_it() {
    let d = dir("shape");
    let mut app = app_in(&d, ColorDepth::TrueColor).shape(super::theme::Shape::PLAIN);
    keys(&mut app, &[KeyCode::Char('3')]);
    assert!(text(&draw(&mut app, 100, 30)).contains("shape     plain (a: shape cycles it)"));
    act(&mut app, "shape");
    assert_eq!(std::fs::read_to_string(d.join("shape")).unwrap(), "flame", "the adapter keeps the next shape");
    assert_eq!(app.shape, super::theme::Shape::named("flame"));
    assert!(app.msg.contains("shape flame"), "{}", app.msg);
}

/// The choice the adapter holds, if any.
fn chosen(d: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(d.join("choice")).ok()
}

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
    let active = chosen(d).map(|s| s.trim().to_string());
    App::new("t", roots).adapter(Choice(d.to_path_buf())).themes(Themes::new(Some(d.to_path_buf()), depth, active))
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
    let t = agent_theme::BUNDLED.iter().find(|(n, _)| *n == name).map(|(_, s)| parse(s).unwrap()).unwrap();
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
    assert!(f.contains("terminal") && f.contains("agent-ways") && f.contains("nord") && f.contains("paper") && f.contains(&d.display().to_string()[..20]), "{f}");
    keys(&mut app, &[KeyCode::Tab]);
    assert_eq!(app.tab, 0, "Tab wraps from the theme tab to the first");
}

#[test]
fn preview_follows_the_cursor_and_reverts_on_leaving_or_esc() {
    let d = dir("preview");
    let mut app = app_in(&d, ColorDepth::TrueColor);
    keys(&mut app, &[KeyCode::Char('3'), KeyCode::Down, KeyCode::Down]);
    assert_eq!(shown(&app), "nord");
    // Paper fills: the whole frame, tab bar and status line too, is on its bg.
    keys(&mut app, &[KeyCode::End]);
    assert_eq!(shown(&app), "paper");
    let buf = draw(&mut app, 100, 24);
    assert_eq!((buf[(99, 0)].bg, buf[(99, 23)].bg), (bg_of("paper"), bg_of("paper")));
    keys(&mut app, &[KeyCode::Home, KeyCode::Down, KeyCode::Down, KeyCode::Down, KeyCode::Down]);
    assert_eq!(shown(&app), "dracula");
    keys(&mut app, &[KeyCode::Char('1')]);
    assert_eq!(shown(&app), "terminal", "leaving the tab shows the active theme");
    assert_eq!(draw(&mut app, 100, 24)[(99, 0)].bg, Color::Reset, "the default leaves the terminal's background");
    keys(&mut app, &[KeyCode::Char('3')]);
    assert_eq!(shown(&app), "dracula", "the cursor is kept");
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!((shown(&app), app.themes.cursor), ("terminal".to_string(), 0), "Esc returns to the active theme");
    assert!(chosen(&d).is_none(), "previewing keeps no choice");
    assert!(!app.key(press(KeyCode::Esc)), "a second Esc quits, nothing pending");
}

#[test]
fn enter_makes_a_theme_active_and_writes_the_choice() {
    let d = dir("enter");
    let mut app = app_in(&d, ColorDepth::TrueColor);
    keys(&mut app, &[KeyCode::Char('3'), KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
    assert_eq!(chosen(&d).unwrap(), "nord\n");
    assert_eq!(app.pending(), 0, "it is not a pending setting");
    keys(&mut app, &[KeyCode::Char('1')]);
    assert_eq!(shown(&app), "nord");
    assert_eq!(Themes::new(Some(d.clone()), ColorDepth::TrueColor, chosen(&d).map(|s| s.trim().into())).active, "nord", "read back at startup");
    assert_eq!(Themes::new(Some(d.clone()), ColorDepth::TrueColor, Some("gone".into())).active, "terminal", "an unknown choice is the default");
    // A click selects, and a second click on the selected row makes it active.
    keys(&mut app, &[KeyCode::Char('3')]);
    let at = find(&mut app, "dracula");
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), at);
    assert_eq!((shown(&app), app.themes.active.as_str()), ("dracula".to_string(), "nord"));
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), at);
    assert_eq!(chosen(&d).unwrap(), "dracula\n");
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
    assert_eq!(app.theme_acts().len(), 4, "a bundled theme offers new, copy, edit and shape");
    act(&mut app, "new");
    type_str(&mut app, "mine");
    keys(&mut app, &[KeyCode::Enter]);
    let mine = parse(&std::fs::read_to_string(d.join("mine.theme")).unwrap()).unwrap();
    assert_eq!((mine.name.as_str(), mine.slots), ("mine", app.themes.set.get("agent-ways").unwrap().slots));
    assert_eq!(shown(&app), "mine", "the cursor moves to the new theme");

    // Copy nord; each refused name keeps the prompt open with the reason.
    keys(&mut app, &[KeyCode::Home, KeyCode::Down, KeyCode::Down]);
    act(&mut app, "copy");
    for (bad, why) in [("Nord2", "lowercase"), ("my/nord", "path separator"), ("nord 2", "no spaces"), ("mine", "taken by a user theme"), ("dracula", "taken by a bundled theme"), ("", "cannot be empty")] {
        type_str(&mut app, bad);
        keys(&mut app, &[KeyCode::Enter]);
        assert!(app.msg.contains(why) && matches!(&app.mode, Mode::ItemName { op: ItemOp::Copy(f), .. } if f == "nord"), "{bad}: {}", app.msg);
        keys(&mut app, &vec![KeyCode::Backspace; bad.len()]);
    }
    type_str(&mut app, "nord-2");
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(parse(&std::fs::read_to_string(d.join("nord-2.theme")).unwrap()).unwrap().slots, app.themes.set.get("nord").unwrap().slots);
    // The copy opens in the editor, saved; Esc closes it on the copy.
    let e = app.themes.editor.as_ref().expect("the copy opens in the editor");
    assert_eq!((e.theme.name.as_str(), e.written, e.dirty()), ("nord-2", true, false));
    assert!(app.msg.contains("copied nord to nord-2") && app.msg.contains("editing it"), "{}", app.msg);
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!(shown(&app), "nord-2");

    // Rename the active theme: the file moves and the active choice follows.
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.themes.active, "nord-2");
    assert_eq!(app.theme_acts().len(), 6, "a user theme adds rename and delete");
    act(&mut app, "rename");
    type_str(&mut app, "arctic");
    keys(&mut app, &[KeyCode::Enter]);
    assert!(!d.join("nord-2.theme").exists() && d.join("arctic.theme").exists());
    assert_eq!(chosen(&d).unwrap(), "arctic\n");
    assert_eq!((app.themes.active.as_str(), shown(&app).as_str()), ("arctic", "arctic"));

    // Delete the active theme: y/n first, then the default takes over.
    act(&mut app, "delete");
    keys(&mut app, &[KeyCode::Char('n')]);
    assert!(d.join("arctic.theme").exists());
    act(&mut app, "delete");
    keys(&mut app, &[KeyCode::Char('y')]);
    assert!(!d.join("arctic.theme").exists());
    assert_eq!(chosen(&d).unwrap(), "terminal\n");
    assert_eq!(app.themes.active, "terminal");
    assert!(app.themes.set.get("arctic").is_none());
    assert!(app.msg.contains("it was the active theme, so terminal is active now"), "{}", app.msg);
}

#[test]
fn a_bundled_theme_is_never_renamed_or_deleted() {
    let d = dir("bundled");
    let mut app = app_in(&d, ColorDepth::TrueColor);
    keys(&mut app, &[KeyCode::Char('3'), KeyCode::Down, KeyCode::Down]);
    assert_eq!(shown(&app), "nord");
    let labels: Vec<&str> = app.theme_acts().iter().map(|a| a.label()).collect();
    assert_eq!(labels, ["new", "copy", "edit", "shape"], "the menu leaves rename and delete out");
    // Reached anyway, the flow refuses with the reason and asks nothing.
    app.item_act(ItemAct::Delete, "nord".into());
    assert!(matches!(app.mode, Mode::Browse) && app.msg == "rejected: nord is a bundled theme, which is never deleted; copy it to change it", "{}", app.msg);
    app.item_act(ItemAct::Rename, "terminal".into());
    assert!(matches!(app.mode, Mode::Browse) && app.msg.contains("terminal is a bundled theme, which is never renamed"), "{}", app.msg);
    assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0, "nothing was written");
}

#[test]
fn deleting_an_override_leaves_the_bundled_theme_active() {
    let d = dir("override");
    let nord = app_in(&d, ColorDepth::TrueColor).themes.get("nord").cloned().unwrap();
    std::fs::write(d.join("nord.theme"), agent_theme::to_text(&agent_theme::Theme { label: "My Nord".into(), ..nord })).unwrap();
    let mut app = app_in(&d, ColorDepth::TrueColor);
    keys(&mut app, &[KeyCode::Char('3'), KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
    assert_eq!((app.themes.active.as_str(), app.themes.get("nord").unwrap().label.as_str()), ("nord", "My Nord"));
    act(&mut app, "delete");
    keys(&mut app, &[KeyCode::Char('y')]);
    assert!(!d.join("nord.theme").exists());
    assert_eq!((app.themes.active.as_str(), chosen(&d).unwrap().as_str()), ("nord", "nord\n"), "the bundled nord is still there");
    assert!(!app.msg.contains("active now"), "{}", app.msg);
}

#[test]
fn editing_a_bundled_theme_starts_a_copy() {
    let d = dir("editcopy");
    let mut app = app_in(&d, ColorDepth::TrueColor);
    keys(&mut app, &[KeyCode::Char('3'), KeyCode::Down, KeyCode::Down, KeyCode::Char('e')]);
    assert!(matches!(&app.mode, Mode::ThemeName { op: NameOp::EditCopy(f), .. } if f == "nord"));
    type_str(&mut app, "my-nord");
    keys(&mut app, &[KeyCode::Enter]);
    let e = app.themes.editor.as_ref().expect("the editor is open");
    assert_eq!((e.theme.name.as_str(), e.written, e.dirty()), ("my-nord", false, true));
    assert!(app.key(ctrl('s')));
    assert!(d.join("my-nord.theme").exists() && !d.join("nord.theme").exists(), "the bundled theme is never written");
    assert!(!app.theme_dirty());
}

/// The editor open on a user copy of nord, on the `err` row.
fn editing_err(tag: &str) -> (App, PathBuf) {
    let d = dir(tag);
    let mut app = app_in(&d, ColorDepth::TrueColor);
    keys(&mut app, &[KeyCode::Char('3'), KeyCode::Down, KeyCode::Down, KeyCode::Char('e')]);
    type_str(&mut app, "ed");
    keys(&mut app, &[KeyCode::Enter]);
    assert!(app.key(ctrl('s')));
    let err = Slots::NAMES.iter().position(|s| *s == "err").unwrap();
    keys(&mut app, &vec![KeyCode::Down; err]);
    assert_eq!(app.themes.editor.as_ref().unwrap().slot(), Some("err"));
    (app, d)
}

use agent_theme::Slots;

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
    let role = |app: &App| app.themes.roles(app.shown_theme()).err;
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
    assert_eq!(parse(&std::fs::read_to_string(d.join("ed.theme")).unwrap()).unwrap(), edited);
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
    assert_eq!(app.themes.set.get("ed").unwrap().slots.bg, parse(&std::fs::read_to_string(d.join("ed.theme")).unwrap()).unwrap().slots.bg);
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
    std::fs::write(d.join("choice"), "paper\n").unwrap();
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
    std::fs::write(d.join("choice"), "nord\n").unwrap();
    let mut app = app_in(&d, ColorDepth::from_env(|k| match k { "NO_COLOR" => Some("1".into()), "COLORTERM" => Some("truecolor".into()), "TERM" => Some("xterm-256color".into()), _ => None }));
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
    // The default is the terminal's own named colours at any depth.
    let named = |c: Color| !matches!(c, Color::Rgb(..) | Color::Indexed(_));
    for depth in [ColorDepth::TrueColor, ColorDepth::Ansi256, ColorDepth::Ansi16] {
        let buf = draw(&mut app_in(&d, depth), 100, 24);
        assert!(buf.content().iter().all(|c| named(c.fg) && named(c.bg)), "{depth:?}");
    }
    // A chosen theme at 256 colours is brought down to the palette.
    std::fs::write(d.join("choice"), "nord\n").unwrap();
    let buf = draw(&mut app_in(&d, ColorDepth::Ansi256), 100, 24);
    assert!(buf.content().iter().all(|c| matches!(c.fg, Color::Reset | Color::Indexed(_)) && matches!(c.bg, Color::Reset | Color::Indexed(_))));
    assert!(buf.content().iter().any(|c| matches!(c.bg, Color::Indexed(_))), "the theme is drawn");
    // At 16 colours a chosen theme gives way to the default whole.
    let buf = draw(&mut app_in(&d, ColorDepth::Ansi16), 100, 24);
    assert!(buf.content().iter().all(|c| named(c.fg) && named(c.bg)));
}
