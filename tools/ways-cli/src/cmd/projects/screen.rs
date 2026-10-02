//! The `ways projects` screen (#748): the projects, most recently active
//! first, and the selected one's detail as `show` prints it. The filter
//! narrows the table by `search`'s shallow match. Each pane's border names
//! the command that prints its data (ADR-507, note of 2026-10-02).

use agent_tui::input::Input;
use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use agent_tui::ratatui::layout::{Alignment, Constraint, Layout};
use agent_tui::ratatui::style::{Modifier, Style};
use agent_tui::ratatui::text::{Line, Span};
use agent_tui::ratatui::widgets::{Cell, HighlightSpacing, Paragraph, Row, Table, TableState, Wrap};
use agent_tui::ratatui::Frame as Draw;
use agent_tui::screen::Screen;
use agent_tui::theme::{self, Ground, Palette, Seg, Shape};
use agent_tui::timeline::key_bar;
use anyhow::Result;

use super::{ellipsize_left, list_cells, scan_all, shallow_match, show_project, Env, Project};
use crate::cmd::screen_host::{self, pane, Open};

/// Open the screen on the terminal, or headless as `open` asks.
pub(crate) fn open(open: &Open) -> Result<()> {
    let (palette, shape) = screen_host::look(screen_host::depth_of(open.depth.as_deref())?);
    screen_host::show(Projects::new(&Env::user(), palette, shape), open)
}

/// One project: what the table and the detail show of it.
struct Entry {
    project: Project,
    cells: [String; 4],
    detail: Vec<String>,
}

pub(crate) struct Projects {
    palette: Palette,
    shape: Shape,
    entries: Vec<Entry>,
    /// The entries the filter keeps, as indexes into `entries`.
    shown: Vec<usize>,
    /// The selected row of `shown`.
    sel: usize,
    filter: Input,
    /// Whether keys go to the filter.
    editing: bool,
    /// The detail pane's first line shown; back to the top on a new selection.
    scroll: u16,
    table: TableState,
}

impl Projects {
    pub(crate) fn new(env: &Env, palette: Palette, shape: Shape) -> Projects {
        let entries: Vec<Entry> = scan_all(env)
            .into_iter()
            .map(|project| {
                let mut out = Vec::new();
                // Writing to a Vec cannot fail.
                let _ = show_project(env, &project, &mut out);
                let text = String::from_utf8_lossy(&out);
                let mut detail: Vec<String> = text.lines().map(str::to_string).collect();
                while detail.last().is_some_and(|l| l.is_empty()) {
                    detail.pop();
                }
                Entry { cells: list_cells(env, &project), project, detail }
            })
            .collect();
        let shown = (0..entries.len()).collect();
        Projects { palette, shape, entries, shown, sel: 0, filter: Input::new(), editing: false, scroll: 0, table: TableState::default() }
    }

    fn selected(&self) -> Option<&Entry> {
        self.shown.get(self.sel).map(|&i| &self.entries[i])
    }

    /// Apply the filter again, the selection kept on its project where the
    /// filter keeps it.
    fn refilter(&mut self) {
        let was = self.shown.get(self.sel).copied();
        let q = self.filter.text().to_lowercase();
        self.shown = (0..self.entries.len()).filter(|&i| shallow_match(&self.entries[i].project, &q).0 > 0).collect();
        let sel = was.and_then(|w| self.shown.iter().position(|&i| i == w)).unwrap_or(0);
        self.select(sel);
    }

    /// Select row `sel`, the detail shown from its top when the row changes.
    fn select(&mut self, sel: usize) {
        if sel != self.sel {
            self.scroll = 0;
        }
        self.sel = sel;
    }

    fn clear_filter(&mut self) {
        self.filter.clear();
        self.editing = false;
        self.refilter();
    }

    /// Move the selection by `k`, if it is a move.
    fn travel(&mut self, k: KeyCode) {
        let last = self.shown.len().saturating_sub(1);
        let sel = match k {
            KeyCode::Up => self.sel.saturating_sub(1),
            KeyCode::Down => (self.sel + 1).min(last),
            KeyCode::PageUp => self.sel.saturating_sub(10),
            KeyCode::PageDown => (self.sel + 10).min(last),
            KeyCode::Home => 0,
            KeyCode::End => last,
            _ => self.sel,
        };
        self.select(sel);
    }

    /// The command that prints the table's data.
    fn list_command(&self) -> String {
        match self.filter.text() {
            "" => " ways projects list --json ".to_string(),
            q => format!(" ways projects search {} --json ", agent_tui::tree::quote(q)),
        }
    }
}

impl Screen for Projects {
    fn palette(&self) -> Palette {
        self.palette
    }

