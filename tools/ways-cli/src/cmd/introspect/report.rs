//! The report tabs of the session screen (#738): aggregates over the
//! sessions in scope, as `ways agent cost` and the `ways tune` reports
//! print them. The judge's spend comes from the shared aggregation in
//! [`ways_agent_core::spend`].

use agent_tui::ratatui::crossterm::event::KeyCode;
use agent_tui::ratatui::layout::{Alignment, Constraint, Layout, Rect};
use agent_tui::ratatui::style::{Modifier, Style};
use agent_tui::ratatui::text::{Line, Span};
use agent_tui::ratatui::widgets::{Cell, Paragraph, Row, Table, TableState};
use agent_tui::ratatui::Frame as Draw;
use agent_tui::theme;
use ways_agent_core::spend::{self, By, Call, Group};

use super::screen::pane;
use crate::cmd::tune_precision::{self, Flag, WayPrecision};

/// The judge's spend over the sessions in scope, by day or by month,
/// newest first, in the shape of `npx ccusage`.
pub(crate) struct Spend {
    calls: Vec<Call>,
    /// Where the scope's history begins: the earliest call the event log
    /// still holds, before compaction dropped older ones.
    covers_since: Option<String>,
    /// The project the calls are kept for; `None` keeps every project's.
    project: Option<String>,
    /// The scope the calls were taken from, as the sessions tab names it.
    scope: String,
    pub(crate) by: By,
    sel: usize,
    table: TableState,
}

impl Spend {
    /// The spend of `calls` in `project`, or of every project with `None`.
    /// `covers_since` is taken from all of `calls`, before the project
    /// filter: the log's start bounds every scope alike.
    pub(crate) fn new(calls: Vec<Call>, project: Option<&str>, scope: String) -> Spend {
        let mut s = Spend { calls: Vec::new(), covers_since: None, project: project.map(str::to_string), scope, by: By::Day, sel: 0, table: TableState::default() };
        s.take(calls);
        s
    }

    /// Keep the calls of the scope's project, matched as the sessions tab
    /// matches a session's.
    fn take(&mut self, calls: Vec<Call>) {
        self.covers_since = spend::covers_since(&calls);
        let project = self.project.as_deref();
        self.calls = calls.into_iter().filter(|c| project.is_none_or(|p| super::scope::project_matches(&c.project, p))).collect();
    }

    /// Take the calls in the event log's `content`, read again by a live
    /// screen when it changed.
    pub(crate) fn reload(&mut self, content: &str) {
        self.take(spend::parse_log(content));
    }

    fn groups(&self) -> Vec<Group> {
        spend::aggregate(&self.calls, self.by)
    }

    pub(crate) fn key(&mut self, k: KeyCode) {
        let last = self.groups().len().saturating_sub(1);
        match k {
            KeyCode::Char('m') => {
                self.by = if self.by == By::Day { By::Month } else { By::Day };
                self.sel = 0;
            }
            KeyCode::Up | KeyCode::Char('k') => self.sel = self.sel.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.sel = (self.sel + 1).min(last),
            KeyCode::Home | KeyCode::Char('g') => self.sel = 0,
            KeyCode::End | KeyCode::Char('G') => self.sel = last,
            _ => {}
        }
    }

    /// The key bar's entries for this tab.
    pub(crate) fn keys(&self) -> Vec<(&'static str, &'static str)> {
        vec![("↑↓", "select"), ("m", if self.by == By::Day { "by month" } else { "by day" })]
    }

    pub(crate) fn draw(&mut self, f: &mut Draw, area: Rect) {
        let [head, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(3)]).areas(area);
        let unit = if self.by == By::Day { "day" } else { "month" };
        let mut line = vec![Span::styled(format!("Judge spend by {unit}"), Style::new().add_modifier(Modifier::BOLD))];
        if let Some(d) = &self.covers_since {
            line.push(Span::styled(format!(" · the log holds calls from {}", d.get(..10).unwrap_or(d)), theme::muted()));
        }
        f.render_widget(Paragraph::new(Line::from(line)), head);

        let title = format!(" {} judge calls in {} ", self.calls.len(), self.scope);
        let groups = self.groups();
        if groups.is_empty() {
            f.render_widget(Paragraph::new(Line::styled("no judge calls recorded", theme::muted())).block(pane(title)), body);
            return;
        }
        let right = |t: String| Cell::from(Line::from(t).alignment(Alignment::Right));
        let row = |g: &Group, key: String| {
            Row::new(vec![
                Cell::from(key),
                right(g.calls.to_string()),
                right(g.input_tokens.to_string()),
                right(g.output_tokens.to_string()),
                right((g.cache_read_tokens + g.cache_write_tokens).to_string()),
                right(g.tokens_short()),
                right(g.cost_short()),
            ])
        };
        let header = Row::new(vec![
            Cell::from(if self.by == By::Day { "Day" } else { "Month" }),
            right("Calls".into()),
            right("Input".into()),
            right("Output".into()),
            // Cache reads and writes in one column, so the cost fits at 80.
            right("Cache".into()),
            right("Tokens".into()),
            right("Cost".into()),
        ])
        .style(Style::new().add_modifier(Modifier::BOLD));
        let mut rows: Vec<Row> = groups.iter().map(|g| row(g, g.key.clone())).collect();
        rows.push(row(&spend::total(&self.calls), "total".into()).style(Style::new().add_modifier(Modifier::BOLD)));
        let widths = [
            Constraint::Length(10),
            Constraint::Length(5),
            Constraint::Length(9),
            Constraint::Length(8),
            Constraint::Length(9),
            Constraint::Length(6),
            Constraint::Length(21),
            // The slack, so the cost stays beside the tokens.
            Constraint::Fill(1),
        ];
        self.sel = self.sel.min(groups.len() - 1);
        self.table.select(Some(self.sel));
        let t = Table::new(rows, widths)
            .header(header)
            .column_spacing(1)
            .block(pane(title))
            .row_highlight_style(theme::selected())
            .highlight_symbol(Line::styled(theme::SELECTED_MARK, theme::accent()));
        f.render_stateful_widget(t, body, &mut self.table);
    }
}

