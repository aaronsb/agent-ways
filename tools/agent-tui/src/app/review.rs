//! Drawing for review mode: the tree of one tab's pending rows with a progress
//! gutter, and the detail pane that describes the change under the cursor.
//! Review keeps the browser's layout; the rows are what differs.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{HighlightSpacing, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use super::apply::{self, RKind, RRow, St};
use super::render::{elide, pane};
use super::theme;
use super::{App, Mode};
use crate::tree;

/// The columns before each row's text: a status glyph and a space.
pub(super) const GUTTER: u16 = 2;

/// Width of a detail field's label column.
pub(super) const LABEL: usize = 10;

/// A label and its value, the value wrapped to `room` columns under itself.
pub(super) fn fields(label: &str, value: &str, style: Style, room: usize, dim: Style) -> Vec<Line<'static>> {
    let chars: Vec<char> = value.chars().collect();
    let mut chunks = chars.chunks(room.max(1)).map(|c| c.iter().collect::<String>());
    let mut out = vec![Line::from(vec![Span::styled(format!("{label:<LABEL$}"), dim), Span::styled(chunks.next().unwrap_or_default(), style)])];
    out.extend(chunks.map(|c| Line::from(vec![Span::raw(" ".repeat(LABEL)), Span::styled(c, style)])));
    out
}

impl App {
    /// The rows the cursor can reach in `tab`: those of the run in flight, or
    /// live ones, without what a closed group hides.
    pub(super) fn review_view(&self, tab: usize) -> Vec<RRow> {
        let rows = match &self.mode {
            Mode::Review { run: Some(r), .. } => r.rows.clone(),
            _ => apply::review_rows(&self.roots, &self.queue, tab),
        };
        apply::visible(&rows, &self.closed)
    }

    /// The state a row's gutter shows, if it shows one: the run's, or after a
    /// stopped run the failed step marked and the rest pending.
    fn mark(&self, tab: usize, row: &RRow) -> Option<St> {
        match (&self.mode, &self.failure) {
            (Mode::Review { run: Some(r), .. }, _) => r.step_of(row).map(|i| r.steps[i].state),
            (_, Some(f)) if f.tab == tab && row.is_item() => Some(if f.marks(row) { St::Failed } else { St::Pending }),
            _ => None,
        }
    }

