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

use crate::cmd::screen_host::pane;
use crate::cmd::stats::{self, StatsReport};
use crate::cmd::tune_precision::{self, Flag, WayPrecision};

/// The command that prints a tab's data for an agent (ADR-507 note of
/// 2026-10-02), on the tab's bottom border. It starts at the left, so a
/// narrow pane cuts the end of a long path and keeps the command whole.
pub(super) fn agent_hint(command: &str) -> Line<'static> {
    Line::styled(format!(" {command} "), theme::muted())
}

/// The report tabs over one scope: the judge's spend, usage and fire
/// precision. Each reads the event log's text it is given, so a live
/// screen reads the log once per change for all of them.
pub(crate) struct Reports {
    /// The event log's text the views read, kept so each recomputes only
    /// when its tab is drawn after a change: stats and precision take a
    /// fifth of a second each over a large log.
    content: String,
    /// Which of spend, stats and precision have not read `content` yet.
    stale: [bool; 3],
    spend: Spend,
    stats: Stats,
    precision: Precision,
}

impl Reports {
    /// The reports of the event log's `content` for `project`, or every
    /// project with `None`; `scope` names it in the tables' titles. The
    /// project is matched with its trailing slash trimmed, as the
    /// sessions tab matches it.
    pub(crate) fn new(content: &str, project: Option<&str>, scope: String) -> Reports {
        let project = project.map(|p| p.trim_end_matches('/'));
        let scope = scope.trim_end_matches('/').to_string();
        Reports {
            content: content.to_string(),
            stale: [true; 3],
            spend: Spend::new(Vec::new(), project, scope.clone()),
            stats: Stats::new("", project, scope.clone()),
            precision: Precision::new("", project, scope),
        }
    }

    /// The log read again: every view reads it when next shown.
    pub(crate) fn reload(&mut self, content: &str) {
        self.content = content.to_string();
        self.stale = [true; 3];
    }

    pub(crate) fn spend(&mut self) -> &mut Spend {
        if std::mem::take(&mut self.stale[0]) {
            self.spend.reload(&self.content);
        }
        &mut self.spend
    }

    pub(crate) fn stats(&mut self) -> &mut Stats {
        if std::mem::take(&mut self.stale[1]) {
            self.stats.reload(&self.content);
        }
        &mut self.stats
    }

    pub(crate) fn precision(&mut self) -> &mut Precision {
        if std::mem::take(&mut self.stale[2]) {
            self.precision.reload(&self.content);
        }
        &mut self.precision
    }

    /// Which of spend, stats and precision wait to read the log.
    #[cfg(test)]
    pub(crate) fn stale(&self) -> [bool; 3] {
        self.stale
    }

    /// These reports with `spend` in place of the log's, for a test.
    #[cfg(test)]
    pub(crate) fn with_spend(mut self, spend: Spend) -> Reports {
        self.spend = spend;
        self.stale[0] = false;
        self
    }
}

/// `command`, scoped as a report tab is: with `--project` when the tab
/// shows one project, else with `unscoped`, the flag that widens the
/// command to every project (empty when that is its default).
fn scoped(command: &str, project: Option<&str>, unscoped: &str) -> String {
    match project {
        Some(p) => format!("{command} --project {}", agent_tui::tree::quote(p)),
        None if unscoped.is_empty() => command.to_string(),
        None => format!("{command} {unscoped}"),
    }
}

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
    /// The command that prints these calls for an agent.
    hint: String,
    pub(crate) by: By,
    sel: usize,
    table: TableState,
}

impl Spend {
    /// The spend of `calls` in `project`, or of every project with `None`.
    /// `covers_since` is taken from all of `calls`, before the project
    /// filter: the log's start bounds every scope alike.
    pub(crate) fn new(calls: Vec<Call>, project: Option<&str>, scope: String) -> Spend {
        let hint = scoped("ways agent cost --json", project, "");
        let mut s = Spend { calls: Vec::new(), covers_since: None, project: project.map(str::to_string), scope, hint, by: By::Day, sel: 0, table: TableState::default() };
        s.take(calls);
        s
    }

