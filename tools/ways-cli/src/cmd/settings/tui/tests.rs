//! The guided flows through the real app: keys, clicks, the queue and the
//! review, over a fixture home. Nothing here runs `ways`: the adapter
//! serves the flows over the fixture and logs what an apply would do.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use agent_tui::flow::Flow;
use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use agent_tui::testkit::{render, rows};
use agent_tui::tree::{Node, Queued};
use agent_tui::{Adapter, App, Write};

use super::flows::testkit::Fixture;
use super::flows::{self, Env};
use super::{Ctx, Ways};

/// Serves the flows over a fixture and logs each command an apply runs.
struct Fixtured {
    env: Env,
    ran: Rc<RefCell<Vec<String>>>,
}

impl Adapter for Fixtured {
    fn write(&mut self, _: &Path, _: &[Write]) -> Result<(), String> {
        Ok(())
    }
    fn run(&mut self, q: &Queued) -> Result<(), String> {
        self.ran.borrow_mut().push(q.command.clone());
        Ok(())
    }
    fn flow(&self, name: &str) -> Option<Flow> {
        flows::flow(&self.env, name)
    }
}

/// The real tabs, built from the registry with no settings file, over the
/// fixture's (empty) corpus.
fn roots(fx: &Fixture) -> Vec<Node> {
    let ctx = Ctx {
        project: fx.root.join("work/current"),
        home: fx.root.clone(),
        corpus: fx.root.join("corpus"),
        themes: None,
        xdg_config: fx.root.join(".config"),
        claude_config_dir: None,
        claude: fx.root.join(".claude"),
    };
    Ways::new(ctx).build(&[])
}

fn app_with(fx: &Fixture, env: Env) -> (App, Rc<RefCell<Vec<String>>>) {
    let ran = Rc::new(RefCell::new(Vec::new()));
    (App::new("t", roots(fx)).adapter(Fixtured { env, ran: ran.clone() }), ran)
}

fn app(fx: &Fixture) -> App {
    app_with(fx, fx.env()).0
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
    rows(&render(app, w, h))
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
    app.flow().expect("a flow is open")
}

fn queue(app: &App) -> Vec<String> {
    app.queued().iter().map(|q| q.command.clone()).collect()
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
    assert!(app.flow().is_none(), "Esc on the first step cancels");
    assert!(queue(&app).is_empty() && app.message().contains("cancelled"));
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
        assert!(app.flow().is_none() && queue(&app).is_empty() && app.pending() == 0, "step {steps}");
    }
    let mut app = activate(&fx);
    press(&mut app, &[KeyCode::Down, KeyCode::Enter, KeyCode::Right, KeyCode::Char('q')]);
    assert!(app.flow().is_none() && queue(&app).is_empty());
    let mut app = activate(&fx);
    assert!(app.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
    assert!(app.flow().is_none() && !app.guarding(), "^C drops the flow without the quit prompt");
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
    press(&mut app, &[KeyCode::Right, KeyCode::Right, KeyCode::Enter]);
    let q = queue(&app);
    assert_eq!(q.len(), 3, "{q:?}");
    assert!(q.iter().all(|c| c.starts_with("ways config target add ")), "{q:?}");
}

/// A path as a queued command line carries it.
fn arg(p: &Path) -> String {
    agent_tui::tree::quote(&p.display().to_string())
}

