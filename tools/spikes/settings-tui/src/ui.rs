//! The generic TUI over a settings tree: browse, filter, edit, review.

use std::io;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};

use crate::tree::{self, Kind, Node, Row};

enum Mode {
    Browse,
    Edit(String),
    Filter,
    Help,
}

pub struct App {
    pub roots: Vec<Node>,
    title: String,
    cursor: usize,
    mode: Mode,
    filter: String,
    msg: String,
    show_changes: bool,
    list: ListState,
}

impl App {
    pub fn new(title: impl Into<String>, roots: Vec<Node>) -> Self {
        App {
            roots,
            title: title.into(),
            cursor: 0,
            mode: Mode::Browse,
            filter: String::new(),
            msg: "? for keys".into(),
            show_changes: false,
            list: ListState::default(),
        }
    }

    pub fn run(mut self, term: &mut DefaultTerminal) -> io::Result<Vec<Node>> {
        loop {
            term.draw(|f| self.draw(f))?;
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press && !self.key(k) {
                    return Ok(self.roots);
                }
            }
        }
    }

    fn rows(&self) -> Vec<Row> {
        tree::rows(&self.roots, &self.filter)
    }

    /// Handle one key. False ends the session.
    fn key(&mut self, k: KeyEvent) -> bool {
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            return false;
        }
        match std::mem::replace(&mut self.mode, Mode::Browse) {
            Mode::Help => {}
            Mode::Filter => match k.code {
                KeyCode::Esc => self.filter.clear(),
                KeyCode::Enter => {}
                KeyCode::Backspace => {
                    self.filter.pop();
                    self.mode = Mode::Filter;
                }
                KeyCode::Char(c) => {
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
            Mode::Browse => return self.browse(k),
        }
        true
    }

    fn browse(&mut self, k: KeyEvent) -> bool {
        let rows = self.rows();
        if rows.is_empty() {
            match k.code {
                KeyCode::Char('q') => return false,
                KeyCode::Esc => self.filter.clear(),
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
            KeyCode::Esc if !self.filter.is_empty() => self.filter.clear(),
            KeyCode::Esc => return false,
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
            KeyCode::Enter | KeyCode::Char(' ') => self.activate(&path),
            KeyCode::Char('e') => self.begin_edit(&path),
            KeyCode::Char('d') => self.reset(&path, true),
            KeyCode::Char('u') => self.reset(&path, false),
            KeyCode::Char('c') => self.show_changes = !self.show_changes,
            KeyCode::Char('/') => self.mode = Mode::Filter,
            KeyCode::Char('?') => self.mode = Mode::Help,
            _ => {}
        }
        true
    }

    /// Enter: toggle a bool, cycle a choice, edit text and numbers, or open
    /// and close a group.
    fn activate(&mut self, path: &[usize]) {
        let n = tree::get_mut(&mut self.roots, path);
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
                _ => self.begin_edit(path),
            },
            None => n.open = !n.open,
        }
    }

    fn begin_edit(&mut self, path: &[usize]) {
        let n = tree::get(&self.roots, path);
        match &n.setting {
            Some(s) if !matches!(s.kind, Kind::ReadOnly) => self.mode = Mode::Edit(s.value.clone()),
            _ => self.msg = "nothing to edit".into(),
        }
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
        let Some(s) = n.setting.as_mut().filter(|s| !matches!(s.kind, Kind::ReadOnly)) else {
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

    fn draw(&mut self, f: &mut Frame) {
        let [main, status] = Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).areas(f.area());
        let [left, right] = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(main);
        let rows = self.rows();
        if !rows.is_empty() {
            self.cursor = self.cursor.min(rows.len() - 1);
        }
        self.draw_tree(f, left, &rows);
        if self.show_changes {
            self.draw_changes(f, right);
        } else if let Some(r) = rows.get(self.cursor) {
            self.draw_detail(f, right, &r.path);
        }
        if let Mode::Help = self.mode {
            draw_help(f, main);
        }
        f.render_widget(Paragraph::new(self.status_line()), status);
    }

    fn draw_tree(&mut self, f: &mut Frame, area: Rect, rows: &[Row]) {
        let inner = area.width.saturating_sub(4) as usize;
        let value_col = inner * 3 / 5;
        let items: Vec<ListItem> = rows
            .iter()
            .map(|r| {
                let n = tree::get(&self.roots, &r.path);
                let marker = match (n.children.is_empty(), n.open || !self.filter.is_empty()) {
                    (true, _) => "  ",
                    (false, true) => "▾ ",
                    (false, false) => "▸ ",
                };
                let left = format!("{}{}{}", "  ".repeat(r.depth), marker, n.name);
                let mut spans = vec![Span::styled(
                    left.clone(),
                    if n.setting.is_none() { Style::new().add_modifier(Modifier::BOLD) } else { Style::new() },
                )];
                let pad = value_col.saturating_sub(left.chars().count()).max(1);
                spans.push(Span::raw(" ".repeat(pad)));
                if let Some(s) = &n.setting {
                    let style = if s.changed() {
                        Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else if matches!(s.kind, Kind::ReadOnly) {
                        Style::new().fg(Color::DarkGray)
                    } else if s.default.as_deref().is_some_and(|d| d != s.value) {
                        Style::new().fg(Color::Cyan)
                    } else {
                        Style::new()
                    };
                    spans.push(Span::styled(s.value.clone(), style));
                }
                let c = n.changes();
                if c > 0 && !n.children.is_empty() {
                    spans.push(Span::styled(format!("  ●{c}"), Style::new().fg(Color::Yellow)));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();
        let title = if self.filter.is_empty() { self.title.clone() } else { format!("{} — /{}", self.title, self.filter) };
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(title))
            .highlight_style(Style::new().bg(Color::DarkGray).add_modifier(Modifier::BOLD));
        self.list.select(if rows.is_empty() { None } else { Some(self.cursor) });
        f.render_stateful_widget(list, area, &mut self.list);
    }

    fn draw_detail(&self, f: &mut Frame, area: Rect, path: &[usize]) {
        let n = tree::get(&self.roots, path);
        let dim = Style::new().fg(Color::DarkGray);
        let mut lines = vec![Line::styled(tree::key(&self.roots, path), Style::new().add_modifier(Modifier::BOLD)), Line::raw("")];
        if !n.doc.is_empty() {
            lines.push(Line::raw(n.doc.clone()));
            lines.push(Line::raw(""));
        }
        if let Some(s) = &n.setting {
            let field = |k: &str, v: String| Line::from(vec![Span::styled(format!("{k:<9}"), dim), Span::raw(v)]);
            lines.push(field("value", s.value.clone()));
            if s.changed() {
                lines.push(field("was", s.loaded.clone()));
            }
            lines.push(field("type", kind_label(&s.kind)));
            if let Some(d) = &s.default {
                lines.push(field("default", d.clone()));
            }
            lines.push(field("from", s.source.clone()));
            if let Some(st) = &s.store {
                lines.push(field("writes", format!("{}  [{}]", st.file.display(), st.key)));
            }
        } else {
            let c = n.changes();
            lines.push(Line::styled(format!("{} entries, {} changed", n.children.len(), c), dim));
        }
        let p = Paragraph::new(lines).wrap(Wrap { trim: false }).block(Block::default().borders(Borders::ALL).title("detail"));
        f.render_widget(p, area);
    }

    fn draw_changes(&self, f: &mut Frame, area: Rect) {
        let ch = tree::changes(&self.roots);
        let mut lines = Vec::new();
        if ch.is_empty() {
            lines.push(Line::raw("no changes"));
        }
        let mut files: Vec<String> = ch.iter().map(|c| c.1.as_ref().map_or("(no store)".into(), |s| s.file.display().to_string())).collect();
        files.dedup();
        files.sort();
        files.dedup();
        for file in files {
            lines.push(Line::styled(file.clone(), Style::new().add_modifier(Modifier::BOLD)));
            for (k, st, from, to) in &ch {
                if st.as_ref().map_or("(no store)".to_string(), |s| s.file.display().to_string()) == file {
                    let key = st.as_ref().map_or(k.clone(), |s| s.key.clone());
                    lines.push(Line::from(vec![
                        Span::raw(format!("  {key}: ")),
                        Span::styled(from.clone(), Style::new().fg(Color::Red)),
                        Span::raw(" → "),
                        Span::styled(to.clone(), Style::new().fg(Color::Green)),
                    ]));
                }
            }
        }
        lines.push(Line::raw(""));
        lines.push(Line::styled("spike: nothing is written", Style::new().fg(Color::DarkGray)));
        let p = Paragraph::new(lines).wrap(Wrap { trim: false }).block(Block::default().borders(Borders::ALL).title(format!("changes ({})", ch.len())));
        f.render_widget(p, area);
    }

    fn status_line(&self) -> Line<'static> {
        let key = Style::new().fg(Color::Black).bg(Color::Cyan);
        match &self.mode {
            Mode::Edit(buf) => Line::from(vec![Span::styled(" edit ", key), Span::raw(format!(" {buf}▏  Enter set · Esc cancel   {}", self.msg))]),
            Mode::Filter => Line::from(vec![Span::styled(" / ", key), Span::raw(format!(" {}▏  Enter keep · Esc clear", self.filter))]),
            _ => Line::from(vec![
                Span::styled(format!(" {} changed ", tree::changes(&self.roots).len()), key),
                Span::raw(format!("  {}", self.msg)),
            ]),
        }
    }
}

fn kind_label(k: &Kind) -> String {
    match k {
        Kind::Bool => "bool (Enter toggles)".into(),
        Kind::Float { min, max } => format!("number {min}..={max} (Enter edits)"),
        Kind::Int { min, max } => format!("integer {min}..={max} (Enter edits)"),
        Kind::Choice(o) => format!("one of {} (Enter cycles)", o.join(" | ")),
        Kind::Text => "text (Enter edits)".into(),
        Kind::ReadOnly => "read-only here".into(),
    }
}

fn draw_help(f: &mut Frame, area: Rect) {
    let lines = [
        "↑↓ / j k     move          PgUp PgDn g G   jump",
        "→ / l        open group    ← / h           close / parent",
        "Enter Space  toggle bool, cycle choice, edit value, open group",
        "e            edit as text  d               set to default",
        "u            revert        /               filter by key",
        "c            changes pane  q Esc           quit (prints the change set)",
        "",
        "yellow = changed · cyan = differs from default · grey = read-only",
        "",
        "any key closes",
    ];
    let w = 70.min(area.width);
    let h = (lines.len() as u16 + 2).min(area.height);
    let r = Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h };
    f.render_widget(ratatui::widgets::Clear, r);
    f.render_widget(
        Paragraph::new(lines.iter().map(|l| Line::raw(*l)).collect::<Vec<_>>()).block(Block::default().borders(Borders::ALL).title("keys")),
        r,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Setting;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn renders_and_edits_through_keys() {
        let roots = vec![Node::group(
            "matching",
            "",
            vec![Node::leaf("tau_s", "doc", Setting::new(Kind::Float { min: 0.0, max: 1.0 }, "0.5", "default").default("0.5"))],
        )];
        let mut app = App::new("t", roots);
        let press = |c| KeyEvent::new(c, KeyModifiers::NONE);
        for k in [KeyCode::Right, KeyCode::Down, KeyCode::Enter] {
            assert!(app.key(press(k)));
        }
        assert!(app.key(press(KeyCode::Backspace)));
        assert!(app.key(press(KeyCode::Char('4'))));
        assert!(app.key(press(KeyCode::Enter)));
        assert_eq!(tree::changes(&app.roots)[0].3, "0.4");
        let mut term = Terminal::new(TestBackend::new(100, 20)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let screen = format!("{:?}", term.backend().buffer());
        assert!(screen.contains("tau_s"));
    }
}
