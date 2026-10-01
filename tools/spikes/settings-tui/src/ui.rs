//! The generic TUI over a settings tree: one tab per root, then browse,
//! filter, edit, run actions, review. Key handling and state live here; drawing is in `render`.

mod render;
pub mod theme;

use std::io;

use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Position, Rect};
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

/// Where the last frame drew what a click can hit.
#[derive(Default)]
struct Hits {
    /// Each tab's lozenge and its root.
    tabs: Vec<(Rect, usize)>,
    /// The tree list inside its border; row `i` is at `y + i - offset`.
    list: Rect,
    /// The open menu and one rect per item.
    menu: Option<(Rect, Vec<Rect>)>,
    /// A confirm's answers: yes or no.
    answers: Vec<(Rect, bool)>,
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
    /// Whether the terminal should report the mouse; `m` turns it off so the
    /// terminal's own text selection works.
    mouse: bool,
    shape: theme::Shape,
    hits: Hits,
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
            mouse: true,
            shape: theme::Shape::ROUND,
            hits: Hits::default(),
        }
    }

    pub fn shape(mut self, shape: theme::Shape) -> Self {
        self.shape = shape;
        self
    }

    /// Run until quit. Mouse capture follows `self.mouse`; the caller turns
    /// it off on every exit path, since `ratatui::restore` does not.
    pub fn run(mut self, term: &mut DefaultTerminal) -> io::Result<Session> {
        let mut captured = false;
        loop {
            if self.mouse != captured {
                if self.mouse {
                    execute!(io::stdout(), EnableMouseCapture)?;
                } else {
                    execute!(io::stdout(), DisableMouseCapture)?;
                }
                captured = self.mouse;
            }
            term.draw(|f| self.draw(f))?;
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press && !self.key(k) => {
                    return Ok(Session { roots: self.roots, queue: self.queue });
                }
                Event::Mouse(m) => self.mouse(m),
                _ => {}
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
            KeyCode::Char('m') => {
                self.mouse = !self.mouse;
                self.msg = if self.mouse { "mouse on" } else { "mouse off: the terminal selects text" }.into();
            }
            KeyCode::Char('/') => self.mode = Mode::Filter,
            KeyCode::Char('?') => self.mode = Mode::Help,
            _ => {}
        }
        true
    }

    /// Handle one mouse event against the last frame's hits. A click reaches
    /// the same paths a key does, so the confirm and secret rules hold.
    /// During text, secret or filter entry and a confirm, only the confirm's
    /// answers respond.
    fn mouse(&mut self, m: MouseEvent) {
        let at = Position::new(m.column, m.row);
        let wheel = match m.kind {
            MouseEventKind::ScrollUp => Some(KeyCode::Up),
            MouseEventKind::ScrollDown => Some(KeyCode::Down),
            _ => None,
        };
        let click = m.kind == MouseEventKind::Down(MouseButton::Left);
        let press = |c| KeyEvent::new(c, KeyModifiers::NONE);
        match &mut self.mode {
            Mode::Browse => {
                if let Some(k) = wheel {
                    self.browse(press(k));
                } else if click {
                    self.click_browse(at);
                }
            }
            Mode::Menu { sel, .. } => {
                if let Some(k) = wheel {
                    self.key(press(k));
                } else if click {
                    let Some((menu, items)) = &self.hits.menu else { return };
                    if let Some(i) = items.iter().position(|r| r.contains(at)) {
                        *sel = i;
                        self.key(press(KeyCode::Enter));
                    } else if !menu.contains(at) {
                        self.key(press(KeyCode::Esc));
                    }
                }
            }
            Mode::Confirm { .. } if click => {
                if let Some(&(_, yes)) = self.hits.answers.iter().find(|(r, _)| r.contains(at)) {
                    self.key(press(KeyCode::Char(if yes { 'y' } else { 'n' })));
                }
            }
            Mode::Help if click => self.mode = Mode::Browse,
            _ => {}
        }
    }

    /// A click while browsing: a tab switches to it; a row selects it, and a
    /// second click on the selected row acts as Enter; a group's marker opens
    /// or closes it.
    fn click_browse(&mut self, at: Position) {
        if let Some(&(_, tab)) = self.hits.tabs.iter().find(|(r, _)| r.contains(at)) {
            return self.switch_tab(tab);
        }
        let list = self.hits.list;
        if !list.contains(at) {
            return;
        }
        let i = self.list.offset() + (at.y - list.y) as usize;
        let rows = self.rows();
        let Some(row) = rows.get(i) else { return };
        // The selection mark takes one column, then two per level of depth.
        let marker = list.x + 1 + 2 * row.depth as u16;
        let n = tree::get_mut(&mut self.roots, &row.path);
        if self.filter.is_empty() && !n.children.is_empty() && (marker..marker + 2).contains(&at.x) {
            n.open = !n.open;
            self.cursor = i;
        } else if i == self.cursor {
            self.browse(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        } else {
            self.cursor = i;
        }
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
    use ratatui::style::Modifier;
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
        assert_eq!(app.cursor, 2);
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
        assert_eq!(app.cursor, 3);
        click(&mut app, (x + 1, y));
        assert!(!screen(&mut app).contains("gamma"), "the marker's second column closes it again");

        mouse_at(&mut app, MouseEventKind::ScrollUp, (0, 0));
        mouse_at(&mut app, MouseEventKind::ScrollUp, (0, 0));
        assert_eq!(app.cursor, 1);
        mouse_at(&mut app, MouseEventKind::ScrollDown, (0, 0));
        assert_eq!(app.cursor, 2);
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
        let mut app = App::new("t", tabbed());
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
        assert_eq!((at("1 ways", 0).fg, at("1 ways", 0).bg), (theme::INK, theme::ACCENT));
        assert_eq!(at("●1", 0).bg, theme::WARN);
        assert_eq!(at("2 matching", 0).bg, theme::ACCENT_DIM);
        // The selected row keeps the changed value's colour over the accent shade.
        let y = (0..25).find(|&y| row(y).contains(theme::SELECTED_MARK)).unwrap();
        let v = at("false", y);
        assert_eq!((v.fg, v.bg), (theme::WARN, theme::ACCENT_SHADE));
        assert!(v.modifier.contains(Modifier::BOLD));
        // The status line: a mode lozenge, then the count in the changed style.
        assert_eq!(at("browse", 24).bg, theme::ACCENT);
        assert_eq!(at("●1 changed", 24).fg, theme::WARN);
    }

    #[test]
    fn m_toggles_mouse_capture_and_the_status_line_says_so() {
        let mut app = App::new("t", tabbed());
        assert!(app.mouse && screen(&mut app).contains("mouse on"));
        keys(&mut app, &[KeyCode::Char('m')]);
        assert!(!app.mouse && screen(&mut app).contains("mouse off"));
    }

    /// Writes frames of the real tree, cell by cell, for a PNG review:
    /// `SNAP_DIR=dir cargo test snap_frames -- --ignored`. Each line is the
    /// symbol, foreground, background and modifiers of one cell, row-major.
    #[test]
    #[ignore]
    fn snap_frames() {
        let out = std::path::PathBuf::from(std::env::var("SNAP_DIR").expect("SNAP_DIR"));
        let dir = std::env::current_dir().unwrap();
        let real = || App::new(" ways settings ", crate::ways::build(&crate::ways::Paths::resolve(&dir), &dir));
        let shot = |app: &mut App, name: &str, w: u16, h: u16| {
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            term.draw(|f| app.draw(f)).unwrap();
            let cells: Vec<String> = term
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|c| format!("{}\t{:?}\t{:?}\t{:?}", c.symbol(), c.fg, c.bg, c.modifier))
                .collect();
            std::fs::write(out.join(format!("{name}.cells")), format!("{w} {h}\n{}\n", cells.join("\n"))).unwrap();
        };
        // gate at 100x30: keys open, a check queued, the cursor on a provider.
        let mut app = real();
        keys(&mut app, &[KeyCode::Char('3'), KeyCode::Char('G'), KeyCode::Right, KeyCode::Down, KeyCode::Char('a'), KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
        keys(&mut app, &[KeyCode::Up, KeyCode::Up, KeyCode::Up, KeyCode::Enter, KeyCode::Down, KeyCode::Enter]);
        shot(&mut app, "gate-100x30", 100, 30);
        // The menu and the confirm of a key's remove action.
        keys(&mut app, &[KeyCode::Char('G'), KeyCode::Up, KeyCode::Char('a')]);
        shot(&mut app, "gate-menu-100x30", 100, 30);
        let Mode::Menu { path, .. } = &app.mode else { panic!("no menu") };
        let remove = tree::get(&app.roots, path).actions.iter().position(|a| a.confirm).expect("a confirmed action");
        keys(&mut app, &vec![KeyCode::Down; remove]);
        keys(&mut app, &[KeyCode::Enter]);
        shot(&mut app, "gate-confirm-100x30", 100, 30);
        // ways at 80x25: a domain switched off, so the tab and the row carry it.
        let mut app = real();
        keys(&mut app, &[KeyCode::Down, KeyCode::Down, KeyCode::Right, KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
        shot(&mut app, "ways-80x25", 80, 25);
        keys(&mut app, &[KeyCode::Char('c')]);
        shot(&mut app, "ways-pending-80x25", 80, 25);
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