    fn draw(&mut self, f: &mut Draw) {
        // The table takes its rows, up to 45% of the body; the detail the rest.
        let body = f.area().height.saturating_sub(2);
        let table_h = (self.shown.len() as u16 + 3).clamp(3, (body * 45 / 100).max(3));
        let [top, list, detail, status] =
            Layout::vertical([Constraint::Length(1), Constraint::Length(table_h), Constraint::Min(5), Constraint::Length(1)]).areas(f.area());

        let mut bar = self.shape.lozenge(&[Seg::on(" projects ", Ground::AccentDim)]);
        if self.editing || !self.filter.is_empty() {
            bar.push(Span::styled("  / ", theme::accent()));
            if self.editing {
                let used: usize = bar.iter().map(Span::width).sum();
                let rows = self.filter.rows(top.width.saturating_sub(used as u16).max(1), theme::body());
                bar.extend(rows.into_iter().next().map(|l| l.spans).unwrap_or_default());
            } else {
                bar.push(Span::styled(self.filter.text().to_string(), theme::body()));
            }
        }
        f.render_widget(Paragraph::new(Line::from(bar)), top);

        let title = self.list_command();
        if self.shown.is_empty() {
            let none = if self.filter.is_empty() { "No projects found." } else { "No matching projects found." };
            f.render_widget(Paragraph::new(Line::styled(none, theme::muted())).block(pane(title)), list);
        } else {
            let right = |t: &str| Cell::from(Line::from(t.to_string()).alignment(Alignment::Right));
            let header = Row::new(vec![Cell::from("Project"), right("Sessions"), right("Size"), right("Last"), right("Memory")])
                .style(Style::new().add_modifier(Modifier::BOLD));
            let mark = Span::raw(theme::SELECTED_MARK).width();
            // Borders, the mark, the fixed columns and the gaps between all five.
            let path_w = (list.width as usize).saturating_sub(2 + mark + 27 + 4).max(8);
            let rows: Vec<Row> = self
                .shown
                .iter()
                .map(|&i| {
                    let e = &self.entries[i];
                    let [sess, size, last, mem] = &e.cells;
                    Row::new(vec![Cell::from(ellipsize_left(&e.project.path, path_w)), right(sess), right(size), right(last), right(mem)])
                })
                .collect();
            let widths = [Constraint::Min(8), Constraint::Length(8), Constraint::Length(6), Constraint::Length(7), Constraint::Length(6)];
            let t = Table::new(rows, widths)
                .header(header)
                .column_spacing(1)
                .block(pane(title))
                .row_highlight_style(theme::selected())
                .highlight_symbol(Line::styled(theme::SELECTED_MARK, theme::accent()))
                .highlight_spacing(HighlightSpacing::Always);
            self.table.select(Some(self.sel));
            f.render_stateful_widget(t, list, &mut self.table);
        }

        // With no project selected there is nothing for `show` to print.
        let (title, lines) = match self.selected() {
            Some(e) => {
                let mut lines: Vec<Line> = e.detail.iter().map(|l| Line::raw(l.clone())).collect();
                if let Some(first) = lines.first_mut() {
                    *first = first.clone().style(theme::accent());
                }
                (format!(" ways projects show {} --json ", agent_tui::tree::quote(&e.project.path)), lines)
            }
            None => (" show ".to_string(), Vec::new()),
        };
        // The lines the detail wraps to, so the scroll stops at its end.
        let inner = detail.width.saturating_sub(2).max(1) as usize;
        // Wrapping at words can take a row more than the width alone needs.
        let rows: usize = lines.iter().map(|l| l.width().max(1)).map(|w| w.div_ceil(inner) + usize::from(w > inner)).sum();
        let visible = detail.height.saturating_sub(2) as usize;
        self.scroll = self.scroll.min(rows.saturating_sub(visible) as u16);
        let mut block = pane(title);
        if rows > visible + self.scroll as usize {
            block = block.title_bottom(Line::styled(" more ↓ ", theme::muted()).right_aligned());
        }
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).scroll((self.scroll, 0)).block(block), detail);

        let count = vec![Span::styled(format!("{}/{}", (self.sel + 1).min(self.shown.len()), self.shown.len()), theme::muted())];
        let (mode, keys): (&str, Vec<(&str, &str)>) = if self.editing {
            ("filter", vec![("↑↓", "select"), ("⏎", "keep"), ("esc", "clear")])
        } else if self.filter.is_empty() {
            ("projects", vec![("↑↓", "select"), ("J/K", "detail"), ("/", "filter"), ("q", "quit")])
        } else {
            ("projects", vec![("↑↓", "select"), ("J/K", "detail"), ("/", "filter"), ("esc", "clear"), ("q", "quit")])
        };
        f.render_widget(Paragraph::new(key_bar(self.shape, mode, Ground::Accent, &keys, count, status.width)), status);
    }

    fn key(&mut self, k: KeyEvent) -> bool {
        if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
            return false;
        }
        if self.editing {
            match k.code {
                KeyCode::Enter => self.editing = false,
                KeyCode::Esc => self.clear_filter(),
                // Home and End stay the filter's.
                c @ (KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown) => self.travel(c),
                _ => {
                    if self.filter.key(k) {
                        self.refilter();
                    }
                }
            }
            return true;
        }
        match k.code {
            KeyCode::Char('q') => false,
            KeyCode::Esc if self.filter.is_empty() => false,
            KeyCode::Esc => {
                self.clear_filter();
                true
            }
            KeyCode::Char('/') => {
                self.editing = true;
                true
            }
            // Shift scrolls the detail; the draw keeps it in range.
            KeyCode::Char('J') => {
                self.scroll = self.scroll.saturating_add(1);
                true
            }
            KeyCode::Char('K') => {
                self.scroll = self.scroll.saturating_sub(1);
                true
            }
            c => {
                self.travel(match c {
                    KeyCode::Char('k') => KeyCode::Up,
                    KeyCode::Char('j') => KeyCode::Down,
                    KeyCode::Char('g') => KeyCode::Home,
                    KeyCode::Char('G') => KeyCode::End,
                    c => c,
                });
                true
            }
        }
    }
}
