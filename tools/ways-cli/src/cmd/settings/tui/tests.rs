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
        user_ways: fx.root.join(".config/agent-ways/ways"),
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

fn nodes(n: &Node) -> Vec<&Node> {
    std::iter::once(n).chain(n.children.iter().flat_map(nodes)).collect()
}

#[test]
fn every_action_on_every_tab_has_one_key_of_its_own() {
    let fx = Fixture::home();
    let roots = roots(&fx);
    let with_actions: Vec<&Node> = roots.iter().flat_map(nodes).filter(|n| !n.actions.is_empty()).collect();
    assert!(with_actions.len() >= 3, "the sweep reached the install and gate actions");
    for n in with_actions {
        let bad = agent_tui::tree::key_conflicts(&n.actions);
        assert!(bad.is_empty(), "{}: {bad:?}", n.name);
    }
}

#[test]
fn the_footer_names_the_selection_s_actions_and_their_keys_run_them() {
    let fx = Fixture::home();
    let mut app = app(&fx);
    press(&mut app, &[KeyCode::Char('4')]);
    let last = frame(&mut app, 100, 30).last().cloned().unwrap();
    assert!(last.contains("t activate · A add · p plan"), "{last}");
    press(&mut app, &[KeyCode::Char('p')]);
    assert!(text(&mut app).contains("directory"), "p asks for plan's directory");
    press(&mut app, &[KeyCode::Esc, KeyCode::Char('t')]);
    assert_eq!(flow(&app).key, "install.targets");
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
    assert!(q.iter().all(|c| c.starts_with("ways target add ")), "{q:?}");
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
    assert_eq!(q[0].command, format!("ways target add {}", arg(&fx.root.join(".claude-work"))));
    assert!(q[0].confirm && q[0].key == "install.targets");
    assert_eq!((app.pending_in(3), app.pending_in(0)), (1, 0), "it counts on the install tab only");
    press(&mut app, &[KeyCode::Char('w')]);
    let t = text(&mut app);
    assert!(t.contains("review") && t.contains("queued") && t.contains("$ ways targ") && t.contains('▲'), "{t}");
    press(&mut app, &[KeyCode::Down]);
    let t = text(&mut app);
    assert!(t.contains("asks") && t.contains("yes: it is"), "{t}");
    press(&mut app, &[KeyCode::Char('a')]);
    agent_tui::testkit::finish_apply(&mut app);
    assert_eq!(*ran.borrow(), [format!("ways target add {}", arg(&fx.root.join(".claude-work")))]);
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
    assert_eq!(queue(&app)[0], format!("ways target enable {}", arg(&off)));
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
    // A long path is cut in the middle of the line, so only its head is sure to show.
    assert!(text(&mut app).contains("$ ways init --pro"));
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
    assert_eq!(r.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["ways", "matching", "gate", "install", "attend", "sensors"]);
    // attend's tabs: its sections, and a group per sensor on the next one.
    assert_eq!(r[4].children.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["governor", "engagement", "cleanup"]);
    assert_eq!(r[5].children.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), attend_config::BUILTINS);
    let project = r[0].children.iter().find(|n| n.name == "project").expect("the per-way toggles");
    let shipped = project.children.iter().find(|n| n.name == "shipped").expect("shipped ways in their own section");
    assert!(shipped.section, "a scope section is no part of the keys");
    let one = &shipped.children.iter().find(|n| n.name == "a").unwrap().children[0];
    assert_eq!(one.name, "one");
    let s = one.setting.as_ref().expect("a way is a toggle");
    assert_eq!((s.value.as_str(), s.store.as_ref().map(|st| st.key.as_str())), ("true", Some("ways.project.a/one")));
    assert_eq!(one.children[0].name, "two", "a way with ways under it is both a toggle and a group");
    assert!(PathBuf::from(&s.store.as_ref().unwrap().file).ends_with("work/current/.claude/ways.yaml"), "a toggle writes the project file");
    assert!(r[3].children.iter().any(|n| n.name == "targets" && !n.actions.is_empty()));
    assert!(r.iter().all(|t| t.children.iter().all(|n| n.name != "theme")), "the theme keys belong to the theme tab");
    // The theme section is declared once, in agent-theme, and its shapes are the screens' own.
    assert_eq!(agent_theme::settings::SHAPES, agent_tui::theme::Shape::NAMES);
}