/// How precisely each way fires over the scope, as `ways tune precision`
/// audits it: the share of its sessions that were off its domain, flagged
/// ways first, with the remedy for the selected one.
pub(crate) struct Precision {
    rows: Vec<WayPrecision>,
    project: Option<String>,
    scope: String,
    sel: usize,
    table: TableState,
}

impl Precision {
    /// The audit of the event log's `content` for `project`, or every
    /// project with `None`, at the CLI's default gates.
    pub(crate) fn new(content: &str, project: Option<&str>, scope: String) -> Precision {
        let mut p = Precision { rows: Vec::new(), project: project.map(str::to_string), scope, sel: 0, table: TableState::default() };
        p.reload(content);
        p
    }

    pub(crate) fn reload(&mut self, content: &str) {
        self.rows = tune_precision::report(content, tune_precision::MIN_SESSIONS, tune_precision::FLAG_THRESHOLD, self.project.as_deref(), None);
    }

    pub(crate) fn key(&mut self, k: KeyCode) {
        let last = self.rows.len().saturating_sub(1);
        self.sel = match k {
            KeyCode::Up | KeyCode::Char('k') => self.sel.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => (self.sel + 1).min(last),
            KeyCode::PageUp => self.sel.saturating_sub(10),
            KeyCode::PageDown => (self.sel + 10).min(last),
            KeyCode::Home | KeyCode::Char('g') => 0,
            KeyCode::End | KeyCode::Char('G') => last,
            _ => self.sel,
        };
    }

    pub(crate) fn draw(&mut self, f: &mut Draw, area: Rect) {
        let [head, body, remedy] = Layout::vertical([Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)]).areas(area);
        let flagged = self.rows.iter().filter(|r| !matches!(r.flag, Flag::Ok | Flag::LowN)).count();
        let line = vec![
            Span::styled("Fire precision", Style::new().add_modifier(Modifier::BOLD)),
            Span::styled(
                format!(
                    " · {flagged} flagged · off-class ≥ {:.0}% over ≥ {} sessions · `ways tune precision --json` for an agent",
                    tune_precision::FLAG_THRESHOLD * 100.0,
                    tune_precision::MIN_SESSIONS
                ),
                theme::muted(),
            ),
        ];
        f.render_widget(Paragraph::new(Line::from(line)), head);
        let title = format!(" {} ways fired in {} ", self.rows.len(), self.scope);
        if self.rows.is_empty() {
            f.render_widget(Paragraph::new(Line::styled("no ways fired", theme::muted())).block(pane(title)), body);
            return;
        }
        let right = |t: String| Cell::from(Line::from(t).alignment(Alignment::Right));
        let header = Row::new(vec![
            Cell::from("Way"),
            Cell::from("Flag"),
            right("Sessions".into()),
            right("Off".into()),
            right("Irrel".into()),
            right("Spread".into()),
            Cell::from("Top off trigger"),
        ])
        .style(Style::new().add_modifier(Modifier::BOLD));
        let rows: Vec<Row> = self
            .rows
            .iter()
            .map(|r| {
                let flag = match r.flag {
                    Flag::MisTargeted | Flag::CrossCutting => Span::styled(r.flag.label(), theme::warn()),
                    _ => Span::styled(r.flag.label(), theme::muted()),
                };
                Row::new(vec![
                    Cell::from(r.way.clone()),
                    Cell::from(flag),
                    right(r.sessions.to_string()),
                    right(r.off_class.to_string()),
                    right(format!("{:.2}", r.irrelevance)),
                    right(r.spread.to_string()),
                    Cell::from(r.top_off_trigger.clone()),
                ])
            })
            .collect();
        let widths = [
            Constraint::Min(20),
            Constraint::Length(13),
            Constraint::Length(8),
            Constraint::Length(4),
            Constraint::Length(5),
            Constraint::Length(6),
            Constraint::Length(15),
        ];
        self.sel = self.sel.min(self.rows.len() - 1);
        self.table.select(Some(self.sel));
        let t = Table::new(rows, widths)
            .header(header)
            .column_spacing(1)
            .block(pane(title))
            .row_highlight_style(theme::selected())
            .highlight_symbol(Line::styled(theme::SELECTED_MARK, theme::accent()));
        f.render_stateful_widget(t, body, &mut self.table);
        let r = &self.rows[self.sel];
        f.render_widget(Paragraph::new(Line::from(vec![Span::styled(format!("{}: ", r.way), theme::muted()), Span::raw(r.flag.remedy())])), remedy);
    }
}