    /// Keep the calls of the scope's project, matched as the sessions tab
    /// matches a session's.
    fn take(&mut self, calls: Vec<Call>) {
        self.covers_since = spend::covers_since(&calls);
        self.calls = spend::filter_project(calls, self.project.as_deref());
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
            f.render_widget(Paragraph::new(Line::styled("no judge calls recorded", theme::muted())).block(pane(title).title_bottom(agent_hint(&self.hint))), body);
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
            .block(pane(title).title_bottom(agent_hint(&self.hint)))
            .row_highlight_style(theme::selected())
            .highlight_symbol(Line::styled(theme::SELECTED_MARK, theme::accent()));
        f.render_stateful_widget(t, body, &mut self.table);
    }
}

/// A trigger in at most 13 columns, its kind kept: `semantic:bash:en` is
/// `sem:bash`, `semantic:embedding:en` is `sem:emb`, and a `bash` pattern
/// stays `bash`. Which kind fired a way off its domain decides the remedy:
/// narrow the pattern, or the vocabulary.
pub(super) fn short_trigger(trigger: &str) -> String {
    // Early history spells a semantic bash fire `bash:semantic:en`.
    if trigger.starts_with("bash:semantic") {
        return "sem:bash".into();
    }
    match trigger.strip_prefix("semantic:") {
        Some(rest) => {
            let lane = rest.split(':').next().unwrap_or(rest);
            let lane = match lane {
                "embedding" => "emb",
                "late-interaction" => "late",
                other => other,
            };
            format!("sem:{lane}")
        }
        None => trigger.to_string(),
    }
}

/// A list's cursor moved by a key: one row, a page, or an end.
fn moved(sel: usize, len: usize, k: KeyCode) -> usize {
    let last = len.saturating_sub(1);
    match k {
        KeyCode::Up | KeyCode::Char('k') => sel.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => (sel + 1).min(last),
        KeyCode::PageUp => sel.saturating_sub(10),
        KeyCode::PageDown => (sel + 10).min(last),
        KeyCode::Home | KeyCode::Char('g') => 0,
        KeyCode::End | KeyCode::Char('G') => last,
        _ => sel,
    }
}

/// The row of `way` in `ways` after a reload re-ranked them, else the
/// old row, so a live screen keeps the cursor on its way.
fn reselect<'a>(was: Option<String>, mut ways: impl Iterator<Item = &'a str>, sel: usize) -> usize {
    was.and_then(|w| ways.position(|x| x == w)).unwrap_or(sel)
}

/// How precisely each way fires over the scope, as `ways tune precision`
/// audits it: the share of its sessions that were off its domain, flagged
/// ways first, with the remedy for the selected one.
pub(crate) struct Precision {
    rows: Vec<WayPrecision>,
    project: Option<String>,
    scope: String,
    /// The command that prints this audit for an agent.
    hint: String,
    sel: usize,
    table: TableState,
}

impl Precision {
    /// The audit of the event log's `content` for `project`, or every
    /// project with `None`, at the CLI's default gates.
    pub(crate) fn new(content: &str, project: Option<&str>, scope: String) -> Precision {
        let hint = scoped("ways tune precision --json", project, "");
        let mut p = Precision { rows: Vec::new(), project: project.map(str::to_string), scope, hint, sel: 0, table: TableState::default() };
        p.reload(content);
        p
    }

    pub(crate) fn reload(&mut self, content: &str) {
        let was = self.rows.get(self.sel).map(|r| r.way.clone());
        self.rows = tune_precision::report(content, tune_precision::MIN_SESSIONS, tune_precision::FLAG_THRESHOLD, self.project.as_deref(), None);
        self.sel = reselect(was, self.rows.iter().map(|r| r.way.as_str()), self.sel);
    }

    pub(crate) fn key(&mut self, k: KeyCode) {
        self.sel = moved(self.sel, self.rows.len(), k);
    }