    pub(super) fn draw_review_tree(&mut self, f: &mut Frame, area: Rect, tab: usize) {
        let rows = self.review_view(tab);
        let cursor = self.rcursor[tab].min(rows.len().saturating_sub(1));
        self.rcursor[tab] = cursor;
        let room = area.width.saturating_sub(4 + GUTTER) as usize;
        let value_col = room / 2;
        let items: Vec<ListItem> = rows
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let (glyph, style) = match self.mark(tab, r) {
                    None => (" ", Style::new()),
                    Some(St::Pending) => ("·", theme::read_only()),
                    Some(St::Running) => ("◐", theme::accent().add_modifier(Modifier::BOLD)),
                    Some(St::Done) => ("✓", theme::ok()),
                    Some(St::Failed) => ("✗", theme::err().add_modifier(Modifier::BOLD)),
                };
                let marker = match (r.toggles(), self.closed.contains(&r.path)) {
                    (false, _) => "  ",
                    (true, false) => "▾ ",
                    (true, true) => "▸ ",
                };
                let indent = format!("{}{}", "  ".repeat(r.depth), marker);
                let mut spans = vec![Span::styled(format!("{glyph} "), style)];
                match &r.kind {
                    RKind::Node { own, below, .. } => {
                        let left = format!("{indent}{}", r.name);
                        let bold = if own.is_none() { Style::new().add_modifier(Modifier::BOLD) } else { Style::new() };
                        let pad = value_col.saturating_sub(left.chars().count()).max(1);
                        spans.extend([Span::styled(left, bold), Span::raw(" ".repeat(pad))]);
                        if let Some(c) = own {
                            let half = room.saturating_sub(value_col + 3) / 2;
                            spans.extend([
                                Span::styled(elide(&c.from, half), theme::was()),
                                Span::raw(" → "),
                                Span::styled(elide(&c.to, half), theme::will()),
                            ]);
                        } else {
                            spans.push(Span::styled(format!("●{below}"), theme::warn()));
                        }
                    }
                    RKind::Queued { count } => {
                        spans.extend([Span::styled(format!("{indent}{}", r.name), Style::new().add_modifier(Modifier::BOLD)), Span::styled(format!("  ●{count}"), theme::queued())]);
                    }
                    RKind::Action { n, command, confirm, .. } => {
                        let tail = if *confirm { "  ▲" } else { "" };
                        let head = format!("{indent}{n}. $ ");
                        let text = elide(command, room.saturating_sub(head.chars().count() + tail.chars().count()));
                        spans.extend([Span::raw(head), Span::styled(text, theme::queued())]);
                        if *confirm {
                            spans.push(Span::styled(tail, theme::warn().add_modifier(Modifier::BOLD)));
                        }
                    }
                }
                ListItem::new(Line::from(spans)).style(if i == cursor { theme::selected_text() } else { Style::new() })
            })
            .collect();
        let block = pane(format!("{} — review ", self.title.trim_end()));
        let inner = block.inner(area);
        self.hits.list = inner;
        if rows.is_empty() {
            f.render_widget(block, area);
            f.render_widget(Paragraph::new(Line::styled(format!(" nothing pending in {}", self.roots[tab].name), theme::hint())), inner);
            return;
        }
        let list = List::new(items)
            .block(block)
            .highlight_style(theme::selected())
            .highlight_symbol(Line::styled(theme::SELECTED_MARK, theme::accent()))
            .highlight_spacing(HighlightSpacing::Always);
        self.list.select(Some(cursor));
        f.render_stateful_widget(list, area, &mut self.list);
    }

    /// Describes the change under the cursor, and while it runs or after it
    /// failed, its step.
    pub(super) fn draw_review_detail(&self, f: &mut Frame, area: Rect, tab: usize) {
        let rows = self.review_view(tab);
        let dim = theme::muted();
        let bold = Style::new().add_modifier(Modifier::BOLD);
        let room = (area.width as usize).saturating_sub(2 + LABEL);
        let field = |k: &str, v: String| fields(k, &v, Style::new(), room, dim);
        let mut lines = Vec::new();
        let Some(r) = rows.get(self.rcursor[tab]) else {
            lines.push(Line::styled(format!("nothing pending in {}", self.roots[tab].name), theme::hint()));
            return f.render_widget(Paragraph::new(lines).block(pane("detail")), area);
        };
        match &r.kind {
            RKind::Node { own, below, files } => {
                let n = tree::get(&self.roots, &r.path);
                lines.extend([Line::styled(tree::key(&self.roots, &r.path), bold), Line::raw("")]);
                if !n.doc.is_empty() {
                    lines.extend(n.doc.lines().map(|l| Line::raw(l.to_string())));
                    lines.push(Line::raw(""));
                }
                let plural = |c: usize| format!("{c} change{}", if c == 1 { "" } else { "s" });
                match (own, &n.setting) {
                    (Some(c), Some(s)) => {
                        lines.push(Line::from(vec![
                            Span::styled(format!("{:<LABEL$}", "change"), dim),
                            Span::styled(c.from.clone(), theme::was()),
                            Span::raw(" → "),
                            Span::styled(c.to.clone(), theme::will()),
                        ]));
                        lines.extend(field("from", format!("{} layer", s.source)));
                        if !c.layer.is_empty() {
                            lines.extend(field("to", format!("{} layer", c.layer)));
                        }
                        lines.extend(field("file", c.file.clone()));
                        lines.extend(field("key", c.key.clone()));
                        if *below > 0 {
                            lines.extend([Line::raw(""), Line::styled(format!("{} below", plural(*below)), dim)]);
                        }
                    }
                    _ => {
                        lines.push(Line::styled(format!("{} under this group", plural(*below)), dim));
                        lines.push(Line::raw(""));
                        lines.push(Line::styled(if files.len() == 1 { "file it writes" } else { "files it writes" }, bold));
                        lines.extend(files.iter().map(|file| Line::raw(format!("  {file}"))));
                    }
                }
            }
            RKind::Queued { count } => {
                lines.push(Line::styled("queued", bold));
                lines.push(Line::raw(""));
                lines.push(Line::raw(format!("{count} command{} to run, in order, after the value writes.", if *count == 1 { "" } else { "s" })));
                lines.push(Line::styled("Select one to see what it does.", dim));
            }
            RKind::Action { command, confirm, key, label, .. } => {
                lines.extend([Line::styled(format!("$ {command}"), theme::queued().add_modifier(Modifier::BOLD)), Line::raw("")]);
                let action = tree::find(&self.roots, key).and_then(|n| n.actions.iter().find(|a| a.label == *label));
                if let Some(a) = action.filter(|a| !a.doc.is_empty()) {
                    lines.extend([Line::raw(a.doc.clone()), Line::raw("")]);
                }
                lines.extend(field("queued by", key.clone()));
                lines.extend(if *confirm {
                    fields("asks", "yes: it is destructive or reconciles", theme::warn().add_modifier(Modifier::BOLD), room, dim)
                } else {
                    field("asks", "no".into())
                });
                if let Some(a) = action.filter(|a| !a.touches.is_empty()) {
                    lines.extend(field("touches", a.touches.clone()));
                }
            }
        }
        let step = match (&self.mode, &self.failure) {
            (Mode::Review { run: Some(run), .. }, _) => run.step_of(r).map(|i| (run.steps[i].state, run.steps[i].text.clone(), run.error(i))),
            (_, Some(fl)) if fl.tab == tab && fl.marks(r) => Some((St::Failed, fl.text.clone(), fl.error.clone())),
            _ => None,
        };
        if let Some((state @ (St::Running | St::Failed), text, error)) = step {
            lines.push(Line::raw(""));
            if state == St::Failed {
                lines.extend(fields("step", &text, theme::err(), room, dim));
                lines.extend(fields("error", &error, theme::err().add_modifier(Modifier::BOLD), room, dim));
            } else {
                lines.extend(fields("step", &text, theme::accent(), room, dim));
            }
        }
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(pane("detail")), area);
    }
}