#[test]
fn a_finished_flow_queues_on_its_tab_and_the_review_applies_it() {
    let fx = Fixture::home();
    let (mut app, ran) = app_with(&fx, fx.env());
    press(&mut app, &[KeyCode::Char('4'), KeyCode::Char('a'), KeyCode::Enter]);
    press(&mut app, &[KeyCode::Down, KeyCode::Char(' '), KeyCode::Right, KeyCode::Right, KeyCode::Enter]);
    assert!(app.flow().is_none());
    let q = app.queued();
    assert_eq!(q.len(), 1);
    assert_eq!(q[0].command, format!("ways config target add {}", arg(&fx.root.join(".claude-work"))));
    assert!(q[0].confirm && q[0].key == "install.targets");
    assert_eq!((app.pending_in(3), app.pending_in(0)), (1, 0), "it counts on the install tab only");
    press(&mut app, &[KeyCode::Char('w')]);
    let t = text(&mut app);
    assert!(t.contains("review") && t.contains("queued") && t.contains("$ ways conf") && t.contains('▲'), "{t}");
    press(&mut app, &[KeyCode::Down]);
    let t = text(&mut app);
    assert!(t.contains("asks") && t.contains("yes: it is"), "{t}");
    press(&mut app, &[KeyCode::Char('a')]);
    agent_tui::testkit::finish_apply(&mut app);
    assert_eq!(*ran.borrow(), [format!("ways config target add {}", arg(&fx.root.join(".claude-work")))]);
    assert_eq!(app.pending(), 0);
}

#[test]
fn a_disabled_recorded_target_is_enabled_not_added() {
    let fx = Fixture::home();
    let off = fx.dir("elsewhere/.claude-off");
    let mut env = fx.env();
    env.targets = vec![(off.display().to_string(), false)];
    let (mut app, _) = app_with(&fx, env);
    press(&mut app, &[KeyCode::Char('4'), KeyCode::Char('a'), KeyCode::Enter]);
    assert!(text(&mut app).contains("disabled"));
    press(&mut app, &[KeyCode::Char(' '), KeyCode::Right, KeyCode::Right, KeyCode::Enter]);
    assert_eq!(queue(&app)[0], format!("ways config target enable {}", arg(&off)));
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
    assert_eq!(queue(&app).len(), 1);
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
    // The MEMORY.md line names a long path; init_preview's own test checks it.
    for want in ["ways init --project", ".claude/ways/", ".claude/.gitignore", "_template.md"] {
        assert!(t.contains(want), "{want}:\n{t}");
    }
    press(&mut app, &[KeyCode::Right]);
    assert!(text(&mut app).contains("$ ways init --project "));
    click_text(&mut app, "also set ways.enabled");
    assert!(text(&mut app).contains("[x] also set ways.enabled"));
    press(&mut app, &[KeyCode::Enter]);
    let dir = fx.root.join("work/current");
    assert_eq!(queue(&app), [format!("ways init --project {}", arg(&dir)), format!("ways settings set ways.enabled true --project {}", arg(&dir))]);
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
    assert!(queue(&app).is_empty() && app.message().contains("nothing to queue"));
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

#[test]
fn the_tabs_are_the_registry_roots_with_a_toggle_per_corpus_way() {
    let fx = Fixture::home();
    for id in ["a/one", "a/one/two", "b/three"] {
        let name = id.rsplit('/').next().unwrap();
        fx.file(&format!("corpus/{id}/{name}.md"), &format!("---\ndescription: {name}\n---\n"));
    }
    let r = roots(&fx);
    assert_eq!(r.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["ways", "matching", "gate", "install"]);
    let project = r[0].children.iter().find(|n| n.name == "project").expect("the per-way toggles");
    let one = &project.children.iter().find(|n| n.name == "a").unwrap().children[0];
    assert_eq!(one.name, "one");
    let s = one.setting.as_ref().expect("a way is a toggle");
    assert_eq!((s.value.as_str(), s.store.as_ref().map(|st| st.key.as_str())), ("true", Some("ways.project.a/one")));
    assert_eq!(one.children[0].name, "two", "a way with ways under it is both a toggle and a group");
    assert!(PathBuf::from(&s.store.as_ref().unwrap().file).ends_with("work/current/.claude/ways.yaml"), "a toggle writes the project file");
    assert!(r[3].children.iter().any(|n| n.name == "targets" && !n.actions.is_empty()));
    assert!(r.iter().all(|t| t.children.iter().all(|n| n.name != "theme")), "the theme keys belong to the theme tab");
}

// ── the adapter's write, under the real paths ──────────────────
//
// `Ways::write` finds its files through the environment, which tests in one
// process must not change. Each check runs as a child of this test binary
// with HOME and every XDG directory in a fixture of its own.

fn in_fixture(test: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("ways-tui-adapter-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("home/.config/agent-ways")).unwrap();
    std::fs::create_dir_all(root.join("home/proj")).unwrap();
    let name = format!("{}::{test}", module_path!().split_once("::").map_or(module_path!(), |(_, rest)| rest));
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([name.as_str(), "--exact", "--ignored", "--test-threads=1"])
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("home/.config"))
        .env("XDG_DATA_HOME", root.join("home/.local/share"))
        .env("XDG_STATE_HOME", root.join("home/.local/state"))
        .env("XDG_CACHE_HOME", root.join("home/.cache"))
        .env("CLAUDE_PROJECT_DIR", root.join("home/proj"))
        .env("WAYS_TUI_ADAPTER_CHILD", "1")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success() && text.contains("1 passed"), "{test} in its fixture:\n{text}{}", String::from_utf8_lossy(&out.stderr));
    root
}

