//! The theme tab's Claude Code section, driven through the real key and
//! mouse handlers. The adapter keeps the render settings in memory and
//! counts the writes it was asked for.

use std::cell::RefCell;
use std::rc::Rc;

use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;

use super::themestate::{ThemeAct, Themes};
use super::*;
use crate::adapter::{ClaudeCode, Render, Write};
use crate::tree::Setting;
use agent_theme::ColorDepth;

/// The section's state, shared with the test after the adapter is boxed.
type Held = Rc<RefCell<ClaudeCode>>;

struct Cc(Held);

impl Adapter for Cc {
    fn write(&mut self, _: &std::path::Path, _: &[Write]) -> Result<(), String> {
        Ok(())
    }
    fn run(&mut self, _: &Queued) -> Result<(), String> {
        Ok(())
    }
    fn claude_code(&self) -> Option<ClaudeCode> {
        Some(self.0.borrow().clone())
    }
    fn set_claude_code(&mut self, key: &str, value: &str) -> Result<(), String> {
        let mut cc = self.0.borrow_mut();
        if value == "neon" {
            return Err(format!("{key}: neon is not one of the options"));
        }
        let r = cc.render.iter_mut().find(|r| r.key == key).ok_or("no such key")?;
        r.value = value.to_string();
        Ok(())
    }
}

fn render(k: &str, value: &str, options: &[&str]) -> Render {
    Render {
        key: format!("mods.ways-theme.{k}"),
        label: k.replace('_', " "),
        value: value.into(),
        default: value.into(),
        options: options.iter().map(|o| o.to_string()).collect(),
        doc: format!("What {k} does."),
    }
}

fn held(active: bool) -> Held {
    Rc::new(RefCell::new(ClaudeCode {
        targets: vec![("~/.claude".into(), active, if active { "installed and active" } else { "not installed" }.into())],
        render: vec![render("rail", "true", &["true", "false"]), render("tables", "on", &["on", "text", "off"]), render("prompt_box", "gradient", &["off", "frame", "gradient"])],
        spinner: vec![],
        done: vec![],
        file: "~/.config/agent-ways/config.yaml".into(),
    }))
}

/// The theme tab on `name`, over the in-memory section.
fn app_on(name: &str, cc: &Held) -> App {
    let flag = |n: &str| Node::leaf(n, "", Setting::new(Kind::Bool, "true", "default").default("true"));
    let roots = vec![Node::group("ways", "", vec![flag("alpha")]).opened()];
    let mut app = App::new("t", roots).adapter(Cc(cc.clone())).themes(Themes::new(None, ColorDepth::TrueColor, None));
    keys(&mut app, &[KeyCode::Char('2')]);
    app.themes.focus(name);
    app
}