    pub(crate) fn draw(&mut self, f: &mut Draw, area: Rect) {
        let [head, body, remedy] = Layout::vertical([Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)]).areas(area);
        let flagged = self.rows.iter().filter(|r| !matches!(r.flag, Flag::Ok | Flag::LowN)).count();
        let line = vec![
            Span::styled("Fire precision", Style::new().add_modifier(Modifier::BOLD)),
            Span::styled(
                format!(" · {flagged} flagged · off-class ≥ {:.0}% over ≥ {} sessions", tune_precision::FLAG_THRESHOLD * 100.0, tune_precision::MIN_SESSIONS),
                theme::muted(),
            ),
        ];
        f.render_widget(Paragraph::new(Line::from(line)), head);
        let title = format!(" {} ways fired in {} ", self.rows.len(), self.scope);
        if self.rows.is_empty() {
            f.render_widget(Paragraph::new(Line::styled("no ways fired", theme::muted())).block(pane(title).title_bottom(agent_hint(&self.hint))), body);
            return;
        }
        // Below 100 columns the trigger is shortened and the spread is left
        // out, so the way keeps its room.
        let wide = area.width >= 100;
        let right = |t: String| Cell::from(Line::from(t).alignment(Alignment::Right));
        let mut header = vec![Cell::from("Way"), Cell::from("Flag"), right("Sess".into()), right("Off".into()), right("Irrel".into())];
        if wide {
            header.push(right("Spread".into()));
        }
        header.push(Cell::from("Off trigger"));
        let rows: Vec<Row> = self
            .rows
            .iter()
            .map(|r| {
                let flag = match r.flag {
                    Flag::MisTargeted | Flag::CrossCutting => Span::styled(r.flag.label(), theme::warn()),
                    _ => Span::styled(r.flag.label(), theme::muted()),
                };
                let mut cells = vec![Cell::from(r.way.clone()), Cell::from(flag), right(r.sessions.to_string()), right(r.off_class.to_string()), right(format!("{:.2}", r.irrelevance))];
                if wide {
                    cells.push(right(r.spread.to_string()));
                }
                cells.push(Cell::from(if wide { r.top_off_trigger.clone() } else { short_trigger(&r.top_off_trigger) }));
                Row::new(cells)
            })
            .collect();
        let mut widths = vec![Constraint::Min(20), Constraint::Length(13), Constraint::Length(4), Constraint::Length(3), Constraint::Length(5)];
        if wide {
            widths.push(Constraint::Length(6));
        }
        widths.push(Constraint::Length(if wide { 24 } else { 13 }));
        self.sel = self.sel.min(self.rows.len() - 1);
        self.table.select(Some(self.sel));
        let t = Table::new(rows, widths)
            .header(Row::new(header).style(Style::new().add_modifier(Modifier::BOLD)))
            .column_spacing(1)
            .block(pane(title).title_bottom(agent_hint(&self.hint)))
            .row_highlight_style(theme::selected())
            .highlight_symbol(Line::styled(theme::SELECTED_MARK, theme::accent()));
        f.render_stateful_widget(t, body, &mut self.table);
        let remedy_text = Line::from(vec![Span::styled("remedy ", theme::muted()), Span::raw(self.rows[self.sel].flag.remedy())]);
        f.render_widget(Paragraph::new(remedy_text), remedy);
    }
}

/// Usage over the scope, as `ways tune stats` reports it: the ways ranked
/// by fires, and beside them how the fires came (channel, scope, checks,
/// ways per hook invocation) and the selected way's split by model.
pub(crate) struct Stats {
    report: StatsReport,
    project: Option<String>,
    scope: String,
    /// The command that prints these stats for an agent.
    hint: String,
    sel: usize,
    table: TableState,
}

impl Stats {
    /// The stats of the event log's `content` for `project`, or every
    /// project with `None`.
    pub(crate) fn new(content: &str, project: Option<&str>, scope: String) -> Stats {
        let hint = scoped("ways tune stats --json", project, "--global");
        let mut s = Stats { report: StatsReport::default(), project: project.map(str::to_string), scope, hint, sel: 0, table: TableState::default() };
        s.reload(content);
        s
    }

