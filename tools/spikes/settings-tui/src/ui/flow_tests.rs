//! The guided flows through the real App: keys, clicks, the queue and the
//! review, over a fixture home. Nothing here runs `ways`.

use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;

use super::*;
use crate::ways::testkit::Fixture;
use crate::ways::{self, Paths};

fn app(fx: &Fixture) -> App {
    let paths = Paths { user: fx.root.join(".config/agent-ways/config.yaml"), agent: fx.root.join("agent.yaml"), keys: fx.root.join("keys"), project: fx.root.join("p.yaml"), corpus: fx.root.join("corpus") };
    App::new("t", ways::build(&paths, &fx.root)).helpers(ways::helpers(fx.env()))
}

fn press(app: &mut App, ks: &[KeyCode]) {
    for k in ks {
        assert!(app.key(KeyEvent::new(*k, KeyModifiers::NONE)));
    }
}

fn type_str(app: &mut App, s: &str) {
    for c in s.chars() {
        press(app, &[KeyCode::Char(c)]);
    }
}

fn frame(app: &mut App, w: u16, h: u16) -> Vec<String> {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    term.backend().buffer().content().chunks(w as usize).map(|r| r.iter().map(|c| c.symbol()).collect()).collect()
}

fn text(app: &mut App) -> String {
    frame(app, 100, 30).join("\n")
}

fn find(app: &mut App, needle: &str) -> (u16, u16) {
    for (y, line) in frame(app, 100, 30).iter().enumerate() {
        if let Some(b) = line.find(needle) {
            return (line[..b].chars().count() as u16, y as u16);
        }
    }
    panic!("{needle} not on screen:\n{}", text(app));
}

fn click_text(app: &mut App, needle: &str) {
    let (x, y) = find(app, needle);
    app.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x + 1, row: y, modifiers: KeyModifiers::NONE });
}

fn flow(app: &App) -> &Flow {
    match &app.mode {
        Mode::Flow(f) => f,
        _ => panic!("no flow open"),
    }
}

/// The install tab, with the flow open from the `targets` row's menu.
fn activate(fx: &Fixture) -> App {
    let mut app = app(fx);
    press(&mut app, &[KeyCode::Char('4'), KeyCode::Char('a'), KeyCode::Enter]);
    app
}

#[test]
fn the_install_tab_launches_the_activation_flow_from_its_menu() {
    let fx = Fixture::home();
    let mut app = activate(&fx);
    let t = text(&mut app);
    assert!(t.contains("activate agent-ways in Claude instances") && t.contains("1 pick · 2 preview · 3 confirm"), "{t}");
    for want in ["~/.claude-work", "available", "~/.claude ", "active", "Other…", "Next (Enter)", "Cancel (q)"] {
        assert!(t.contains(want), "{want} missing:\n{t}");
    }
    assert!(!t.contains("Back (Esc)"), "no Back on the first step");
    assert_eq!(flow(&app).key, "install.targets");
}

#[test]
fn next_back_and_cancel_by_key() {
    let fx = Fixture::home();
    let mut app = activate(&fx);
    // The first row, ~/.claude, is active and cannot be picked; Next says so.
    press(&mut app, &[KeyCode::Right]);
    assert_eq!(flow(&app).step(), 0);
    assert!(text(&mut app).contains("nothing picked yet"));
    press(&mut app, &[KeyCode::Down, KeyCode::Char(' '), KeyCode::Right]);
    assert_eq!(flow(&app).step(), 1);
    let t = text(&mut app);
    assert!(t.contains("plan for") && t.contains("Back (Esc)") && t.contains("2 preview"), "{t}");
    press(&mut app, &[KeyCode::Right]);
    assert_eq!(flow(&app).step(), 2);
    assert!(text(&mut app).contains("Finish (Enter)"));
    press(&mut app, &[KeyCode::Esc, KeyCode::Esc]);
    assert_eq!((flow(&app).step(), flow(&app).picked_ids().len()), (0, 1), "back twice returns to the pick with it kept");
    press(&mut app, &[KeyCode::Esc]);
    assert!(matches!(app.mode, Mode::Browse), "Esc on the first step cancels");
    assert!(app.queue.is_empty() && app.msg.contains("cancelled"));
}