fn keys(app: &mut App, ks: &[KeyCode]) {
    for k in ks {
        assert!(app.key(KeyEvent::new(*k, KeyModifiers::NONE)));
    }
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

fn value(cc: &Held, k: &str) -> String {
    cc.borrow().render.iter().find(|r| r.key.ends_with(k)).unwrap().value.clone()
}

#[test]
fn the_theme_detail_shows_the_mod_and_the_palette_on_one_line() {
    let cc = held(true);
    let mut app = app_on("miami-nights", &cc);
    let t = text(&draw(&mut app, 120, 35));
    let line = t.lines().find(|l| l.contains("claude    ")).unwrap_or_else(|| panic!("{t}"));
    assert!(line.contains("active"), "{line}");
    assert!(t.contains("c claude code"), "the bottom bar names the key: {t}");
    let off = held(false);
    let t = text(&draw(&mut app_on("miami-nights", &off), 120, 35));
    assert!(t.lines().any(|l| l.contains("claude    ") && l.contains("not active")), "{t}");
}

#[test]
fn c_opens_the_section_and_its_rows_change_in_place() {
    let cc = held(true);
    let mut app = app_on("miami-nights", &cc);
    keys(&mut app, &[KeyCode::Char('c')]);
    assert_eq!(app.themes.claude, Some(0));
    let t = text(&draw(&mut app, 120, 35));
    for want in ["Miami Nights in Claude Code", "installed and active in ~/.claude", " accent  body  ok  warn  err  info ", "rail  true  true | false", "spinner   Cruising Gliding", "from the theme's [words]", "writes    ~/.config/agent-ways/config.yaml"] {
        assert!(t.contains(want), "{want:?} not in\n{t}");
    }
    // Enter steps forward, Left back; each step is kept at once.
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!((value(&cc, "rail").as_str(), app.msg.as_str()), ("false", "mods.ways-theme.rail = false"));
    keys(&mut app, &[KeyCode::Down, KeyCode::Left]);
    assert_eq!(value(&cc, "tables"), "off", "Left from the first option wraps to the last");
    keys(&mut app, &[KeyCode::Right, KeyCode::Right]);
    assert_eq!(value(&cc, "tables"), "text");
    // The rows show what was kept, a changed one apart from its default.
    let t = text(&draw(&mut app, 120, 35));
    assert!(t.lines().any(|l| l.contains("rail") && l.contains("false")), "{t}");
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!(app.themes.claude, None);
    assert!(!text(&draw(&mut app, 120, 35)).contains("Miami Nights in Claude Code"));
}

#[test]
fn a_refused_change_says_why_and_keeps_the_value() {
    let cc = held(true);
    cc.borrow_mut().render[2].options = vec!["gradient".into(), "neon".into()];
    let mut app = app_on("tampa", &cc);
    keys(&mut app, &[KeyCode::Char('c'), KeyCode::End, KeyCode::Right]);
    assert_eq!(value(&cc, "prompt_box"), "gradient");
    assert!(app.msg.starts_with("rejected: mods.ways-theme.prompt_box: neon"), "{}", app.msg);
}

#[test]
fn a_click_selects_a_row_and_a_second_click_changes_it() {
    let cc = held(true);
    let mut app = app_on("miami-nights", &cc);
    keys(&mut app, &[KeyCode::Char('c')]);
    let buf = draw(&mut app, 120, 35);
    let (x, y) = crate::testkit::find(&buf, "tables").expect("the row is drawn");
    let click = |app: &mut App| app.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE });
    click(&mut app);
    assert_eq!((app.themes.claude, value(&cc, "tables").as_str()), (Some(1), "on"));
    click(&mut app);
    assert_eq!(value(&cc, "tables"), "text");
}

#[test]
fn prompt_box_gradient_is_marked_as_frame_under_a_theme_with_no_gradient() {
    let cc = held(true);
    let mut app = app_on("nord", &cc);
    keys(&mut app, &[KeyCode::Char('c'), KeyCode::End]);
    let t = text(&draw(&mut app, 120, 35));
    assert!(t.contains("nord has no gradient, so it draws as frame"), "{t}");
    let mut app = app_on("miami-nights", &cc);
    keys(&mut app, &[KeyCode::Char('c'), KeyCode::End]);
    assert!(!text(&draw(&mut app, 120, 35)).contains("has no gradient"));
}

#[test]
fn the_settings_words_win_over_the_theme_s() {
    let cc = held(true);
    cc.borrow_mut().spinner = vec!["Sailing".into(), "Docking".into()];
    let mut app = app_on("tampa", &cc);
    keys(&mut app, &[KeyCode::Char('c')]);
    let t = text(&draw(&mut app, 120, 35));
    assert!(t.contains("spinner   Sailing Docking") && t.contains("from mods.ways-theme.spinner_words"), "{t}");
    assert!(t.contains("done      Struck Crossed Landed Plundered"), "{t}");
    let mut app = app_on("nord", &cc);
    keys(&mut app, &[KeyCode::Char('c')]);
    let t = text(&draw(&mut app, 120, 35));
    assert!(t.contains("from Claude Code's own"), "a theme with no [words] and no setting: {t}");
}

#[test]
fn the_terminal_theme_maps_to_claude_code_s_own_colours() {
    let cc = held(true);
    let mut app = app_on("terminal", &cc);
    keys(&mut app, &[KeyCode::Char('c')]);
    assert!(text(&draw(&mut app, 120, 35)).contains("palette   Claude Code's own dark"));
}

#[test]
fn the_menu_offers_the_section_only_where_the_adapter_has_one() {
    let cc = held(true);
    let mut app = app_on("nord", &cc);
    assert!(app.theme_acts().contains(&ThemeAct::ClaudeCode));
    app.theme_act(ThemeAct::ClaudeCode);
    assert_eq!(app.themes.claude, Some(0));
    let mut bare = App::new("t", vec![]).themes(Themes::new(None, ColorDepth::TrueColor, None));
    assert!(!bare.theme_acts().contains(&ThemeAct::ClaudeCode));
    keys(&mut bare, &[KeyCode::Char('c')]);
    assert_eq!(bare.themes.claude, None, "c keeps its old meaning there");
    assert!(!text(&draw(&mut bare, 100, 30)).contains("claude"));
}