    pub(crate) fn reload(&mut self, content: &str) {
        let was = self.report.by_way.get(self.sel).map(|(w, _)| w.clone());
        self.report = stats::report(content, None, self.project.as_deref());
        self.sel = reselect(was, self.report.by_way.iter().map(|(w, _)| w.as_str()), self.sel);
    }

    pub(crate) fn key(&mut self, k: KeyCode) {
        self.sel = moved(self.sel, self.report.by_way.len(), k);
    }

    pub(crate) fn draw(&mut self, f: &mut Draw, area: Rect) {
        let r = &self.report;
        let [head, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(3)]).areas(area);
        let day = |t: &Option<String>| t.as_deref().and_then(|t| t.get(..10)).unwrap_or("?").to_string();
        let line = vec![
            Span::styled("Usage", Style::new().add_modifier(Modifier::BOLD)),
            Span::styled(
                format!(" · {} sessions · {} fires · {} re-disclosures · {} → {}", r.sessions, r.fires, r.redisclosures, day(&r.first_ts), day(&r.last_ts)),
                theme::muted(),
            ),
        ];
        f.render_widget(Paragraph::new(Line::from(line)), head);
        // The side pane fits the longest channel name from 100 columns;
        // below, the way column keeps the room and that one name is cut.
        let side_width = if area.width >= 100 { 40 } else { 32 };
        let [left, side] = Layout::horizontal([Constraint::Min(30), Constraint::Length(side_width)]).areas(body);

        let title = format!(" {} ways in {} ", r.by_way.len(), self.scope);
        if r.by_way.is_empty() {
            f.render_widget(Paragraph::new(Line::styled("no ways fired", theme::muted())).block(pane(title).title_bottom(agent_hint(&self.hint))), left);
        } else {
            let rows: Vec<Row> =
                r.by_way.iter().map(|(way, n)| Row::new(vec![Cell::from(way.clone()), Cell::from(Line::from(n.to_string()).alignment(Alignment::Right))])).collect();
            let header = Row::new(vec![Cell::from("Way"), Cell::from(Line::from("Fires").alignment(Alignment::Right))]).style(Style::new().add_modifier(Modifier::BOLD));
            self.sel = self.sel.min(r.by_way.len() - 1);
            self.table.select(Some(self.sel));
            let t = Table::new(rows, [Constraint::Min(10), Constraint::Length(5)])
                .header(header)
                .column_spacing(1)
                .block(pane(title).title_bottom(agent_hint(&self.hint)))
                .row_highlight_style(theme::selected())
                .highlight_symbol(Line::styled(theme::SELECTED_MARK, theme::accent()));
            f.render_stateful_widget(t, left, &mut self.table);
        }

        let bold = |t: &str| Line::styled(t.to_string(), Style::new().add_modifier(Modifier::BOLD));
        let count = |n: u32, k: &str| Line::from(format!("{n:>7}  {k}"));
        let mut lines: Vec<Line> = vec![bold("Channel")];
        lines.extend(r.by_trigger.iter().map(|(k, n)| count(*n, k)));
        lines.push(bold("Scope"));
        lines.extend(r.by_scope.iter().map(|(k, n)| count(*n, k)));
        lines.push(bold("Checks"));
        lines.push(count(r.check_fires, &format!("fired, {} anchored", r.check_anchored)));
        lines.push(Line::from(format!("         mean distance {:.1}", r.check_avg_distance)));
        lines.push(bold("Ways per invocation 1·2·3·4+"));
        for (channel, l) in &r.ways_per_invocation {
            let b = l.buckets.iter().map(u32::to_string).collect::<Vec<_>>().join("·");
            lines.push(Line::from(format!("  {channel} {b}, max {}", l.max)));
        }
        if let Some(split) = r.by_way.get(self.sel).and_then(|(way, _)| r.by_way_model.get(way)) {
            lines.push(bold("Selected way by model"));
            lines.extend(split.iter().map(|(m, n)| count(*n, m)));
        }
        f.render_widget(Paragraph::new(lines).block(pane(" how they fired ")), side);
    }
}
