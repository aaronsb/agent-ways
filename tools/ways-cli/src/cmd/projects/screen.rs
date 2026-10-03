//! The `ways projects` screen (#748): the projects, most recently active
//! first, and the selected one's detail as `show` prints it. The filter
//! narrows the table by `search`'s shallow match. Each pane's border names
//! the command that prints its data (ADR-507, note of 2026-10-02). It is a
//! pane of agent-tui's shell (ADR-504 §3), which draws the bars and takes
//! the mouse: a click selects a row, and the wheel moves the selection or
//! scrolls the detail under it.

use agent_tui::input::Input;
use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, MouseEvent};
use agent_tui::ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use agent_tui::ratatui::style::{Modifier, Style};
use agent_tui::ratatui::text::{Line, Span};
use agent_tui::ratatui::widgets::{Cell, HighlightSpacing, Paragraph, Row, Table, TableState, Wrap};
use agent_tui::ratatui::Frame as Draw;
use agent_tui::screen::Screen;
use agent_tui::theme::{self, Ground, Palette, Seg, Shape};
use agent_tui::{App, Binding, Keyed, Pane, PaneTab, Tone};
use anyhow::Result;

use super::{ellipsize_left, list_cells, scan_all, shallow_match, show_project, Env, Project};
use crate::cmd::screen_host::{self, pane, Open};

/// Open the screen on the terminal, or headless as `open` asks.
pub(crate) fn open(open: &Open) -> Result<()> {
    let (palette, shape) = screen_host::look(screen_host::depth_of(open.depth.as_deref())?);
    screen_host::show(Projects::new(&Env::user(), palette, shape).into_app(), open)
}

/// One project: what the table and the detail show of it.
struct Entry {
    project: Project,
    cells: [String; 4],
    detail: Vec<String>,
}

/// The projects screen's state: the pane the shell hosts.
pub(crate) struct ProjectsPane {
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
    /// Where the last frame drew the table and the detail.
    hits: (Rect, Rect),
}

impl ProjectsPane {
    fn new(env: &Env, palette: Palette, shape: Shape) -> ProjectsPane {
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
        ProjectsPane { palette, shape, entries, shown, sel: 0, filter: Input::new(), editing: false, scroll: 0, table: TableState::default(), hits: Default::default() }
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

impl Pane for ProjectsPane {
    fn palette(&self) -> Palette {
        self.palette
    }

    /// No tabs: the bar names the screen and the filter, as a trailer.
    fn tabs(&mut self) -> Vec<PaneTab> {
        Vec::new()
    }

    fn tab(&mut self) -> usize {
        0
    }

    fn set_tab(&mut self, _: usize) {}

    fn trailer(&mut self) -> Vec<Span<'static>> {
        let mut bar = self.shape.lozenge(&[Seg::on(" projects ", Ground::AccentDim)]);
        if self.editing || !self.filter.is_empty() {
            bar.push(Span::styled("  / ", theme::accent()));
            if self.editing {
                let rows = self.filter.rows(u16::MAX, theme::body());
                bar.extend(rows.into_iter().next().map(|l| l.spans).unwrap_or_default());
            } else {
                bar.push(Span::styled(self.filter.text().to_string(), theme::body()));
            }
        }
        bar
    }

    fn draw(&mut self, f: &mut Draw, area: Rect) {
        // The table takes its rows, up to 45% of the body; the detail the rest.
        let body = area.height;
        let table_h = (self.shown.len() as u16 + 3).clamp(3, (body * 45 / 100).max(3));
        let [list, detail] = Layout::vertical([Constraint::Length(table_h), Constraint::Min(5)]).areas(area);
        self.hits = (list, detail);
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

    }

    fn key(&mut self, k: KeyEvent) -> Keyed {
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
            return Keyed::Done;
        }
        match k.code {
            KeyCode::Esc if self.filter.is_empty() => return Keyed::Pass,
            KeyCode::Esc => self.clear_filter(),
            KeyCode::Char('/') => self.editing = true,
            // Shift scrolls the detail; the draw keeps it in range.
            KeyCode::Char('J') => self.scroll = self.scroll.saturating_add(1),
            KeyCode::Char('K') => self.scroll = self.scroll.saturating_sub(1),
            c => self.travel(match c {
                KeyCode::Char('k') => KeyCode::Up,
                KeyCode::Char('j') => KeyCode::Down,
                KeyCode::Char('g') => KeyCode::Home,
                KeyCode::Char('G') => KeyCode::End,
                c => c,
            }),
        }
        Keyed::Done
    }

    /// The wheel over the detail scrolls it; elsewhere it moves the
    /// selection, as the arrows do.
    fn wheel_at(&mut self, up: bool, at: Position) {
        if self.hits.1.contains(at) {
            self.scroll = if up { self.scroll.saturating_sub(1) } else { self.scroll.saturating_add(1) };
        } else {
            self.travel(if up { KeyCode::Up } else { KeyCode::Down });
        }
    }

    /// A click on a row selects it.
    fn click(&mut self, at: Position) {
        if let Some(i) = agent_tui::hit::row_at(self.hits.0, at, 1, self.table.offset()) {
            if i < self.shown.len() {
                self.select(i);
            }
        }
    }

    fn bindings(&self) -> Vec<Binding> {
        let mut out = vec![Binding::new("↑↓", "select")];
        if self.editing {
            out.extend([Binding::new("⏎", "keep"), Binding::new("esc", "clear")]);
        } else {
            out.extend([Binding::new("J/K", "detail"), Binding::new("/", "filter")]);
            if !self.filter.is_empty() {
                out.push(Binding::new("esc", "clear"));
            }
        }
        out.extend([
            Binding::help("PgUp PgDn g G", "move by ten, to the first, to the last"),
            Binding::help("click a row", "selects it; the wheel over the detail scrolls it"),
        ]);
        out
    }

    fn mode(&self) -> String {
        if self.editing { "filter" } else { "projects" }.into()
    }

    /// Where the selection is in the rows the filter keeps.
    fn status(&mut self) -> Option<(String, Tone)> {
        Some((format!("{}/{}", (self.sel + 1).min(self.shown.len()), self.shown.len()), Tone::Back))
    }

    /// While the filter is typed, every plain key is a letter of it.
    fn owns_text(&self) -> bool {
        self.editing
    }
}

/// The projects screen on the shell: [`ProjectsPane`] inside
/// `agent_tui::App`, run on the terminal as [`Projects::into_app`] and
/// driven headless as a [`Screen`]. It reads as the pane.
pub(crate) struct Projects {
    app: App,
}

impl Projects {
    pub(crate) fn new(env: &Env, palette: Palette, shape: Shape) -> Projects {
        Projects { app: App::with_pane("projects", ProjectsPane::new(env, palette, shape)).shape(shape) }
    }

    pub(crate) fn into_app(self) -> App {
        self.app
    }
}

impl std::ops::Deref for Projects {
    type Target = ProjectsPane;
    fn deref(&self) -> &ProjectsPane {
        self.app.pane_ref().expect("the projects pane")
    }
}

impl Screen for Projects {
    fn palette(&self) -> Palette {
        self.palette
    }

    fn draw(&mut self, f: &mut Draw) {
        Screen::draw(&mut self.app, f);
    }

    fn key(&mut self, k: KeyEvent) -> bool {
        Screen::key(&mut self.app, k)
    }

    fn mouse(&mut self, m: MouseEvent) {
        Screen::mouse(&mut self.app, m);
    }
}
