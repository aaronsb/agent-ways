use super::*;
use crate::testkit::Dry;
use crate::tree::{Action, Setting};
use ratatui::backend::TestBackend;
use ratatui::style::Modifier;
use ratatui::Terminal;

/// `app` drawing with the bundled agent-ways theme in truecolor, so the
/// colour checks see a theme's roles rather than the terminal palette.
fn themed(app: App) -> App {
    app.themes(Themes::new(None, agent_theme::ColorDepth::TrueColor, Some("agent-ways".into())))
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

fn screen(app: &mut App) -> String {
    let mut term = Terminal::new(TestBackend::new(110, 24)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    term.backend().buffer().content().iter().map(|c| c.symbol()).collect()
}

/// A key node with a secret and a destructive action, and a plain target.
fn action_tree() -> Vec<Node> {
    let key = Node::leaf("anthropic", "", Setting::new(Kind::Secret, "absent", "keys/")).with_actions(vec![
        Action::new("set", "ways agent key add --provider anthropic").arg(Arg::Secret),
        Action::new("remove", "ways agent key remove --provider anthropic").confirm(),
        Action::new("check", "ways agent key check --provider anthropic"),
    ]);
    vec![Node::group("keys", "", vec![key]).opened()]
}

#[test]
fn renders_and_edits_through_keys() {
    let roots = vec![Node::group(
        "matching",
        "",
        vec![Node::leaf("tau_s", "doc", Setting::new(Kind::Float { min: 0.0, max: 1.0 }, "0.5", "default").default("0.5"))],
    )];
    let mut app = App::new("t", roots);
    keys(&mut app, &[KeyCode::Right, KeyCode::Down, KeyCode::Enter, KeyCode::Backspace, KeyCode::Char('4'), KeyCode::Enter]);
    assert_eq!(tree::changes(&app.roots)[0].3, "0.4");
    assert!(screen(&mut app).contains("tau_s"));
}

#[test]
fn typed_secret_never_reaches_the_screen_the_pane_or_the_summary() {
    const SECRET: &str = "Q#Z9@X!";
    let mut app = App::new("t", action_tree());
    keys(&mut app, &[KeyCode::Down, KeyCode::Enter]);
    type_str(&mut app, SECRET);
    let masked = screen(&mut app);
    assert!(masked.contains('•'));
    assert!(!masked.contains(SECRET) && !masked.chars().any(|c| SECRET.contains(c)), "typed characters reached the frame");

    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.queue.len(), 1);
    keys(&mut app, &[KeyCode::Char('c')]);
    let pane = screen(&mut app);
    assert!(pane.contains("<stdin>"));
    assert!(!pane.chars().any(|c| SECRET.contains(c)), "typed characters reached the pending pane");

    let out = summary(&app.roots, &app.queue);
    assert!(out.contains("ways agent key add --provider anthropic < <stdin>"));
    assert!(!out.chars().any(|c| SECRET.contains(c)), "typed characters reached the summary");
    assert!(!format!("{:?}", app.queue).contains(SECRET));
}

#[test]
fn destructive_action_is_queued_only_on_yes() {
    let mut app = App::new("t", action_tree());
    let to_remove = [KeyCode::Down, KeyCode::Char('a'), KeyCode::Down, KeyCode::Enter];
    keys(&mut app, &to_remove);
    keys(&mut app, &[KeyCode::Char('n')]);
    assert!(app.queue.is_empty());
    keys(&mut app, &to_remove);
    keys(&mut app, &[KeyCode::Char('y')]);
    assert_eq!(app.queue.items()[0].command, "ways agent key remove --provider anthropic");
}

#[test]
fn queue_keeps_order_and_x_undoes_the_last() {
    let mut app = App::new("t", action_tree());
    // check, then remove (confirmed); undo drops remove, leaving check.
    keys(&mut app, &[KeyCode::Down, KeyCode::Char('a'), KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
    keys(&mut app, &[KeyCode::Char('a'), KeyCode::Down, KeyCode::Enter, KeyCode::Char('y')]);
    let cmds: Vec<_> = app.queue.items().iter().map(|q| q.label.as_str()).collect();
    assert_eq!(cmds, ["check", "remove"]);
    keys(&mut app, &[KeyCode::Char('x')]);
    assert_eq!(app.queue.items().len(), 1);
    assert_eq!(app.queue.items()[0].label, "check");
}

#[test]
fn action_only_node_opens_the_menu_on_enter() {
    let target = Node::leaf("/home/me/.claude", "", Setting::new(Kind::ReadOnly, "enabled", "user"))
        .with_actions(vec![Action::new("plan", "ways config target plan {}").arg(Arg::Text("dir".into()))]);
    let mut app = App::new("t", vec![Node::group("install", "", vec![target]).opened()]);
    keys(&mut app, &[KeyCode::Enter, KeyCode::Enter]);
    type_str(&mut app, "/tmp/a b");
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.queue.items()[0].command, "ways config target plan '/tmp/a b'");
}

fn flag(name: &str) -> Node {
    Node::leaf(name, "", Setting::new(Kind::Bool, "true", "default").default("true"))
}

/// Three tabs: nested ways, a number, and a node with an action.
fn tabbed() -> Vec<Node> {
    let ways = Node::group("ways", "", vec![flag("alpha"), flag("beta"), Node::group("deep", "", vec![flag("gamma")])]);
    let tau = |name| Node::leaf(name, "", Setting::new(Kind::Float { min: 0.0, max: 1.0 }, "0.5", "default").default("0.5"));
    let matching = Node::group("matching", "", vec![tau("tau_s"), tau("tau_k")]);
    let gate = Node::group("gate", "", vec![flag("language")]).with_actions(vec![Action::new("check", "ways agent status")]);
    vec![ways.opened(), matching.opened(), gate.opened()]
}

fn frame(app: &mut App, w: u16, h: u16) -> Vec<String> {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let buf = term.backend().buffer();
    buf.content().chunks(w as usize).map(|r| r.iter().map(|c| c.symbol()).collect()).collect()
}

#[test]
fn tab_bar_names_every_root_and_tab_keys_switch_the_tree() {
    let mut app = App::new("t", tabbed());
    let bar = frame(&mut app, 100, 24)[0].clone();
    assert!(["1 ways", "2 matching", "3 gate"].iter().all(|t| bar.contains(t)), "{bar}");
    assert!(screen(&mut app).contains("alpha") && !screen(&mut app).contains("tau_s"));
    keys(&mut app, &[KeyCode::Tab]);
    assert!(screen(&mut app).contains("tau_s") && !screen(&mut app).contains("alpha"));
    keys(&mut app, &[KeyCode::Char('3')]);
    assert!(screen(&mut app).contains("language"));
    keys(&mut app, &[KeyCode::BackTab, KeyCode::BackTab]);
    assert!(screen(&mut app).contains("alpha"));
    keys(&mut app, &[KeyCode::Char('9')]);
    assert!(screen(&mut app).contains("alpha"), "a digit past the last tab is ignored");
}

#[test]
fn each_tab_keeps_its_cursor_and_open_state() {
    let mut app = App::new("t", tabbed());
    // On ways: move the cursor to `deep` at row 2 and open it.
    keys(&mut app, &[KeyCode::Down, KeyCode::Down, KeyCode::Right]);
    assert_eq!(app.cursor, 2);
    assert!(screen(&mut app).contains("gamma"));
    keys(&mut app, &[KeyCode::Char('2'), KeyCode::Down]);
    assert_eq!(app.cursor, 1);
    keys(&mut app, &[KeyCode::Char('1')]);
    assert_eq!(app.cursor, 2);
    assert!(screen(&mut app).contains("gamma"), "open state kept");
    keys(&mut app, &[KeyCode::Char('2')]);
    assert_eq!(app.cursor, 1);
}

#[test]
fn filter_hit_in_another_tab_jumps_there_on_enter() {
    let mut app = App::new("t", tabbed());
    keys(&mut app, &[KeyCode::Char('/')]);
    type_str(&mut app, "language");
    let hits = screen(&mut app);
    assert!(hits.contains("gate") && hits.contains("language") && !hits.contains("alpha"));
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.filter, "language");
    keys(&mut app, &[KeyCode::Down, KeyCode::Enter]);
    assert!(app.filter.is_empty());
    assert_eq!(app.tab, 2);
    assert_eq!(tree::key(&app.roots, &app.rows()[app.cursor].path), "gate.language");
    // Esc from a filter returns to the tab's own cursor.
    keys(&mut app, &[KeyCode::Char('1'), KeyCode::Down, KeyCode::Char('/')]);
    type_str(&mut app, "tau");
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!((app.tab, app.cursor), (0, 1));
}

#[test]
fn pending_count_shows_on_the_edited_tab_only() {
    // The plain shape: the badge segment abuts the label, no glyphs between.
    let mut app = App::new("t", tabbed()).shape(theme::Shape::PLAIN);
    keys(&mut app, &[KeyCode::Down, KeyCode::Enter]);
    let bar = frame(&mut app, 100, 24)[0].clone();
    assert!(bar.contains("1 ways  ●1") && !bar.contains("matching  ●") && !bar.contains("gate  ●"), "{bar}");
    keys(&mut app, &[KeyCode::Char('3'), KeyCode::Char('a'), KeyCode::Enter]);
    let bar = frame(&mut app, 100, 24)[0].clone();
    assert!(bar.contains("3 gate  ●1"), "{bar}");
    keys(&mut app, &[KeyCode::Char('c')]);
    assert!(screen(&mut app).contains("pending (1 changes, 1 actions)"));
}

#[test]
fn minimum_size_fits_the_tab_bar_and_the_selected_row() {
    let mut app = App::new("t", tabbed());
    keys(&mut app, &[KeyCode::Down, KeyCode::Down]);
    let f = frame(&mut app, 80, 25);
    assert!(["1 ways", "2 matching", "3 gate"].iter().all(|t| f[0].contains(t)));
    assert!(f.iter().any(|l| l.contains("beta")), "{f:#?}");
    keys(&mut app, &[KeyCode::Char('?')]);
    frame(&mut app, 80, 25);
}

fn mouse_at(app: &mut App, kind: MouseEventKind, (x, y): (u16, u16)) {
    // Draw first, as the run loop does, so the hits are the frame's.
    frame(app, 110, 24);
    app.mouse(MouseEvent { kind, column: x, row: y, modifiers: KeyModifiers::NONE });
}

fn click(app: &mut App, at: (u16, u16)) {
    mouse_at(app, MouseEventKind::Down(MouseButton::Left), at);
}

/// Where `text` first appears on screen, as (column, row).
fn find(app: &mut App, text: &str) -> (u16, u16) {
    let f = frame(app, 110, 24);
    for (y, line) in f.iter().enumerate() {
        if let Some(b) = line.find(text) {
            return (line[..b].chars().count() as u16, y as u16);
        }
    }
    panic!("{text} not on screen:\n{}", f.join("\n"));
}

#[test]
fn mouse_clicks_a_tab_and_selects_then_activates_a_row() {
    let mut app = App::new("t", tabbed());
    let at = find(&mut app, "2 matching");
    click(&mut app, at);
    assert_eq!(app.tab, 1);
    let at = find(&mut app, "1 ways");
    click(&mut app, at);
    assert_eq!(app.tab, 0);

    let beta = find(&mut app, "beta");
    click(&mut app, beta);
    assert_eq!(app.cursor, 1);
    assert!(tree::changes(&app.roots).is_empty(), "the first click only selects");
    click(&mut app, beta);
    assert_eq!(tree::changes(&app.roots)[0].0, "ways.beta", "a second click acts as Enter");
}

#[test]
fn mouse_marker_toggles_a_group_and_the_wheel_moves_the_cursor() {
    let mut app = App::new("t", tabbed());
    let (x, y) = find(&mut app, "▸ deep");
    click(&mut app, (x, y));
    assert!(screen(&mut app).contains("gamma"), "a click on ▸ opens the group");
    assert_eq!(app.cursor, 2);
    click(&mut app, (x + 1, y));
    assert!(!screen(&mut app).contains("gamma"), "the marker's second column closes it again");

    mouse_at(&mut app, MouseEventKind::ScrollUp, (0, 0));
    mouse_at(&mut app, MouseEventKind::ScrollUp, (0, 0));
    assert_eq!(app.cursor, 0);
    mouse_at(&mut app, MouseEventKind::ScrollDown, (0, 0));
    assert_eq!(app.cursor, 1);
}

#[test]
fn mouse_picks_a_menu_item_and_a_click_outside_closes_it() {
    let mut app = App::new("t", action_tree());
    keys(&mut app, &[KeyCode::Down, KeyCode::Char('a')]);
    click(&mut app, (0, 23));
    assert!(matches!(app.mode, Mode::Browse) && app.queue.is_empty());
    keys(&mut app, &[KeyCode::Char('a')]);
    let at = find(&mut app, " check");
    click(&mut app, at);
    assert_eq!(app.queue.items()[0].label, "check");
}

#[test]
fn mouse_answers_a_confirm_on_its_targets_only() {
    let mut app = App::new("t", action_tree());
    keys(&mut app, &[KeyCode::Down, KeyCode::Char('a')]);
    let at = find(&mut app, " remove");
    click(&mut app, at);
    assert!(matches!(app.mode, Mode::Confirm { .. }));
    click(&mut app, (1, 5));
    assert!(matches!(app.mode, Mode::Confirm { .. }), "a click off the answers does nothing");
    let at = find(&mut app, "n cancel");
    click(&mut app, at);
    assert!(app.queue.is_empty() && matches!(app.mode, Mode::Browse));

    keys(&mut app, &[KeyCode::Char('a'), KeyCode::Down, KeyCode::Enter]);
    let at = find(&mut app, "y queue");
    click(&mut app, at);
    assert_eq!(app.queue.items()[0].command, "ways agent key remove --provider anthropic");
}

#[test]
fn mouse_during_secret_entry_neither_reveals_nor_queues() {
    const SECRET: &str = "Q#Z9@X!";
    let mut app = App::new("t", action_tree());
    keys(&mut app, &[KeyCode::Down, KeyCode::Enter]);
    type_str(&mut app, SECRET);
    // Every cell, clicked twice, and the wheel.
    for y in 0..24 {
        for x in 0..110 {
            click(&mut app, (x, y));
            click(&mut app, (x, y));
        }
    }
    mouse_at(&mut app, MouseEventKind::ScrollDown, (5, 5));
    let Mode::Secret { buf, .. } = &app.mode else { panic!("left secret entry") };
    assert_eq!(buf.len(), SECRET.chars().count());
    assert!(app.queue.is_empty());
    assert!(!screen(&mut app).chars().any(|c| SECRET.contains(c)), "typed characters reached the frame");
}

#[test]
fn the_look_follows_the_statusline_palette() {
    let mut app = themed(App::new("t", tabbed()).shape(theme::Shape::ROUND));
    keys(&mut app, &[KeyCode::Down, KeyCode::Enter]);
    let mut term = Terminal::new(TestBackend::new(80, 25)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let buf = term.backend().buffer();
    let row = |y: u16| (0..80).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>();
    let at = |text: &str, y: u16| {
        let r = row(y);
        &buf[(r[..r.find(text).unwrap_or_else(|| panic!("{text} not in {r}"))].chars().count() as u16, y)]
    };

    // The tab bar: a round cap, the shown tab on the accent, its count on a warn badge.
    assert_eq!(buf[(0, 0)].symbol(), theme::Shape::ROUND.cap);
    assert_eq!((at("1 ways", 0).fg, at("1 ways", 0).bg), (theme::Ground::Accent.fg(), theme::Ground::Accent.bg()));
    assert_eq!(at("●1", 0).bg, theme::Ground::Warn.bg());
    assert_eq!(at("2 matching", 0).bg, theme::Ground::AccentDim.bg());
    // The selected row keeps the changed value's colour over the accent shade.
    let y = (0..25).find(|&y| row(y).contains(theme::SELECTED_MARK)).unwrap();
    let v = at("false", y);
    assert_eq!((Some(v.fg), v.bg), (theme::warn().fg, theme::shade_color()));
    assert!(v.modifier.contains(Modifier::BOLD));
    // The status line: a mode lozenge, then the count in the changed style.
    assert_eq!(at("browse", 24).bg, theme::Ground::Accent.bg());
    assert_eq!(Some(at("●1 changed", 24).fg), theme::warn().fg);
}

#[test]
fn a_tab_starts_at_its_roots_children() {
    let mut app = App::new("t", tabbed());
    let tree = frame(&mut app, 100, 24);
    assert!(tree.iter().all(|l| !l.contains("▾ ways")), "the root has no row");
    assert_eq!(app.rows().len(), 3);
    assert_eq!(tree::key(&app.roots, &app.rows()[0].path), "ways.alpha");
}

#[test]
fn group_and_tab_badges_both_count_values_and_queued_actions() {
    let mut app = App::new("t", tabbed());
    // ways.deep.gamma changed, and a command queued under ways.deep.
    keys(&mut app, &[KeyCode::Down, KeyCode::Down, KeyCode::Right, KeyCode::Down, KeyCode::Enter]);
    app.queue.push(Queued::new("ways.deep", "x", "ways x", false));
    let f = frame(&mut app, 100, 24);
    let deep = f.iter().find(|l| l.contains("▾ deep")).unwrap();
    assert!(deep.contains("●2"), "{deep}");
    assert!(f[0].contains("●2"), "{}", f[0]);
}

/// Three tabs. `ways` has three bools in two files and a key node with
/// actions; `other` has one bool in a third file; `clean` has nothing to change.
fn pending_tree() -> Vec<Node> {
    let s = |file: &str, key: &str| Setting::new(Kind::Bool, "true", "user").default("true").store("user", file.into(), key);
    let key = action_tree().remove(0).children.remove(0);
    let ways = Node::group(
        "ways",
        "",
        vec![
            Node::leaf("alpha", "", s("/c/a.yaml", "alpha")),
            Node::leaf("beta", "", s("/c/a.yaml", "beta")),
            Node::leaf("gamma", "", s("/c/b.yaml", "gamma")),
            key,
        ],
    );
    let other = Node::group("other", "", vec![Node::leaf("delta", "", s("/c/c.yaml", "delta"))]);
    vec![ways.opened(), other.opened(), Node::group("clean", "", vec![flag("zeta")]).opened()]
}

/// In `ways`: alpha, beta and gamma toggled; `check` queued, then `remove`
/// (confirmed). In `other`: delta toggled. Ends on `ways`: 5 and 1 pending.
fn pending_app() -> App {
    let mut app = App::new("t", pending_tree()).adapter(Dry::default());
    keys(&mut app, &[KeyCode::Enter, KeyCode::Down, KeyCode::Enter, KeyCode::Down, KeyCode::Enter, KeyCode::Down]);
    keys(&mut app, &[KeyCode::Char('a'), KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
    keys(&mut app, &[KeyCode::Char('a'), KeyCode::Down, KeyCode::Enter, KeyCode::Char('y')]);
    keys(&mut app, &[KeyCode::Char('2'), KeyCode::Enter, KeyCode::Char('1')]);
    app
}

/// Tick until the run has ended and review has closed it out.
fn tick_out(app: &mut App) {
    for _ in 0..60 {
        if !matches!(app.mode, Mode::Review { run: Some(_), .. }) {
            return;
        }
        app.tick();
    }
    panic!("the run never ended");
}

/// Tick until the run has ended, but not past it: the stopped or finished
/// frame is still on screen.
fn tick_to_end(app: &mut App) {
    for _ in 0..60 {
        if matches!(&app.mode, Mode::Review { run: Some(r), .. } if r.finished()) {
            return;
        }
        app.tick();
    }
    panic!("the run never ended");
}

fn bottom(app: &mut App, w: u16, h: u16) -> String {
    frame(app, w, h)[h as usize - 1].clone()
}

fn delta_changed(app: &App) -> bool {
    app.roots[1].children[0].setting.as_ref().unwrap().changed()
}

fn review_tab(app: &App) -> Option<usize> {
    match &app.mode {
        Mode::Review { tab, .. } => Some(*tab),
        _ => None,
    }
}

/// The tree rows of the screen's left pane and its detail pane, as text.
fn panes(app: &mut App) -> (String, String) {
    let f = frame(app, 110, 24);
    let split = 61;
    let cut = |from: usize, to: usize| f[1..23].iter().map(|l| l.chars().skip(from).take(to - from).collect::<String>()).collect::<Vec<_>>().join("\n");
    (cut(0, split), cut(split, 110))
}

#[test]
fn the_call_to_action_shows_while_the_current_tab_has_pending_items() {
    let mut app = App::new("t", pending_tree());
    assert!(!bottom(&mut app, 100, 24).contains("unsaved"));
    keys(&mut app, &[KeyCode::Enter]);
    assert!(bottom(&mut app, 100, 24).contains("● 1 unsaved in ways · w review & apply"));
    keys(&mut app, &[KeyCode::Enter]);
    assert!(!bottom(&mut app, 100, 24).contains("unsaved"), "reverting the value clears it");
    app.queue.push(Queued::new("ways.anthropic", "x", "ways x", false));
    assert!(bottom(&mut app, 100, 24).contains("● 1 unsaved in ways"), "a queued action alone counts");
    app.queue.clear();
    assert!(!bottom(&mut app, 100, 24).contains("unsaved"));
    app.queue.push(Queued::new("other", "x", "ways x", false));
    assert!(!bottom(&mut app, 100, 24).contains("unsaved"), "pending in another tab is shown on its badge only");
    assert!(frame(&mut app, 100, 24)[0].contains("●1 ↺"));
    keys(&mut app, &[KeyCode::Char('2')]);
    assert!(bottom(&mut app, 100, 24).contains("● 1 unsaved in other"));
}

#[test]
fn the_call_to_action_is_bold_hot_and_clicking_it_enters_review() {
    let mut app = themed(pending_app());
    let (x, y) = find(&mut app, "● 5 unsaved in ways");
    let mut term = Terminal::new(TestBackend::new(110, 24)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let c = &term.backend().buffer()[(x + 2, y)];
    assert_eq!(c.bg, theme::Ground::Hot.bg());
    assert!(c.modifier.contains(Modifier::BOLD));
    click(&mut app, (x + 3, y));
    assert_eq!(review_tab(&app), Some(0));
    let mut app = pending_app();
    keys(&mut app, &[KeyCode::Char('w')]);
    assert_eq!(review_tab(&app), Some(0));
    let mut app = pending_app();
    assert!(app.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)));
    assert!(matches!(app.mode, Mode::Review { .. }));
    let mut app = App::new("t", pending_tree());
    keys(&mut app, &[KeyCode::Char('w')]);
    assert!(matches!(app.mode, Mode::Browse), "nothing to review");
}

#[test]
fn review_keeps_the_browsers_layout_and_shows_only_what_is_pending() {
    let mut app = pending_app();
    keys(&mut app, &[KeyCode::Char('w')]);
    let f = frame(&mut app, 100, 30);
    assert!(["1 ways", "2 other", "3 clean"].iter().all(|t| f[0].contains(t)), "the tab bar stays: {}", f[0]);
    assert!(f[1].contains("review"), "the tree pane is titled review: {}", f[1]);
    assert!(f.iter().any(|l| l.contains("detail")), "the detail pane stays");
    let bar = &f[29];
    assert!(bar.contains("review") && !bar.contains("browse"), "{bar}");
    assert!(bar.contains("a apply ways · X discard ways · Esc back") || ["a apply ways", "X discard ways", "Esc back"].iter().all(|t| bar.contains(t)), "{bar}");
    let (tree, _) = panes(&mut app);
    let at = |t: &str| tree.find(t).unwrap_or_else(|| panic!("{t} missing:\n{tree}"));
    assert!(at("alpha") < at("beta") && at("beta") < at("gamma") && at("gamma") < at("queued"));
    assert!(at("queued") < at("1. $ ways agent key check") && at("1. $ ways agent key check") < at("2. $ ways agent key remove ") + 3);
    let remove = tree.lines().find(|l| l.contains("key remove")).unwrap();
    assert!(remove.contains('▲') && !tree.lines().find(|l| l.contains("key check")).unwrap().contains('▲'));
    assert!(!tree.contains("delta") && !tree.contains("a.yaml") && tree.lines().filter(|l| l.contains("anthropic")).all(|l| l.contains('$')), "unchanged and other tabs' rows stay out:\n{tree}");
    assert!(tree.contains("true → false"));
}

#[test]
fn a_group_over_a_change_is_listed_with_its_ancestors_open_and_toggles_on_enter() {
    let mut app = App::new("t", tabbed());
    keys(&mut app, &[KeyCode::Down, KeyCode::Down, KeyCode::Right, KeyCode::Down, KeyCode::Enter]);
    keys(&mut app, &[KeyCode::Char('w')]);
    let (tree, detail) = panes(&mut app);
    assert!(tree.contains("▾ deep") && tree.contains("gamma") && !tree.contains("alpha"), "{tree}");
    keys(&mut app, &[KeyCode::Enter]);
    assert!(!panes(&mut app).0.contains("gamma"), "Enter on a group closes it");
    assert!(tree.contains("●1") && detail.contains("1 change under this group"), "{detail}");
    keys(&mut app, &[KeyCode::Right, KeyCode::Right]);
    assert!(panes(&mut app).0.contains("gamma"), "→ opens it, and a second → steps in");
    assert_eq!(app.rcursor[0], 1);
    assert_eq!(tree::changes(&app.roots).len(), 1, "toggling a group changes no value");
}

#[test]
fn tab_and_digits_skip_clean_tabs_which_are_dimmed_but_clickable() {
    let mut app = themed(pending_app());
    keys(&mut app, &[KeyCode::Char('w')]);
    let mut term = Terminal::new(TestBackend::new(110, 24)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let buf = term.backend().buffer();
    let bar: String = (0..110).map(|x| buf[(x, 0)].symbol().to_string()).collect();
    let at = |t: &str| &buf[(bar[..bar.find(t).unwrap()].chars().count() as u16, 0)];
    assert_eq!((Some(at("3 clean").fg), at("3 clean").bg), (theme::muted().fg, theme::shade_color()), "a clean tab is dimmed");
    assert_eq!(at("2 other").bg, theme::Ground::AccentDim.bg());
    assert_eq!(at("1 ways").bg, theme::Ground::Accent.bg());

    keys(&mut app, &[KeyCode::Tab]);
    assert_eq!(review_tab(&app), Some(1));
    keys(&mut app, &[KeyCode::Tab]);
    assert_eq!(review_tab(&app), Some(0), "Tab skips the clean tab");
    keys(&mut app, &[KeyCode::BackTab]);
    assert_eq!(review_tab(&app), Some(1), "so does Shift-Tab");
    keys(&mut app, &[KeyCode::Char('3')]);
    assert_eq!(review_tab(&app), Some(1), "a digit onto a clean tab does nothing");
    assert!(bottom(&mut app, 110, 24).contains("nothing pending in clean"));
    keys(&mut app, &[KeyCode::Char('1')]);
    assert_eq!(review_tab(&app), Some(0));

    let at = find(&mut app, "3 clean");
    click(&mut app, at);
    assert_eq!(review_tab(&app), Some(2));
    let (tree, detail) = panes(&mut app);
    assert!(tree.contains("nothing pending in clean") && detail.contains("nothing pending in clean"), "{tree}");
    assert!(matches!(app.mode, Mode::Review { .. }) && app.pending_in(0) == 5);
}

#[test]
fn each_tab_keeps_its_own_review_cursor() {
    let mut app = pending_app();
    keys(&mut app, &[KeyCode::Char('w'), KeyCode::Down, KeyCode::Down]);
    keys(&mut app, &[KeyCode::Tab, KeyCode::Tab]);
    assert_eq!((review_tab(&app), app.rcursor[0]), (Some(0), 2));
    keys(&mut app, &[KeyCode::Char('G')]);
    assert_eq!(app.rcursor[0], 5);
    keys(&mut app, &[KeyCode::Tab, KeyCode::Tab]);
    assert_eq!(app.rcursor[0], 5);
}

#[test]
fn editing_keys_do_nothing_in_review_and_say_so() {
    let mut app = pending_app();
    keys(&mut app, &[KeyCode::Char('w')]);
    let values = |app: &App| tree::changes(&app.roots).iter().map(|c| format!("{}={}", c.0, c.3)).collect::<Vec<_>>();
    let (before, queued) = (values(&app), app.queue.items().iter().map(|q| q.command.clone()).collect::<Vec<_>>());
    // Every row in turn: a value, then the group, then the commands.
    for row in 0..6 {
        app.rcursor[0] = row;
        for c in ['e', 'd', 'u', 'x', 'c', 'w', 'z', '5'] {
            if c == '5' {
                keys(&mut app, &[KeyCode::Backspace, KeyCode::Char('5')]);
            } else {
                keys(&mut app, &[KeyCode::Char(c)]);
            }
        }
        if row != 3 {
            keys(&mut app, &[KeyCode::Enter, KeyCode::Char(' ')]);
            assert!(bottom(&mut app, 110, 24).contains("read-only in review; Esc to edit"), "row {row}");
        }
        assert!(matches!(app.mode, Mode::Review { run: None, discard: false, .. }), "row {row}");
        assert_eq!(values(&app), before, "row {row}");
        assert_eq!(app.queue.items().iter().map(|q| q.command.clone()).collect::<Vec<_>>(), queued, "row {row}");
    }
    // The group is the one thing Enter does: it hides its commands.
    app.rcursor[0] = 3;
    keys(&mut app, &[KeyCode::Enter]);
    assert!(!panes(&mut app).0.contains("key check"));
    assert_eq!((values(&app), app.queue.len()), (before, 2));
}

#[test]
fn detail_for_a_setting_shows_was_will_be_layers_file_and_key() {
    let mut app = themed(pending_app());
    keys(&mut app, &[KeyCode::Char('w'), KeyCode::Down]);
    let (_, detail) = panes(&mut app);
    for t in ["ways.beta", "change", "true → false", "from      user layer", "to        user layer", "file      /c/a.yaml", "key       beta"] {
        assert!(detail.contains(t), "{t} missing:\n{detail}");
    }
    let (x, y) = find(&mut app, "change    true");
    let mut term = Terminal::new(TestBackend::new(110, 24)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let buf = term.backend().buffer();
    let (was, will) = (&buf[(x + 10, y)], &buf[(x + 10 + 7, y)]);
    assert!(was.modifier.contains(Modifier::CROSSED_OUT) && Some(was.fg) == theme::muted().fg, "the old value is struck and muted");
    assert!(will.modifier.contains(Modifier::BOLD) && Some(will.fg) == theme::ok().fg, "the new value is green and bold");
}

#[test]
fn detail_for_a_group_counts_its_changes_and_names_the_files() {
    let mut app = App::new("t", pending_tree());
    // ways.deep holds a change in another file.
    app.roots[0].children.push(Node::group("deep", "", vec![Node::leaf("eps", "", Setting::new(Kind::Bool, "false", "user").store("project", "/p/x.yaml".into(), "eps"))]));
    app.roots[0].children[4].children[0].setting.as_mut().unwrap().value = "true".into();
    app.roots[0].children[4].open = true;
    keys(&mut app, &[KeyCode::Char('w'), KeyCode::Char('G')]);
    app.rcursor[0] = app.review_view(0).iter().position(|r| r.name == "deep").unwrap();
    let (_, detail) = panes(&mut app);
    assert!(detail.contains("1 change under this group") && detail.contains("file it writes") && detail.contains("/p/x.yaml"), "{detail}");
    assert!(!detail.contains("/c/a.yaml"), "only the files under it:\n{detail}");
}

#[test]
fn detail_for_an_action_shows_the_command_with_stdin_for_a_secret_and_never_the_typed_text() {
    const SECRET: &str = "ß#¤@%^";
    let mut app = App::new("t", action_tree());
    keys(&mut app, &[KeyCode::Enter]);
    type_str(&mut app, SECRET);
    keys(&mut app, &[KeyCode::Enter]);
    // Queue a confirmed action too.
    keys(&mut app, &[KeyCode::Char('a'), KeyCode::Down, KeyCode::Enter, KeyCode::Char('y')]);
    keys(&mut app, &[KeyCode::Char('w'), KeyCode::Down]);
    let (_, detail) = panes(&mut app);
    for t in ["$ ways agent key add --provider anthropic", "<stdin>", "queued by", "keys.anthropic", "asks      no"] {
        assert!(detail.contains(t), "{t} missing:\n{detail}");
    }
    keys(&mut app, &[KeyCode::Down]);
    let (_, detail) = panes(&mut app);
    assert!(detail.contains("$ ways agent key remove") && detail.contains("asks      yes: it is destructive or reconciles"), "{detail}");
    let all = frame(&mut app, 110, 24).join("\n");
    assert!(!all.chars().any(|c| SECRET.contains(c)), "typed characters reached the review");
    assert!(!format!("{:?}", app.queue).contains(SECRET));
}

#[test]
fn an_action_detail_carries_its_doc_and_the_files_it_touches() {
    let a = Action::new("remove", "ways x remove").confirm().doc("Deletes the thing.").touches("/keys/x");
    let node = Node::leaf("x", "", Setting::new(Kind::ReadOnly, "on", "user")).with_actions(vec![a.clone()]);
    let mut app = App::new("t", vec![Node::group("g", "", vec![node]).opened()]);
    app.queue.push(Queued::new("g.x", a.label.clone(), a.render(""), true));
    keys(&mut app, &[KeyCode::Char('w'), KeyCode::Down]);
    let (_, detail) = panes(&mut app);
    assert!(detail.contains("Deletes the thing.") && detail.contains("touches   /keys/x"), "{detail}");
}

#[test]
fn discard_in_review_asks_first_and_takes_only_its_tab_then_review_moves_on() {
    let mut app = pending_app();
    keys(&mut app, &[KeyCode::Char('w'), KeyCode::Char('X')]);
    assert!(bottom(&mut app, 100, 24).contains("discard 5 pending in ways?"));
    keys(&mut app, &[KeyCode::Char('x'), KeyCode::Enter]);
    assert_eq!(app.pending_in(0), 5, "other keys do not answer");
    keys(&mut app, &[KeyCode::Char('n')]);
    assert!(matches!(app.mode, Mode::Review { discard: false, .. }) && app.pending_in(0) == 5);
    keys(&mut app, &[KeyCode::Char('X'), KeyCode::Char('y')]);
    assert_eq!((app.pending_in(0), app.pending_in(1)), (0, 1));
    assert_eq!(review_tab(&app), Some(1), "the next tab with pending");
    assert!(delta_changed(&app));
    keys(&mut app, &[KeyCode::Char('X'), KeyCode::Char('y')]);
    assert!(matches!(app.mode, Mode::Browse) && app.pending() == 0, "nothing left ends review");
}

#[test]
fn the_bar_targets_are_clickable() {
    let mut app = pending_app();
    keys(&mut app, &[KeyCode::Char('w')]);
    let at = find(&mut app, "X discard ways");
    click(&mut app, at);
    assert!(matches!(app.mode, Mode::Review { discard: true, .. }));
    let at = find(&mut app, "n keep");
    click(&mut app, at);
    assert!(matches!(app.mode, Mode::Review { discard: false, .. }) && app.pending_in(0) == 5);
    let at = find(&mut app, "a apply ways");
    click(&mut app, at);
    assert!(matches!(app.mode, Mode::Review { run: Some(_), .. }));
    tick_out(&mut app);
    assert_eq!(app.pending_in(0), 0);

    let mut app = pending_app();
    keys(&mut app, &[KeyCode::Char('w')]);
    let at = find(&mut app, "X discard ways");
    click(&mut app, at);
    let at = find(&mut app, "y discard");
    click(&mut app, at);
    assert_eq!((app.pending_in(0), review_tab(&app)), (0, Some(1)));

    let mut app = pending_app();
    keys(&mut app, &[KeyCode::Char('w')]);
    let at = find(&mut app, "Esc back");
    click(&mut app, at);
    assert!(matches!(app.mode, Mode::Browse));
}

#[test]
fn clicks_and_the_wheel_move_the_review_cursor_without_editing() {
    let mut app = pending_app();
    keys(&mut app, &[KeyCode::Char('w')]);
    let before = tree::changes(&app.roots).len();
    let at = find(&mut app, "gamma");
    click(&mut app, at);
    assert_eq!(app.rcursor[0], 2, "a click on a row moves the cursor");
    click(&mut app, at);
    assert_eq!(tree::changes(&app.roots).len(), before, "a second click on a value edits nothing");
    assert!(bottom(&mut app, 110, 24).contains("read-only in review"));
    let at = find(&mut app, "1. $ ways agent key check");
    click(&mut app, at);
    assert_eq!(app.rcursor[0], 4);
    assert_eq!(app.queue.len(), 2);
    // The marker of the `queued` group closes it.
    let (x, y) = find(&mut app, "▾ queued");
    click(&mut app, (x, y));
    assert!(!panes(&mut app).0.contains("key check"));
    click(&mut app, (x + 1, y));
    assert!(panes(&mut app).0.contains("key check"));
    mouse_at(&mut app, MouseEventKind::ScrollUp, (0, 0));
    mouse_at(&mut app, MouseEventKind::ScrollUp, (0, 0));
    assert_eq!(app.rcursor[0], 1, "the marker click put the cursor on the group, row 3");
    mouse_at(&mut app, MouseEventKind::ScrollDown, (0, 0));
    assert_eq!(app.rcursor[0], 2);
    assert_eq!((tree::changes(&app.roots).len(), app.queue.len()), (before, 2));
}

#[test]
fn esc_returns_to_browse_with_the_browse_cursor_where_it_was() {
    let mut app = pending_app();
    keys(&mut app, &[KeyCode::Down, KeyCode::Down]);
    let at = (app.tab, app.cursor);
    keys(&mut app, &[KeyCode::Char('w'), KeyCode::Down, KeyCode::Down, KeyCode::Down, KeyCode::Tab]);
    keys(&mut app, &[KeyCode::Esc]);
    assert!(matches!(app.mode, Mode::Browse));
    assert_eq!((app.tab, app.cursor), at);
    assert!(bottom(&mut app, 100, 24).contains("browse"));
    assert_eq!(app.pending(), 6, "nothing was applied or dropped");
}

#[test]
fn apply_ticks_gutter_glyphs_in_place_and_clears_only_the_current_tab() {
    let mut app = pending_app();
    keys(&mut app, &[KeyCode::Char('w'), KeyCode::Char('a')]);
    let tree = |app: &mut App| panes(app).0;
    let t = tree(&mut app);
    assert!(t.contains("·   alpha") && t.contains("·   gamma") && t.contains("·     1. $ ways agent key check"), "{t}");
    app.tick();
    let (t, detail) = panes(&mut app);
    assert!(t.contains("◐   alpha") && t.contains("◐   beta") && t.contains("·   gamma"), "{t}");
    assert!(detail.contains("step      write /c/a.yaml (2 keys)") && detail.contains("ways.alpha"), "the detail follows the running row:\n{detail}");
    app.tick();
    let t = tree(&mut app);
    assert!(t.contains("✓   alpha") && t.contains("✓   beta") && t.contains("·   gamma"), "a finished row stays, marked:\n{t}");
    assert_eq!(app.pending_in(0), 3, "a finished write is applied at once");
    assert!(bottom(&mut app, 100, 24).contains("applying"));
    tick_to_end(&mut app);
    let t = tree(&mut app);
    assert!(["✓   alpha", "✓   gamma", "✓     1. $ ways agent key check", "✓     2. $ ways agent key remove"].iter().all(|g| t.contains(g)), "{t}");
    app.tick();
    assert_eq!((app.pending_in(0), app.pending_in(1)), (0, 1));
    assert!(delta_changed(&app));
    assert_eq!(review_tab(&app), Some(1), "review moves to the next tab with pending");
    let (t, _) = panes(&mut app);
    assert!(t.contains("delta") && !t.contains("alpha"), "{t}");
    assert!(bottom(&mut app, 100, 24).contains("applied 5 in ways"));

    keys(&mut app, &[KeyCode::Char('a')]);
    tick_out(&mut app);
    assert!(matches!(app.mode, Mode::Browse) && app.pending() == 0, "nothing else pending ends review");
    assert!(bottom(&mut app, 100, 24).contains("applied 1 in other"));
}

#[test]
fn apply_writes_files_first_then_commands_in_the_steps() {
    let mut app = pending_app();
    keys(&mut app, &[KeyCode::Char('w'), KeyCode::Char('a')]);
    let Mode::Review { run: Some(r), .. } = &app.mode else { panic!("not applying") };
    assert_eq!(
        r.steps.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(),
        [
            "write /c/a.yaml (2 keys)",
            "write /c/b.yaml (1 key)",
            "run ways agent key check --provider anthropic",
            "run ways agent key remove --provider anthropic",
        ]
    );
    keys(&mut app, &[KeyCode::Char('x'), KeyCode::Char('X'), KeyCode::Esc]);
    assert!(matches!(app.mode, Mode::Review { run: Some(_), discard: false, .. }), "a run in flight takes no keys");
}

#[test]
fn a_failing_write_step_keeps_it_and_everything_after_with_the_failed_row_marked() {
    let mut app = pending_app().adapter(Dry::failing(2));
    keys(&mut app, &[KeyCode::Char('w'), KeyCode::Char('a')]);
    tick_to_end(&mut app);
    let (t, detail) = panes(&mut app);
    assert!(t.contains("✓   alpha") && t.contains("✗   gamma") && t.contains("·     1. $ ways agent key check"), "{t}");
    assert!(detail.contains("error") && detail.contains("planted failure at step 2") && detail.contains("write /c/b.yaml (1 key)"), "{detail}");
    assert!(bottom(&mut app, 100, 24).contains("stopped"));
    app.tick();
    assert_eq!(review_tab(&app), Some(0), "review stays on the tab");
    let (t, detail) = panes(&mut app);
    assert!(t.contains("✗   gamma") && t.contains("·     1. $ ways agent key check") && t.contains("·     2. $ ways agent key remove"), "{t}");
    assert!(!t.contains("alpha") && !t.contains("beta"), "applied rows are cleared:\n{t}");
    assert!(detail.contains("planted failure at step 2"), "the cursor sits on the failed row:\n{detail}");
    assert!(
        app.msg.contains("wrote /c/a.yaml; stopped at step 2 of 4: planted failure at step 2; 3 still pending in ways"),
        "the message names what was written before the stop: {}",
        app.msg
    );
    assert_eq!((app.pending_in(0), app.queue.len()), (3, 2));
    assert!(app.roots[0].children[2].setting.as_ref().unwrap().changed() && !app.roots[0].children[0].setting.as_ref().unwrap().changed());
    keys(&mut app, &[KeyCode::Esc]);
    assert!(matches!(app.mode, Mode::Browse) && app.failure.is_none());
}

#[test]
fn a_failing_command_step_keeps_it_and_the_commands_after_it() {
    let mut app = pending_app().adapter(Dry::failing(3));
    keys(&mut app, &[KeyCode::Char('w'), KeyCode::Char('a')]);
    tick_out(&mut app);
    assert_eq!(app.roots[0].changes(), 0, "both writes landed");
    assert_eq!(app.queue.len(), 2, "the failed command and the one after stay queued");
    let (t, _) = panes(&mut app);
    assert!(t.contains("✗     1. $ ways agent key check") && t.contains("·     2. $ ways agent key remove") && !t.contains("alpha"), "{t}");
}

#[test]
fn quit_with_pending_in_any_tab_lists_each_and_discarding_needs_a_second_confirm() {
    let mut app = App::new("t", pending_tree());
    assert!(!app.key(press(KeyCode::Char('q'))), "nothing pending quits");
    let mut app = pending_app();
    keys(&mut app, &[KeyCode::Char('2')]);
    assert!(app.key(press(KeyCode::Char('q'))));
    let f = frame(&mut app, 100, 24).join("\n");
    for t in ["6 unsaved in 2 tabs", "ways", "●5", "other", "●1", "Back (Esc)", "Review ways (r)", "Quit and discard all (D)"] {
        assert!(f.contains(t), "{t} missing:\n{f}");
    }
    keys(&mut app, &[KeyCode::Esc]);
    assert!(matches!(app.mode, Mode::Browse) && app.pending() == 6);
    assert!(app.key(press(KeyCode::Esc)) && matches!(app.mode, Mode::Guard { .. }), "Esc asks too");
    let at = find(&mut app, "Back (Esc)");
    click(&mut app, at);
    assert!(matches!(app.mode, Mode::Browse));

    keys(&mut app, &[KeyCode::Char('q'), KeyCode::Char('r')]);
    assert!(review_tab(&app) == Some(0) && app.tab == 0, "Review enters review mode on the first tab with pending");
    assert!(bottom(&mut app, 100, 24).contains("review"));
    let at = find(&mut app, "Esc back");
    click(&mut app, at);
    keys(&mut app, &[KeyCode::Char('q')]);
    let at = find(&mut app, "Review ways (r)");
    click(&mut app, at);
    assert_eq!(review_tab(&app), Some(0), "a click on Review does the same");

    let mut app = pending_app();
    keys(&mut app, &[KeyCode::Char('q'), KeyCode::Char('D')]);
    assert!(matches!(app.mode, Mode::Guard { confirm: true }) && app.pending() == 6, "one D discards nothing");
    assert!(bottom(&mut app, 100, 24).contains("quit and discard all 6 pending?"));
    keys(&mut app, &[KeyCode::Esc]);
    assert!(matches!(app.mode, Mode::Guard { confirm: false }));
    keys(&mut app, &[KeyCode::Char('D')]);
    assert!(!app.key(press(KeyCode::Char('y'))), "the second confirm quits");
    assert_eq!(app.pending(), 0);
}

#[test]
fn ctrl_c_meets_the_guard_when_anything_is_pending() {
    let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    let mut app = App::new("t", pending_tree());
    assert!(!app.key(ctrl_c), "nothing pending quits");
    let mut app = pending_app();
    keys(&mut app, &[KeyCode::Char('2')]);
    assert!(app.key(ctrl_c) && matches!(app.mode, Mode::Guard { confirm: false }));
    assert!(app.key(ctrl_c), "a second ^C does not quit past the prompt");
    let at = find(&mut app, "Quit and discard all (D)");
    click(&mut app, at);
    let at = find(&mut app, "y quit");
    assert_eq!(app.pending(), 6);
    click(&mut app, at);
    assert_eq!(app.pending(), 0);

    let mut app = pending_app();
    keys(&mut app, &[KeyCode::Char('w'), KeyCode::Char('a')]);
    assert!(app.key(ctrl_c) && matches!(app.mode, Mode::Review { run: Some(_), .. }), "a running apply is not interrupted");
}

#[test]
fn a_typed_secret_never_reaches_review_or_its_progress() {
    const SECRET: &str = "ß#¤@%^";
    let mut app = App::new("t", action_tree());
    keys(&mut app, &[KeyCode::Enter]);
    type_str(&mut app, SECRET);
    keys(&mut app, &[KeyCode::Enter, KeyCode::Char('w')]);
    let review = frame(&mut app, 100, 30).join("\n");
    assert!(review.contains("< <stdin>") && !review.chars().any(|c| SECRET.contains(c)), "{review}");
    keys(&mut app, &[KeyCode::Char('a')]);
    for _ in 0..4 {
        let f = frame(&mut app, 100, 30).join("\n");
        assert!(f.contains("<stdin>") && !f.chars().any(|c| SECRET.contains(c)), "{f}");
        app.tick();
    }
    assert!(!format!("{:?}", app.queue).contains(SECRET));
}

#[test]
fn a_modal_as_tall_as_the_pane_leaves_nothing_of_the_trees_title_beside_its_corner() {
    for (w, h) in [(80, 25), (80, 24), (100, 30), (110, 24)] {
        for open in [vec![KeyCode::Char('?')], vec![KeyCode::Char('q')], vec![KeyCode::Down, KeyCode::Char('a')]] {
            let mut app = pending_app();
            app.title = " ways settings — /home/aaron/Projects/ai/harness/agent-ways ".into();
            keys(&mut app, &open);
            let f = frame(&mut app, w, h);
            // The tree's title sits on the second row; a modal whose top edge is there starts at column 0.
            for tag in ["┌keys", "┌actions", "┌quit"] {
                if let Some(at) = f[1].find(tag) {
                    assert_eq!(f[1][..at].chars().count(), 0, "{w}x{h}: {}", f[1]);
                }
            }
        }
    }
}

#[test]
fn m_toggles_mouse_capture_and_the_status_line_says_so() {
    let mut app = App::new("t", tabbed());
    assert!(app.mouse && screen(&mut app).contains("mouse on"));
    keys(&mut app, &[KeyCode::Char('m')]);
    assert!(!app.mouse && screen(&mut app).contains("mouse off"));
}