#[test]
fn cancel_anywhere_queues_nothing() {
    let fx = Fixture::home();
    for steps in 0..3 {
        let mut app = activate(&fx);
        press(&mut app, &[KeyCode::Down, KeyCode::Char(' ')]);
        press(&mut app, &vec![KeyCode::Right; steps]);
        assert_eq!(flow(&app).step(), steps);
        click_text(&mut app, "Cancel (q)");
        assert!(matches!(app.mode, Mode::Browse) && app.queue.is_empty() && app.pending() == 0, "step {steps}");
    }
    let mut app = activate(&fx);
    press(&mut app, &[KeyCode::Down, KeyCode::Enter, KeyCode::Right, KeyCode::Char('q')]);
    assert!(matches!(app.mode, Mode::Browse) && app.queue.is_empty());
    let mut app = activate(&fx);
    assert!(app.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
    assert!(matches!(app.mode, Mode::Browse), "^C drops the flow without the quit prompt");
}

#[test]
fn multi_select_by_key_and_other_by_typed_path() {
    let fx = Fixture::home();
    fx.dir("extra/.claude-x");
    fx.file("extra/.claude-x/settings.json", "{}");
    let mut app = activate(&fx);
    press(&mut app, &[KeyCode::Down, KeyCode::Char(' '), KeyCode::Down, KeyCode::Char(' ')]);
    assert_eq!(flow(&app).picked_ids().len(), 2);
    press(&mut app, &[KeyCode::End, KeyCode::Enter]);
    assert!(text(&mut app).contains("path ▏"));
    type_str(&mut app, &format!("{}/extra/.claude-x", fx.root.display()));
    press(&mut app, &[KeyCode::Enter]);
    assert_eq!(flow(&app).picked_ids().len(), 3);
    assert!(text(&mut app).contains("extra/.claude-x"));
    press(&mut app, &[KeyCode::Right, KeyCode::Right]);
    let t = text(&mut app);
    assert_eq!(t.matches("ways config target add").count(), 3, "{t}");
}

#[test]
fn a_finished_flow_queues_on_its_tab_and_the_review_shows_it() {
    let fx = Fixture::home();
    let mut app = activate(&fx);
    press(&mut app, &[KeyCode::Down, KeyCode::Char(' '), KeyCode::Right, KeyCode::Right, KeyCode::Enter]);
    assert!(matches!(app.mode, Mode::Browse));
    let q = app.queue.items();
    assert_eq!(q.len(), 1);
    assert_eq!(q[0].command, format!("ways config target add {}/.claude-work", fx.root.display()));
    assert!(q[0].confirm && q[0].key == "install.targets");
    assert_eq!((app.pending_in(3), app.pending_in(0)), (1, 0), "it counts on the install tab only");
    press(&mut app, &[KeyCode::Char('w')]);
    let t = text(&mut app);
    assert!(t.contains("review & apply install") && t.contains("$ ways config target add") && t.contains("asks first"), "{t}");
    press(&mut app, &[KeyCode::Char('a')]);
    assert!(matches!(&app.mode, Mode::Apply(r) if r.steps[0].text.starts_with("would run ways config target add")));
}

#[test]
fn a_disabled_recorded_target_is_enabled_not_added() {
    let fx = Fixture::home();
    let off = fx.dir("elsewhere/.claude-off");
    fx.file(".config/agent-ways/config.yaml", &format!("targets:\n  - path: {}\n    enabled: false\n", off.display()));
    let mut app = app(&fx);
    press(&mut app, &[KeyCode::Char('4'), KeyCode::Char('a'), KeyCode::Enter]);
    assert!(text(&mut app).contains("disabled"));
    press(&mut app, &[KeyCode::Char(' '), KeyCode::Right, KeyCode::Right, KeyCode::Enter]);
    assert_eq!(app.queue.items()[0].command, format!("ways config target enable {}", off.display()));
}

#[test]
fn clicks_pick_a_row_open_other_press_next_and_back() {
    let fx = Fixture::home();
    let mut app = activate(&fx);
    click_text(&mut app, "~/.claude-work");
    assert_eq!(flow(&app).picked_ids().len(), 1);
    click_text(&mut app, "Other…");
    assert!(text(&mut app).contains("path ▏"), "a click on Other opens the path entry");
    click_text(&mut app, "Next (Enter)");
    assert_eq!(flow(&app).step(), 0, "buttons other than Cancel wait while typing");
    press(&mut app, &[KeyCode::Esc]);
    click_text(&mut app, "Next (Enter)");
    assert_eq!(flow(&app).step(), 1);
    click_text(&mut app, "Back (Esc)");
    assert_eq!(flow(&app).step(), 0);
    click_text(&mut app, "Next (Enter)");
    click_text(&mut app, "Next (Enter)");
    click_text(&mut app, "Finish (Enter)");
    assert_eq!(app.queue.len(), 1);
}

#[test]
fn the_preview_scrolls_with_keys_and_the_wheel() {
    let fx = Fixture::home();
    let mut app = activate(&fx);
    press(&mut app, &[KeyCode::Down, KeyCode::Char(' '), KeyCode::Down, KeyCode::Char(' '), KeyCode::Right]);
    let wheel = |app: &mut App, kind| {
        frame(app, 100, 24);
        app.mouse(MouseEvent { kind, column: 10, row: 10, modifiers: KeyModifiers::NONE });
    };
    assert!(text(&mut app).contains("lines 1-"), "the footer places the view");
    wheel(&mut app, MouseEventKind::ScrollDown);
    press(&mut app, &[KeyCode::Down]);
    assert!(text(&mut app).contains("lines 3-"), "{}", text(&mut app));
    wheel(&mut app, MouseEventKind::ScrollUp);
    press(&mut app, &[KeyCode::PageDown]);
    let last = text(&mut app);
    assert!(last.contains("--force") || last.contains("move a hook"), "{last}");
}

#[test]
fn the_project_flow_queues_init_and_the_checkbox_adds_enable() {
    let fx = Fixture::home();
    fx.dir("work/current/.git");
    let mut app = app(&fx);
    press(&mut app, &[KeyCode::Char('1'), KeyCode::Char('a'), KeyCode::Enter]);
    let t = text(&mut app);
    assert!(t.contains("set up agent-ways in a project") && t.contains("current directory") && t.contains("none"), "{t}");
    assert_eq!(flow(&app).key, "ways");
    press(&mut app, &[KeyCode::Char(' '), KeyCode::Right]);
    let t = text(&mut app);
    for want in ["ways init --project", ".claude/ways/", ".claude/.gitignore", "_template.md", "MEMORY.md"] {
        assert!(t.contains(want), "{want}:\n{t}");
    }
    press(&mut app, &[KeyCode::Right]);
    assert!(text(&mut app).contains("$ ways init --project "));
    click_text(&mut app, "also set ways.enabled");
    assert!(text(&mut app).contains("[x] also set ways.enabled"));
    press(&mut app, &[KeyCode::Enter]);
    let cmds: Vec<_> = app.queue.items().iter().map(|q| q.command.clone()).collect();
    let dir = fx.root.join("work/current");
    assert_eq!(cmds, [format!("ways init --project {}", dir.display()), format!("ways settings set ways.enabled true --project {}", dir.display())]);
    assert_eq!(app.pending_in(0), 2);
}

#[test]
fn a_project_init_would_skip_queues_nothing() {
    let fx = Fixture::home();
    let mut app = app(&fx);
    press(&mut app, &[KeyCode::Char('a'), KeyCode::Enter, KeyCode::Char(' '), KeyCode::Right]);
    assert!(text(&mut app).contains("init does nothing"));
    press(&mut app, &[KeyCode::Right]);
    assert!(text(&mut app).contains("nothing: this choice has no command to queue"));
    press(&mut app, &[KeyCode::Enter]);
    assert!(app.queue.is_empty() && app.msg.contains("nothing to queue"));
}

#[test]
fn the_flow_fits_80x25() {
    let fx = Fixture::home();
    let mut app = activate(&fx);
    let f = frame(&mut app, 80, 25);
    assert!(f.iter().any(|l| l.contains("Next (Enter)")) && f.iter().any(|l| l.contains("1 pick · 2 preview · 3 confirm")));
    press(&mut app, &[KeyCode::Down, KeyCode::Enter]);
    assert!(frame(&mut app, 80, 25).iter().any(|l| l.contains("Back (Esc)")));
}

/// Frames of the flows over a fixture home, as `snap_frames` writes them:
/// `SNAP_DIR=dir cargo test snap_flow -- --ignored`.
#[test]
#[ignore]
fn snap_flow() {
    let out = std::path::PathBuf::from(std::env::var("SNAP_DIR").expect("SNAP_DIR"));
    let fx = Fixture::home();
    fx.dir("elsewhere/.claude-off");
    fx.file("elsewhere/.claude-off/settings.json", "{}");
    fx.file(".config/agent-ways/config.yaml", &format!("targets:\n  - path: {}/elsewhere/.claude-off\n    enabled: false\n  - path: {}/.claude\n", fx.root.display(), fx.root.display()));
    let shot = |app: &mut App, name: &str, w: u16, h: u16| {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let cells: Vec<String> = term.backend().buffer().content().iter().map(|c| format!("{}\t{:?}\t{:?}\t{:?}", c.symbol(), c.fg, c.bg, c.modifier)).collect();
        std::fs::write(out.join(format!("{name}.cells")), format!("{w} {h}\n{}\n", cells.join("\n"))).unwrap();
    };
    let mut app = activate(&fx);
    press(&mut app, &[KeyCode::Down, KeyCode::Char(' '), KeyCode::Down, KeyCode::Char(' ')]);
    shot(&mut app, "flow-pick-100x30", 100, 30);
    shot(&mut app, "flow-pick-80x25", 80, 25);
    press(&mut app, &[KeyCode::Right]);
    shot(&mut app, "flow-preview-100x30", 100, 30);
}

/// The flows over this machine's real home and projects, with the real plan:
/// `SNAP_DIR=dir cargo test snap_real_flow -- --ignored --nocapture`. Prints
/// the candidates it found; runs `ways config target plan`, which is read-only.
#[test]
#[ignore]
fn snap_real_flow() {
    let out = std::path::PathBuf::from(std::env::var("SNAP_DIR").expect("SNAP_DIR"));
    let dir = std::env::current_dir().unwrap();
    let paths = Paths::resolve(&dir);
    let env = ways::Env::resolve(&paths, &dir);
    for c in ways::claude_dirs(&env) {
        println!("claude dir: {} [{}] pickable={} ({})", c.label, c.badge, c.pickable, c.detail);
    }
    for c in ways::projects(&env).iter().take(8) {
        println!("project: {} [{}] ({})", c.label, c.badge, c.detail);
    }
    let shot = |app: &mut App, name: &str, w: u16, h: u16| {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let cells: Vec<String> = term.backend().buffer().content().iter().map(|c| format!("{}\t{:?}\t{:?}\t{:?}", c.symbol(), c.fg, c.bg, c.modifier)).collect();
        std::fs::write(out.join(format!("{name}.cells")), format!("{w} {h}\n{}\n", cells.join("\n"))).unwrap();
    };
    let new = || App::new("t", ways::build(&paths, &dir)).helpers(ways::helpers(ways::Env::resolve(&paths, &dir)));
    let mut app = new();
    press(&mut app, &[KeyCode::Char('4'), KeyCode::Char('a'), KeyCode::Enter, KeyCode::Down, KeyCode::Char(' ')]);
    shot(&mut app, "flow-pick-100x30", 100, 30);
    shot(&mut app, "flow-pick-80x25", 80, 25);
    press(&mut app, &[KeyCode::Right]);
    shot(&mut app, "flow-preview-100x30", 100, 30);
    press(&mut app, &[KeyCode::Right]);
    shot(&mut app, "flow-confirm-100x30", 100, 30);
    let mut app = new();
    press(&mut app, &[KeyCode::Char('1'), KeyCode::Char('a'), KeyCode::Enter, KeyCode::Char(' '), KeyCode::Right]);
    shot(&mut app, "flow-setup-preview-100x30", 100, 30);
}