fn section<'a>(project: &'a Node, name: &str) -> &'a Node {
    project.children.iter().find(|n| n.name == name).unwrap_or_else(|| panic!("no {name} section"))
}

#[test]
fn the_ways_tab_lists_every_scope_each_way_once_where_it_wins() {
    let fx = Fixture::home();
    let way = |root: &str, id: &str, extra: &str| {
        let name = id.rsplit('/').next().unwrap();
        fx.file(&format!("{root}/{id}/{name}.md"), &format!("---\ndescription: {name} from {root}\n{extra}---\n"));
    };
    way("corpus", "a/one", "");
    way("corpus", "b/three", "");
    way(".config/agent-ways/ways", "b/three", "vocabulary: mine\n");
    way("work/current/.claude/ways", "api/dual", "pattern: \\bapi\\b\nmacro: prepend\n");
    fx.file("work/current/.claude/ways/api/dual/macro.sh", "#!/bin/sh\n# says what the API is\ncurl -s localhost/api\n");
    let r = roots(&fx);
    let project = r[0].children.iter().find(|n| n.name == "project").unwrap();
    let names: Vec<&str> = project.children.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, ["this project", "your ways", "shipped"], "scopes in precedence order");
    let mine = &section(project, "your ways").children[0].children[0];
    assert_eq!(mine.name, "three");
    assert!(section(project, "shipped").children.iter().all(|d| d.name != "b"), "a user way shadows the shipped one");
    assert!(mine.about.contains("three from .config/agent-ways/ways") && mine.about.contains("vocabulary mine"), "{}", mine.about);
    let dual = &section(project, "this project").children[0].children[0];
    let st = dual.setting.as_ref().unwrap().store.as_ref().unwrap();
    assert_eq!(st.key, "ways.project.api/dual", "a project way's switch has the same key as any other");
    assert!(dual.about.contains("pattern    \\bapi\\b") && dual.about.contains("macro      prepend") && dual.about.contains("curl -s localhost/api"), "{}", dual.about);
    assert!(!dual.about.contains("says what"), "the macro's comments are not what it runs");
    assert_eq!(section(project, "this project").about, "1 way, 0 switched off as loaded.");
    assert!(dual.about.contains("from       this project"), "{}", dual.about);
}

#[test]
fn a_project_without_ways_says_how_to_add_them() {
    let fx = Fixture::home();
    fx.file("corpus/a/one/one.md", "---\ndescription: one\n---\n");
    let r = roots(&fx);
    let project = r[0].children.iter().find(|n| n.name == "project").unwrap();
    let none = &section(project, "this project").children[0];
    assert_eq!(none.setting.as_ref().unwrap().value, "none · ways init");
    assert_eq!(section(project, "this project").about, "0 ways, 0 switched off as loaded.", "the placeholder is no way");
}

#[test]
fn a_way_file_is_found_by_its_frontmatter_whatever_its_name() {
    let fx = Fixture::home();
    fx.file("corpus/a/one/one.md", "---\ndescription: one\n---\n");
    fx.file("work/current/.claude/ways/api/dual/way.md", "---\ndescription: named way.md\nscope: agent, subagent\n---\n");
    let r = roots(&fx);
    let project = r[0].children.iter().find(|n| n.name == "project").unwrap();
    let dual = &section(project, "this project").children[0].children[0];
    assert!(dual.about.starts_with("named way.md") && dual.about.contains("scope      agent, subagent"), "{}", dual.about);
}

