//! The generic TUI over a settings tree: one tab per root, then browse,
//! filter, edit, run actions, review. Key handling and state live here; drawing is in `render`.

mod render;

use std::io;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::widgets::ListState;
use ratatui::DefaultTerminal;

use crate::tree::{self, Arg, Kind, Node, Queue, Queued, Row, SecretBuf};

enum Mode {
    Browse,
    Edit(String),
    Filter,
    Help,
    /// Choosing among the actions of the node at `path`.
    Menu { path: Vec<usize>, sel: usize },
    /// A visible argument, such as a path, for action `action` of the node.
    Arg { path: Vec<usize>, action: usize, buf: String },
    /// A masked argument for action `action` of the node.
    Secret { path: Vec<usize>, action: usize, buf: SecretBuf },
    /// y/n before the action is queued.
    Confirm { queued: Queued },
}

/// What a session leaves behind: the edited tree and the queued actions.
pub struct Session {
    pub roots: Vec<Node>,
    pub queue: Queue,
}

pub struct App {
    pub roots: Vec<Node>,
    queue: Queue,
    title: String,
    /// The tab shown: an index into `roots`.
    tab: usize,
    /// The cursor in the rows on screen: the tab's, or the filter's.
    cursor: usize,
    /// Each tab's cursor while another tab, or a filter, is shown.
    saved: Vec<usize>,
    mode: Mode,
    filter: String,
    msg: String,
    show_changes: bool,
    list: ListState,
}

impl App {
    pub fn new(title: impl Into<String>, roots: Vec<Node>) -> Self {
        App {
            saved: vec![0; roots.len()],
            roots,
            queue: Queue::default(),
            title: title.into(),
            tab: 0,
            cursor: 0,
            mode: Mode::Browse,
            filter: String::new(),
            msg: "? keys · Tab 1-9 tabs · / filters all tabs".into(),
            show_changes: false,
            list: ListState::default(),
        }
    }