/// The adapter over the fixture the parent set up; `None` outside it.
fn child() -> Option<(Ways, PathBuf)> {
    std::env::var_os("WAYS_TUI_ADAPTER_CHILD")?;
    Some((Ways::new(Ctx::from_env(None)), ways_core::paths::user_config()))
}

fn store(key: &str, file: &Path) -> agent_tui::tree::Store {
    agent_tui::tree::Setting::new(agent_tui::tree::Kind::Text, "", "user").store("user", file.to_path_buf(), key).store.unwrap()
}

#[test]
#[ignore = "run by a_value_write_to_a_broken_file_is_refused"]
fn child_refuses_a_broken_file() {
    let Some((mut ways, user)) = child() else { return };
    let broken = "near_miss_margin: 0.2\nmatching: [unclosed\n";
    std::fs::write(&user, broken).unwrap();
    let st = store("matching.near_miss_margin", &user);
    let e = ways.write(&user, &[Write { store: &st, value: "0.1", loaded: "0.05" }]).expect_err("a broken file takes no write");
    assert!(e.contains("does not parse, so it fails closed and takes no write"), "{e}");
    assert_eq!(std::fs::read_to_string(&user).unwrap(), broken, "the file is untouched");
}

#[test]
fn a_value_write_to_a_broken_file_is_refused() {
    let _ = std::fs::remove_dir_all(in_fixture("child_refuses_a_broken_file"));
}

#[test]
#[ignore = "run by a_write_never_overwrites_an_outside_change"]
fn child_refuses_a_changed_value() {
    let Some((mut ways, user)) = child() else { return };
    // The tree read 0.05 (the default); another writer has since set 0.3.
    std::fs::write(&user, "# by hand\nnear_miss_margin: 0.3\n").unwrap();
    let st = store("matching.near_miss_margin", &user);
    let e = ways.write(&user, &[Write { store: &st, value: "0.1", loaded: "0.05" }]).expect_err("an outside change is not overwritten");
    assert!(e.contains("matching.near_miss_margin is 0.3 on disk, not the 0.05 it was read as"), "{e}");
    assert_eq!(std::fs::read_to_string(&user).unwrap(), "# by hand\nnear_miss_margin: 0.3\n");
    // Read as it now is, the write goes through.
    ways.write(&user, &[Write { store: &st, value: "0.1", loaded: "0.3" }]).expect("the value read is the one on disk");
    assert_eq!(std::fs::read_to_string(&user).unwrap(), "# by hand\nnear_miss_margin: 0.1\n");
}

#[test]
fn a_write_never_overwrites_an_outside_change() {
    let _ = std::fs::remove_dir_all(in_fixture("child_refuses_a_changed_value"));
}