#[test]
fn a_switch_for_a_way_no_root_holds_is_listed_as_not_found() {
    let fx = Fixture::home();
    fx.file("corpus/a/one/one.md", "---\ndescription: one\n---\n");
    fx.file("work/current/.claude/ways.yaml", "ways:\n  gone/old: false\n");
    // The project's file only: the user layer would read this machine's config.
    let file = fx.root.join("work/current/.claude/ways.yaml");
    let project = agent_settings::Layer::read(&ways_core::settings::SCHEMA, "project", ways_core::settings::FILE, agent_settings::LayerScope::Project, &file);
    let ctx = Ctx {
        project: fx.root.join("work/current"),
        home: fx.root.clone(),
        corpus: fx.root.join("corpus"),
        user_ways: fx.root.join(".config/agent-ways/ways"),
        themes: None,
        xdg_config: fx.root.join(".config"),
        claude_config_dir: None,
        claude: fx.root.join(".claude"),
    };
    let r = Ways::new(ctx).build(&[project]);
    let project = r[0].children.iter().find(|n| n.name == "project").unwrap();
    let gone = &section(project, "not found").children[0].children[0];
    assert_eq!(gone.setting.as_ref().unwrap().store.as_ref().unwrap().key, "ways.project.gone/old");
    assert_eq!(section(project, "not found").about, "1 way, 1 switched off as loaded.");
}

// ── other projects' ways ───────────────────────────────────────

fn ctx_of(fx: &Fixture) -> Ctx {
    Ctx {
        project: fx.root.join("work/current"),
        home: fx.root.clone(),
        corpus: fx.root.join("corpus"),
        user_ways: fx.root.join(".config/agent-ways/ways"),
        themes: None,
        xdg_config: fx.root.join(".config"),
        claude_config_dir: None,
        claude: fx.root.join(".claude"),
    }
}

/// A project Claude Code knows, at `rel` under the fixture, with `ways`.
fn known(fx: &Fixture, rel: &str, ways: &[&str]) -> PathBuf {
    let dir = fx.dir(rel);
    let path = dir.display().to_string();
    // Serialized, not formatted: a Windows path's backslashes need escaping.
    let index = serde_json::json!({ "originalPath": path }).to_string();
    fx.file(&format!(".claude/projects/{}/sessions-index.json", claude_sessions::project_slug(&path)), &index);
    fx.dir(&format!("{rel}/.claude/ways"));
    for id in ways {
        let name = id.rsplit('/').next().unwrap();
        fx.file(&format!("{rel}/.claude/ways/{id}/{name}.md"), &format!("---\ndescription: {name} of {rel}\n---\n"));
    }
    dir
}

fn project_group(r: &[Node]) -> &Node {
    r[0].children.iter().find(|n| n.name == "project").unwrap()
}

fn others(r: &[Node]) -> &Node {
    section(project_group(r), "other projects")
}

/// The key the ways tab's view action runs, as the footer names it.
fn view_key(r: &[Node]) -> char {
    let actions = &r[0].actions;
    let i = actions.iter().position(|a| matches!(a.arg, agent_tui::tree::Arg::View(_))).expect("the ways tab has a view action");
    agent_tui::tree::action_keys(actions)[i].expect("a key of its own")
}

#[test]
fn the_view_action_switches_between_this_project_and_every_known_one() {
    let fx = Fixture::home();
    fx.file("corpus/a/one/one.md", "---\ndescription: one\n---\n");
    known(&fx, "work/current", &["mine/here"]);
    known(&fx, "work/other", &["api/dual", "api/dual/deep"]);
    known(&fx, "lab/third", &["x/y"]);
    let mut ways = Ways::new(ctx_of(&fx));
    let r = ways.build(&[]);
    assert!(others(&r).section, "the other projects gather under a header after this one's scopes");
    assert_eq!(project_group(&r).children.last().unwrap().name, "other projects");
    let hint = &others(&r).children[..];
    assert_eq!(hint.len(), 1, "one row stands for them all");
    let k = view_key(&r);
    assert_eq!(hint[0].setting.as_ref().unwrap().value, format!("3 ways · {k} shows them"), "this project's own ways are not counted");
    assert!(hint[0].doc.contains(&format!("3 more ways in 2 other projects. The projects: all action ({k})")), "{}", hint[0].doc);

    assert_eq!(ways.view("projects", &[]), Ok(Some("all projects shown".into())));
    let r = ways.build(&[]);
    let names: Vec<&str> = others(&r).children.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, ["other", "third"], "a group per other project, by name");
    assert!(r[0].actions.iter().any(|a| a.label == "projects: this one"));
    assert_eq!(view_key(&r), k, "one key switches both ways");
    let other = section(others(&r), "other");
    assert!(!other.section, "a project's name is part of its ways' keys");
    assert_eq!(other.columns, Some(("way".into(), "enabled".into())));
    assert!(other.doc.contains("writes ~/work/other/.claude/ways.yaml"), "{}", other.doc);
    assert_eq!(other.about, "2 ways, 0 switched off as loaded.");
    let keys: Vec<String> = agent_tui::tree::keyed(&r).into_iter().map(|(_, k)| k).collect();
    assert!(keys.contains(&"ways.project.other.api.dual.deep".to_string()), "{keys:?}");

    assert_eq!(ways.view("projects", &[]), Ok(Some("this project shown".into())));
    assert_eq!(others(&ways.build(&[])).children[0].name, "(hidden)");
}