    pub fn run(mut self, term: &mut DefaultTerminal) -> io::Result<Session> {
        loop {
            term.draw(|f| self.draw(f))?;
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press && !self.key(k) {
                    return Ok(Session { roots: self.roots, queue: self.queue });
                }
            }
        }
    }

    /// The tab's rows, or with a filter the matches of every tab.
    fn rows(&self) -> Vec<Row> {
        if self.filter.is_empty() {
            tree::tab_rows(&self.roots, self.tab)
        } else {
            tree::rows(&self.roots, &self.filter)
        }
    }

    fn switch_tab(&mut self, to: usize) {
        if !self.filter.is_empty() {
            self.msg = "Esc clears the filter before switching tabs".into();
            return;
        }
        self.saved[self.tab] = self.cursor;
        self.tab = to;
        self.cursor = self.saved[to];
    }

    fn clear_filter(&mut self) {
        if !self.filter.is_empty() {
            self.filter.clear();
            self.cursor = self.saved[self.tab];
        }
    }

    /// Show the node at `path` in its own tab, opening what hides it.
    fn jump(&mut self, path: &[usize]) {
        self.filter.clear();
        for i in 1..path.len() {
            tree::get_mut(&mut self.roots, &path[..i]).open = true;
        }
        self.tab = path[0];
        self.cursor = self.rows().iter().position(|r| r.path == path).unwrap_or(0);
        self.saved[self.tab] = self.cursor;
        self.msg = tree::key(&self.roots, path);
    }

    /// Handle one key. False ends the session.
    fn key(&mut self, k: KeyEvent) -> bool {
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            return false;
        }
        match std::mem::replace(&mut self.mode, Mode::Browse) {
            Mode::Help => {}
            Mode::Filter => match k.code {
                KeyCode::Esc => self.clear_filter(),
                KeyCode::Enter => {}
                KeyCode::Backspace => {
                    self.filter.pop();
                    if self.filter.is_empty() {
                        self.cursor = self.saved[self.tab];
                    }
                    self.mode = Mode::Filter;
                }
                KeyCode::Char(c) => {
                    if self.filter.is_empty() {
                        self.saved[self.tab] = self.cursor;
                    }
                    self.filter.push(c);
                    self.cursor = 0;
                    self.mode = Mode::Filter;
                }
                _ => self.mode = Mode::Filter,
            },
            Mode::Edit(mut buf) => match k.code {
                KeyCode::Esc => self.msg = "edit cancelled".into(),
                KeyCode::Enter => self.commit(&buf),
                KeyCode::Backspace => {
                    buf.pop();
                    self.mode = Mode::Edit(buf);
                }
                KeyCode::Char(c) => {
                    buf.push(c);
                    self.mode = Mode::Edit(buf);
                }
                _ => self.mode = Mode::Edit(buf),
            },
            Mode::Menu { path, sel } => {
                let len = tree::get(&self.roots, &path).actions.len();
                match k.code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('a') => {}
                    KeyCode::Enter => self.pick(path, sel),
                    KeyCode::Up | KeyCode::Char('k') => self.mode = Mode::Menu { path, sel: sel.saturating_sub(1) },
                    KeyCode::Down | KeyCode::Char('j') => self.mode = Mode::Menu { path, sel: (sel + 1).min(len - 1) },
                    _ => self.mode = Mode::Menu { path, sel },
                }
            }
            Mode::Arg { path, action, mut buf } => match k.code {
                KeyCode::Esc => self.msg = "cancelled".into(),
                KeyCode::Enter if buf.trim().is_empty() => self.mode = Mode::Arg { path, action, buf },
                KeyCode::Enter => self.stage(&path, action, buf.trim()),
                KeyCode::Backspace => {
                    buf.pop();
                    self.mode = Mode::Arg { path, action, buf };
                }
                KeyCode::Char(c) => {
                    buf.push(c);
                    self.mode = Mode::Arg { path, action, buf };
                }
                _ => self.mode = Mode::Arg { path, action, buf },
            },
            Mode::Secret { path, action, mut buf } => match k.code {
                KeyCode::Esc => self.msg = "cancelled; nothing entered was kept".into(),
                KeyCode::Enter if buf.is_empty() => self.mode = Mode::Secret { path, action, buf },
                // The queued command shows `<stdin>`; `buf` drops, zeroed, here.
                KeyCode::Enter => self.stage(&path, action, ""),
                KeyCode::Backspace => {
                    buf.pop();
                    self.mode = Mode::Secret { path, action, buf };
                }
                KeyCode::Char(c) => {
                    buf.push(c);
                    self.mode = Mode::Secret { path, action, buf };
                }
                _ => self.mode = Mode::Secret { path, action, buf },
            },
            Mode::Confirm { queued } => match k.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => self.enqueue(queued),
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => self.msg = "not queued".into(),
                _ => self.mode = Mode::Confirm { queued },
            },
            Mode::Browse => return self.browse(k),
        }
        true
    }

    fn browse(&mut self, k: KeyEvent) -> bool {
        let rows = self.rows();
        if rows.is_empty() {
            match k.code {
                KeyCode::Char('q') => return false,
                KeyCode::Esc => self.clear_filter(),
                KeyCode::Char('/') => self.mode = Mode::Filter,
                _ => {}
            }
            return true;
        }
        self.cursor = self.cursor.min(rows.len() - 1);
        let path = rows[self.cursor].path.clone();
        let last = rows.len() - 1;
        match k.code {
            KeyCode::Char('q') => return false,
            KeyCode::Esc if !self.filter.is_empty() => self.clear_filter(),
            KeyCode::Esc => return false,
            KeyCode::Tab => self.switch_tab((self.tab + 1) % self.roots.len()),
            KeyCode::BackTab => self.switch_tab((self.tab + self.roots.len() - 1) % self.roots.len()),
            KeyCode::Char(c @ '1'..='9') => {
                let i = c as usize - '1' as usize;
                if i < self.roots.len() {
                    self.switch_tab(i);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.cursor = (self.cursor + 1).min(last),
            KeyCode::PageUp => self.cursor = self.cursor.saturating_sub(10),
            KeyCode::PageDown => self.cursor = (self.cursor + 10).min(last),
            KeyCode::Home | KeyCode::Char('g') => self.cursor = 0,
            KeyCode::End | KeyCode::Char('G') => self.cursor = last,
            KeyCode::Right | KeyCode::Char('l') => {
                let n = tree::get_mut(&mut self.roots, &path);
                if !n.children.is_empty() {
                    if n.open {
                        self.cursor = (self.cursor + 1).min(last);
                    } else {
                        n.open = true;
                    }
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                let n = tree::get_mut(&mut self.roots, &path);
                if n.open && !n.children.is_empty() && self.filter.is_empty() {
                    n.open = false;
                } else if path.len() > 1 {
                    let parent = &path[..path.len() - 1];
                    if let Some(i) = rows.iter().position(|r| r.path == parent) {
                        self.cursor = i;
                    }
                }
            }
            KeyCode::Enter if !self.filter.is_empty() => self.jump(&path),
            KeyCode::Enter | KeyCode::Char(' ') => self.activate(&path),
            KeyCode::Char('e') => self.begin_edit(&path),
            KeyCode::Char('d') => self.reset(&path, true),
            KeyCode::Char('u') => self.reset(&path, false),
            KeyCode::Char('a') => self.open_menu(&path),
            KeyCode::Char('x') => match self.queue.undo_last() {
                Some(q) => self.msg = format!("unqueued: {}", q.command),
                None => self.msg = "no queued action".into(),
            },
            KeyCode::Char('c') => self.show_changes = !self.show_changes,
            KeyCode::Char('/') => self.mode = Mode::Filter,
            KeyCode::Char('?') => self.mode = Mode::Help,
            _ => {}
        }
        true
    }

    /// Enter: toggle a bool, cycle a choice, edit text and numbers, enter a
    /// secret masked, open the action menu of a node that only has actions, or
    /// open and close a group.
    fn activate(&mut self, path: &[usize]) {
        let n = tree::get_mut(&mut self.roots, path);
        if !n.actions.is_empty() && n.setting.as_ref().is_some_and(|s| matches!(s.kind, Kind::ReadOnly)) {
            return self.open_menu(path);
        }
        match n.setting.as_mut() {
            Some(s) => match &s.kind {
                Kind::Bool => {
                    s.value = if s.value == "true" { "false" } else { "true" }.into();
                    self.msg = format!("{} = {}", n.name, s.value);
                }
                Kind::Choice(opts) => {
                    let i = opts.iter().position(|o| *o == s.value).map_or(0, |i| (i + 1) % opts.len());
                    s.value = opts[i].clone();
                    self.msg = format!("{} = {}", n.name, s.value);
                }
                Kind::ReadOnly => self.msg = "read-only here; the detail pane names the command".into(),
                Kind::Secret => match n.actions.iter().position(|a| matches!(a.arg, Arg::Secret)) {
                    Some(action) => self.mode = Mode::Secret { path: path.to_vec(), action, buf: SecretBuf::default() },
                    None => self.msg = "no way to set this here".into(),
                },
                _ => self.begin_edit(path),
            },
            None => n.open = !n.open,
        }
    }

    fn begin_edit(&mut self, path: &[usize]) {
        let n = tree::get(&self.roots, path);
        match &n.setting {
            Some(s) if s.kind.editable() => self.mode = Mode::Edit(s.value.clone()),
            _ => self.msg = "nothing to edit".into(),
        }
    }

    fn open_menu(&mut self, path: &[usize]) {
        if tree::get(&self.roots, path).actions.is_empty() {
            self.msg = "no actions here".into();
        } else {
            self.mode = Mode::Menu { path: path.to_vec(), sel: 0 };
        }
    }

    /// An action was chosen: ask for its argument, or stage it at once.
    fn pick(&mut self, path: Vec<usize>, action: usize) {
        match tree::get(&self.roots, &path).actions[action].arg {
            Arg::None => self.stage(&path, action, ""),
            Arg::Text(_) => self.mode = Mode::Arg { path, action, buf: String::new() },
            Arg::Secret => self.mode = Mode::Secret { path, action, buf: SecretBuf::default() },
        }
    }

    /// Build the queued command; ask first when the action needs confirming.
    fn stage(&mut self, path: &[usize], action: usize, text: &str) {
        let a = &tree::get(&self.roots, path).actions[action];
        let queued = Queued { key: tree::key(&self.roots, path), label: a.label.clone(), command: a.render(text) };
        if a.confirm {
            self.mode = Mode::Confirm { queued };
        } else {
            self.enqueue(queued);
        }
    }

    fn enqueue(&mut self, q: Queued) {
        self.msg = format!("queued: {}", q.command);
        self.queue.push(q);
    }

    fn commit(&mut self, buf: &str) {
        let rows = self.rows();
        let path = rows[self.cursor.min(rows.len() - 1)].path.clone();
        let n = tree::get_mut(&mut self.roots, &path);
        let s = n.setting.as_mut().expect("edit mode only on settings");
        match s.validate(buf) {
            Ok(v) => {
                s.value = v;
                self.msg = format!("{} = {}", n.name, s.value);
            }
            Err(e) => {
                self.msg = format!("rejected: {e}");
                self.mode = Mode::Edit(buf.to_string());
            }
        }
    }

    fn reset(&mut self, path: &[usize], to_default: bool) {
        let n = tree::get_mut(&mut self.roots, path);
        let Some(s) = n.setting.as_mut().filter(|s| s.kind.editable()) else {
            self.msg = "nothing to reset".into();
            return;
        };
        if to_default {
            match &s.default {
                Some(d) => {
                    s.value = d.clone();
                    self.msg = format!("{} = {} (default)", n.name, d);
                }
                None => self.msg = "no default".into(),
            }
        } else {
            s.value = s.loaded.clone();
            self.msg = format!("{} reverted", n.name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{Action, Setting};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

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

        let out = crate::summary(&app.roots, &app.queue);
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
        let mut app = App::new("t", vec![target]);
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
        let matching = Node::group("matching", "", vec![Node::leaf("tau_s", "", Setting::new(Kind::Float { min: 0.0, max: 1.0 }, "0.5", "default").default("0.5"))]);
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
        // On ways: close `deep` at row 3, then move the cursor to it.
        keys(&mut app, &[KeyCode::Down, KeyCode::Down, KeyCode::Down, KeyCode::Right]);
        assert_eq!(app.cursor, 3);
        assert!(screen(&mut app).contains("gamma"));
        keys(&mut app, &[KeyCode::Char('2'), KeyCode::Down]);
        assert_eq!(app.cursor, 1);
        keys(&mut app, &[KeyCode::Char('1')]);
        assert_eq!(app.cursor, 3);
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
        let mut app = App::new("t", tabbed());
        keys(&mut app, &[KeyCode::Down, KeyCode::Enter]);
        let bar = frame(&mut app, 100, 24)[0].clone();
        assert!(bar.contains("1 ways ●1") && !bar.contains("matching ●") && !bar.contains("gate ●"), "{bar}");
        keys(&mut app, &[KeyCode::Char('3'), KeyCode::Char('a'), KeyCode::Enter]);
        let bar = frame(&mut app, 100, 24)[0].clone();
        assert!(bar.contains("3 gate ●1"), "{bar}");
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

    /// Prints one frame of the real tree for layout review: `cargo test frame_dump -- --nocapture --ignored`.
    #[test]
    #[ignore]
    fn frame_dump() {
        let dir = std::env::current_dir().unwrap();
        let mut app = App::new(" ways settings ", crate::ways::build(&crate::ways::Paths::resolve(&dir), &dir));
        keys(&mut app, &[KeyCode::Down, KeyCode::Down, KeyCode::Right, KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
        println!("{}", frame(&mut app, 100, 30).join("\n"));
    }
}
