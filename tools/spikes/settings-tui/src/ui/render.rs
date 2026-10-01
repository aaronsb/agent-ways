//! Drawing for the TUI: the tree, the detail and pending panes, the action
//! menu, the status line and the key help.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use super::{App, Mode};
use crate::tree::{self, Arg, Kind, Row};

/// A fixed-width mask once anything is typed, so the key's length never
/// reaches the screen.
fn mask(n: usize) -> String {
    if n == 0 { String::new() } else { "••••••••".into() }
}

impl App {
    pub(super) fn draw(&mut self, f: &mut Frame) {
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
        match &self.mode {
            Mode::Help => draw_help(f, main),
            Mode::Menu { path, sel } => self.draw_menu(f, main, path, *sel),
            _ => {}
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
                    } else if matches!(s.kind, Kind::Secret) {
                        Style::new().fg(if s.value == "present" { Color::Green } else { Color::DarkGray })
                    } else if matches!(s.kind, Kind::ReadOnly) {
                        Style::new().fg(Color::DarkGray)
                    } else if s.default.as_deref().is_some_and(|d| d != s.value) {
                        Style::new().fg(Color::Cyan)
                    } else {
                        Style::new()
                    };
                    spans.push(Span::styled(s.value.clone(), style));
                }
                if !n.actions.is_empty() {
                    spans.push(Span::styled(" [a]", Style::new().fg(Color::Magenta)));
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
        if !n.actions.is_empty() {
            lines.push(Line::raw(""));
            lines.push(Line::styled("actions (a)", dim));
            for a in &n.actions {
                let tag = if a.confirm { " (asks first)" } else { "" };
                lines.push(Line::raw(format!("  {}{tag}", a.label)));
                lines.push(Line::styled(format!("    {}", a.render("<arg>")), dim));
            }
        }
        let p = Paragraph::new(lines).wrap(Wrap { trim: false }).block(Block::default().borders(Borders::ALL).title("detail"));
        f.render_widget(p, area);
    }

    /// The pending pane: value changes by file, then queued actions in order.
    fn draw_changes(&self, f: &mut Frame, area: Rect) {
        let ch = tree::changes(&self.roots);
        let mut lines = Vec::new();
        if ch.is_empty() && self.queue.is_empty() {
            lines.push(Line::raw("nothing pending"));
        }
        let mut files: Vec<String> = ch.iter().map(|c| c.1.as_ref().map_or("(no store)".into(), |s| s.file.display().to_string())).collect();
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
        if !self.queue.is_empty() {
            lines.push(Line::styled("queued actions (x unqueues the last)", Style::new().add_modifier(Modifier::BOLD)));
            for (i, q) in self.queue.items().iter().enumerate() {
                lines.push(Line::raw(format!("  {}. {}  [{}]", i + 1, q.label, q.key)));
                lines.push(Line::styled(format!("     $ {}", q.command), Style::new().fg(Color::Magenta)));
            }
        }
        lines.push(Line::raw(""));
        lines.push(Line::styled("spike: nothing is written or run", Style::new().fg(Color::DarkGray)));
        let title = format!("pending ({} changes, {} actions)", ch.len(), self.queue.len());
        let p = Paragraph::new(lines).wrap(Wrap { trim: false }).block(Block::default().borders(Borders::ALL).title(title));
        f.render_widget(p, area);
    }

    fn draw_menu(&self, f: &mut Frame, area: Rect, path: &[usize], sel: usize) {
        let n = tree::get(&self.roots, path);
        let lines: Vec<Line> = n
            .actions
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let tag = match (&a.arg, a.confirm) {
                    (Arg::Secret, _) => "  masked".to_string(),
                    (Arg::Text(p), true) => format!("  {p}, asks first"),
                    (Arg::Text(p), false) => format!("  {p}"),
                    (Arg::None, true) => "  asks first".to_string(),
                    (Arg::None, false) => String::new(),
                };
                let style = if i == sel { Style::new().bg(Color::DarkGray).add_modifier(Modifier::BOLD) } else { Style::new() };
                Line::styled(format!(" {:<10}{tag}", a.label), style)
            })
            .collect();
        let w = 50.min(area.width);
        let h = (lines.len() as u16 + 2).min(area.height);
        let r = Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h };
        f.render_widget(Clear, r);
        let title = format!("actions: {}", tree::key(&self.roots, path));
        f.render_widget(Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(title)), r);
    }

    fn status_line(&self) -> Line<'static> {
        let key = Style::new().fg(Color::Black).bg(Color::Cyan);
        let hint = Style::new().fg(Color::Black).bg(Color::Magenta);
        match &self.mode {
            Mode::Edit(buf) => Line::from(vec![Span::styled(" edit ", key), Span::raw(format!(" {buf}▏  Enter set · Esc cancel   {}", self.msg))]),
            Mode::Filter => Line::from(vec![Span::styled(" / ", key), Span::raw(format!(" {}▏  Enter keep · Esc clear", self.filter))]),
            Mode::Menu { .. } => Line::from(vec![Span::styled(" action ", hint), Span::raw("  ↑↓ choose · Enter pick · Esc close")]),
            Mode::Arg { path, action, buf } => {
                let prompt = match &tree::get(&self.roots, path).actions[*action].arg {
                    Arg::Text(p) => p.clone(),
                    _ => "argument".into(),
                };
                Line::from(vec![Span::styled(format!(" {prompt} "), hint), Span::raw(format!(" {buf}▏  Enter queue · Esc cancel"))])
            }
            Mode::Secret { buf, .. } => Line::from(vec![
                Span::styled(" secret ", hint),
                Span::raw(format!(" {}▏  Enter queue (goes to stdin, never shown) · Esc cancel", mask(buf.len()))),
            ]),
            Mode::Confirm { queued } => Line::from(vec![
                Span::styled(" confirm ", Style::new().fg(Color::Black).bg(Color::Yellow)),
                Span::raw(format!(" {}  y queue · n cancel", queued.command)),
            ]),
            _ => Line::from(vec![
                Span::styled(format!(" {} changed ", tree::changes(&self.roots).len()), key),
                Span::styled(format!(" {} queued ", self.queue.len()), hint),
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
        Kind::Secret => "secret (Enter enters it masked)".into(),
    }
}

fn draw_help(f: &mut Frame, area: Rect) {
    let lines = [
        "↑↓ / j k     move          PgUp PgDn g G   jump",
        "→ / l        open group    ← / h           close / parent",
        "Enter Space  toggle bool, cycle choice, edit value, open group;",
        "             on a secret, masked entry; on an action-only node, its menu",
        "e            edit as text  d               set to default",
        "u            revert        /               filter by key",
        "a            actions menu  x               unqueue the last action",
        "c            pending pane (changes and queued actions)",
        "q Esc        quit (prints the change set and queued commands)",
        "",
        "yellow = changed · cyan = differs from default · grey = read-only",
        "[a] = has actions · magenta = queued · y/n answers a confirm",
        "",
        "any key closes",
    ];
    let w = 72.min(area.width);
    let h = (lines.len() as u16 + 2).min(area.height);
    let r = Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h };
    f.render_widget(Clear, r);
    f.render_widget(
        Paragraph::new(lines.iter().map(|l| Line::raw(*l)).collect::<Vec<_>>()).block(Block::default().borders(Borders::ALL).title("keys")),
        r,
    );
}