#[test]
fn another_project_s_switch_reads_and_writes_that_project_s_file() {
    let fx = Fixture::home();
    let other = known(&fx, "work/other", &["api/dual", "api/rest"]);
    fx.file("work/other/.claude/ways.yaml", "ways:\n  api/rest: false\n");
    let mut ways = Ways::new(ctx_of(&fx));
    ways.view("projects", &[]).unwrap();
    let r = ways.build(&[]);
    let api = &section(others(&r), "other").children[0];
    let (dual, rest) = (&api.children[0], &api.children[1]);
    let file = other.join(".claude/ways.yaml");
    let st = rest.setting.as_ref().unwrap();
    assert_eq!((st.value.as_str(), st.default.as_deref()), ("false", Some("true")), "the value is that project's");
    assert_eq!(dual.setting.as_ref().unwrap().value, "true", "absent means on");
    let store = st.store.as_ref().unwrap();
    assert_eq!((store.file.as_path(), store.key.as_str(), store.shown.as_str()), (file.as_path(), "ways.project.api/rest", "~/work/other/.claude/ways.yaml"));
    assert!(rest.about.contains("from       another project") && rest.about.contains("root       ~/work/other/.claude/ways"), "{}", rest.about);
    // The write lands in that project's file, never this one's.
    let dstore = dual.setting.as_ref().unwrap().store.clone().unwrap();
    ways.write(&file, &[Write { store: &dstore, value: "false", loaded: "true" }]).expect("another project's file takes the write");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "ways:\n  api/rest: false\n  api/dual: false\n");
    assert!(!fx.root.join("work/current/.claude/ways.yaml").exists());
    // An outside change to that file is caught against the file itself.
    let e = ways.write(&file, &[Write { store: &dstore, value: "true", loaded: "true" }]).expect_err("it is false on disk now");
    assert!(e.contains("ways.project.api/dual is false on disk"), "{e}");
}

#[test]
fn no_hint_when_no_other_project_has_ways() {
    let fx = Fixture::home();
    fx.file("corpus/a/one/one.md", "---\ndescription: one\n---\n");
    known(&fx, "work/current", &["mine/here"]);
    known(&fx, "work/empty", &[]);
    let r = roots(&fx);
    assert!(project_group(&r).children.iter().all(|n| n.name != "other projects"), "a project whose ways dir is empty has no ways");
}

#[test]
fn projects_of_one_name_are_told_apart_by_their_parent() {
    let fx = Fixture::home();
    known(&fx, "a/app", &["x/y"]);
    known(&fx, "b/app", &["x/y"]);
    known(&fx, "c/api", &["x/y"]);
    fx.file("work/current/.claude/ways/api/dual/dual.md", "---\ndescription: d\n---\n");
    let mut ways = Ways::new(ctx_of(&fx));
    ways.view("projects", &[]).unwrap();
    let r = ways.build(&[]);
    let names: Vec<&str> = others(&r).children.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, ["a/app", "b/app", "c/api"], "`api` is this project's way, so the project named api takes its parent");
}

#[test]
fn a_dotted_project_name_never_shares_a_key_with_another_project_s_way() {
    let fx = Fixture::home();
    known(&fx, "work/foo", &["js/x"]);
    known(&fx, "work/foo.js", &["x"]);
    let mut ways = Ways::new(ctx_of(&fx));
    ways.view("projects", &[]).unwrap();
    let r = ways.build(&[]);
    let names: Vec<&str> = others(&r).children.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, ["foo", "foo·js"]);
    let keys: Vec<String> = agent_tui::tree::keyed(&r).into_iter().map(|(_, k)| k).filter(|k| k.ends_with(".x")).collect();
    assert_eq!(keys, ["ways.project.foo.js.x", "ways.project.foo·js.x"], "each way keeps a key of its own");
}

#[test]
fn back_to_this_project_is_refused_while_an_edit_elsewhere_is_pending() {
    let fx = Fixture::home();
    fx.file("corpus/a/one/one.md", "---\ndescription: one\n---\n");
    known(&fx, "work/other", &["api/dual"]);
    let mut ways = Ways::new(ctx_of(&fx));
    ways.view("projects", &[]).unwrap();
    let r = ways.build(&[]);
    let elsewhere = section(others(&r), "other").children[0].children[0].setting.clone().unwrap().store.unwrap();
    let here = section(project_group(&r), "shipped").children[0].children[0].setting.clone().unwrap().store.unwrap();
    let e = ways.view("projects", &[&here, &elsewhere]).expect_err("the edit to work/other would be dropped");
    assert_eq!(e, "1 pending change to other projects' ways.yaml would be dropped; review and apply, or undo, it first");
    assert_eq!(ways.title("ways").unwrap(), format!(" ways settings — ~/work/current · all projects "), "the view stays");
    assert!(ways.view("projects", &[&here]).is_ok(), "an edit to this project's file shows in both views");
    assert_eq!(ways.title("ways").unwrap(), " ways settings — ~/work/current · this project ");
    assert_eq!(ways.title("matching"), None, "only the ways tab has views");
}

#[test]
fn a_view_switch_finds_the_other_projects_again() {
    let fx = Fixture::home();
    known(&fx, "work/other", &["api/dual"]);
    let mut ways = Ways::new(ctx_of(&fx));
    assert_eq!(ways.others().len(), 1);
    known(&fx, "work/later", &["x/y"]);
    assert_eq!(ways.others().len(), 1, "kept between builds");
    ways.view("projects", &[]).unwrap();
    assert_eq!(ways.others().len(), 2);
}

#[test]
fn a_write_to_a_ways_yaml_no_known_project_owns_is_refused() {
    let fx = Fixture::home();
    known(&fx, "work/other", &["api/dual"]);
    let mut ways = Ways::new(ctx_of(&fx));
    let file = fx.root.join("stray/.claude/ways.yaml");
    let st = agent_tui::tree::Setting::new(agent_tui::tree::Kind::Bool, "true", "default").store("project", file.clone(), "ways.project.api/dual").store.unwrap();
    let e = ways.write(&file, &[Write { store: &st, value: "false", loaded: "true" }]).expect_err("no known project owns it");
    assert!(e.contains("stray/.claude/ways.yaml is the ways.yaml of neither this project nor one Claude Code knows"), "{e}");
    assert!(!file.exists());
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
        .env("WAYS_TUI_FIXTURE", &root)
        .env("WAYS_SETTINGS_RUNNER", root.join("runner.sh"))
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

#[test]
#[ignore = "run by a_write_waits_a_bounded_time_for_a_held_lock"]
fn child_gives_up_on_a_held_lock() {
    let Some((mut ways, user)) = child() else { return };
    std::fs::write(&user, "near_miss_margin: 0.05\n").unwrap();
    let held = agent_settings::writer::Lock::acquire(&user).unwrap();
    let st = store("matching.near_miss_margin", &user);
    let start = std::time::Instant::now();
    let e = ways.write(&user, &[Write { store: &st, value: "0.1", loaded: "0.05" }]).expect_err("a held lock is not waited on forever");
    assert!(start.elapsed() < std::time::Duration::from_secs(4), "{:?}", start.elapsed());
    assert!(e.contains("another writer holds it"), "{e}");
    drop(held);
    assert_eq!(std::fs::read_to_string(&user).unwrap(), "near_miss_margin: 0.05\n");
}

#[test]
fn a_write_waits_a_bounded_time_for_a_held_lock() {
    let _ = std::fs::remove_dir_all(in_fixture("child_gives_up_on_a_held_lock"));
}

#[cfg(unix)]
#[test]
#[ignore = "run by stopping_a_command_ends_what_it_started_at_once"]
fn child_stops_a_command_and_its_children() {
    let Some((mut ways, _)) = child() else { return };
    let root = PathBuf::from(std::env::var_os("WAYS_TUI_FIXTURE").unwrap());
    let pidfile = root.join("sleep.pid");
    // A stand-in that does not exec: it starts a process and waits on it.
    std::fs::write(root.join("runner.sh"), format!("#!/bin/sh\nsleep 45 &\necho $! > {}\nwait\n", pidfile.display())).unwrap();
    std::fs::set_permissions(root.join("runner.sh"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let mut job = ways.start(&Queued::new("gate", "check", "ways agent key check", false));
    let start = std::time::Instant::now();
    while !pidfile.exists() && start.elapsed() < std::time::Duration::from_secs(10) {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let sleep: u32 = std::fs::read_to_string(&pidfile).unwrap().trim().parse().unwrap();
    assert!(job.poll().is_none(), "still running");
    let start = std::time::Instant::now();
    job.stop();
    let ended = job.poll();
    assert!(start.elapsed() < std::time::Duration::from_secs(1), "the stop waited on the command's child: {:?}", start.elapsed());
    assert!(matches!(ended, Some(Err(_))), "{ended:?}");
    let stat = std::fs::read_to_string(format!("/proc/{sleep}/stat")).unwrap_or_default();
    assert!(stat.is_empty() || stat.split_whitespace().nth(2) == Some("Z"), "the process the command started is still running: {stat}");
}

#[cfg(unix)]
#[test]
fn stopping_a_command_ends_what_it_started_at_once() {
    let _ = std::fs::remove_dir_all(in_fixture("child_stops_a_command_and_its_children"));
}

// Linux: macOS has no setsid command.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "run by a_stop_does_not_wait_on_a_process_that_left_the_group"]
fn child_does_not_wait_on_a_process_out_of_the_group() {
    let Some((mut ways, _)) = child() else { return };
    let root = PathBuf::from(std::env::var_os("WAYS_TUI_FIXTURE").unwrap());
    let pidfile = root.join("stray.pid");
    // A process in a session of its own still holds the command's pipes;
    // killing the group cannot reach it.
    std::fs::write(root.join("runner.sh"), format!("#!/bin/sh\nsetsid sleep 45 &\necho $! > {}\nwait\n", pidfile.display())).unwrap();
    std::fs::set_permissions(root.join("runner.sh"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let mut job = ways.start(&Queued::new("gate", "check", "ways agent key check", false));
    let start = std::time::Instant::now();
    while !pidfile.exists() && start.elapsed() < std::time::Duration::from_secs(10) {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    std::thread::sleep(std::time::Duration::from_millis(100));
    let stray = std::fs::read_to_string(&pidfile).unwrap().trim().to_string();
    let start = std::time::Instant::now();
    job.stop();
    let ended = job.poll();
    let took = start.elapsed();
    let _ = std::process::Command::new("kill").args(["-9", &stray]).status();
    assert!(took < std::time::Duration::from_secs(1), "the stop waited on output a stray process holds: {took:?}");
    assert!(matches!(ended, Some(Err(_))), "{ended:?}");
}

#[cfg(target_os = "linux")]
#[test]
fn a_stop_does_not_wait_on_a_process_that_left_the_group() {
    let _ = std::fs::remove_dir_all(in_fixture("child_does_not_wait_on_a_process_out_of_the_group"));
}
