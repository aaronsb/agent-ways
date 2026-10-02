//! The fires tab: each semantic fire of the session as score, way and the
//! text it matched, the borderline fires first, as `ways session fires`
//! lists them.

use agent_tui::ratatui::crossterm::event::KeyCode;
use agent_tui::ratatui::layout::{Alignment, Constraint, Layout};
use agent_tui::ratatui::style::{Modifier, Style};
use agent_tui::ratatui::text::{Line, Span};
use agent_tui::ratatui::widgets::{Cell, HighlightSpacing, Paragraph, Row, Table, TableState};
use agent_tui::ratatui::Frame as Draw;
use agent_tui::theme::{self, Ground, Shape};
use agent_tui::timeline::key_bar;

use super::report::agent_hint;
use crate::cmd::screen_host::pane;
use super::SemanticFire;

/// A session's semantic fires and the one selected.
#[derive(Default)]
pub(crate) struct Fires {
    list: Vec<SemanticFire>,
    sel: usize,
    table: TableState,
}

impl Fires {
    /// Take the fires read again, the selection kept on its fire: a new
    /// lower-scoring fire sorts in above it and would otherwise move it.
    pub(crate) fn set(&mut self, list: Vec<SemanticFire>) {
        let kept = self.list.get(self.sel).and_then(|was| list.iter().position(|f| f == was));
        self.sel = kept.unwrap_or(self.sel).min(list.len().saturating_sub(1));
        self.list = list;
    }

    pub(crate) fn key(&mut self, k: KeyCode) {
        let last = self.list.len().saturating_sub(1);
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

    /// Draw the tab for `session`. `back` says Esc returns to a sessions
    /// tab; without one it ends the screen, and the bar names `q` alone.
    pub(crate) fn draw(&mut self, f: &mut Draw, session: &str, tab_line: Line<'static>, shape: Shape, back: bool) {
        let [bar, head, body, status] = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)]).areas(f.area());
        f.render_widget(Paragraph::new(tab_line), bar);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Session ", Style::new().add_modifier(Modifier::BOLD)),
                Span::raw(super::short_id(session)),
                Span::styled("  lowest score first · ↻ a re-disclosure", theme::muted()),
            ])),
            head,
        );
        let title = format!(" {} semantic fires ", self.list.len());
        if self.list.is_empty() {
            let none = Line::styled("no semantic fires: keyword and state fires carry no score or surface", theme::muted());
            f.render_widget(Paragraph::new(none).block(pane(title)), body);
        } else {
            let right = |t: String| Cell::from(Line::from(t).alignment(Alignment::Right));
            let header = Row::new(vec![right("Score".into()), Cell::from(""), Cell::from("Way"), Cell::from("Surface")]).style(Style::new().add_modifier(Modifier::BOLD));
            let rows: Vec<Row> = self
                .list
                .iter()
                .map(|x| {
                    Row::new(vec![
                        right(format!("{:.3}", x.score)),
                        Cell::from(if x.redisclosed { "↻" } else { " " }),
                        Cell::from(x.way.clone()),
                        Cell::from(Span::styled(x.surface.clone(), theme::muted())),
                    ])
                })
                .collect();
            let widths = [Constraint::Length(5), Constraint::Length(1), Constraint::Percentage(40), Constraint::Min(10)];
            self.sel = self.sel.min(self.list.len() - 1);
            self.table.select(Some(self.sel));
            let t = Table::new(rows, widths)
                .header(header)
                .column_spacing(1)
                .block(pane(title).title_bottom(agent_hint("ways session fires --json")))
                .row_highlight_style(theme::selected())
                .highlight_symbol(Line::styled(theme::SELECTED_MARK, theme::accent()))
                .highlight_spacing(HighlightSpacing::Always);
            f.render_stateful_widget(t, body, &mut self.table);
        }
        let right = vec![Span::styled(format!("{}/{}", (self.sel + 1).min(self.list.len()), self.list.len()), theme::muted())];
        let mut keys = vec![("↑↓", "select")];
        if back {
            keys.push(("esc", "sessions"));
        }
        keys.push(("q", "quit"));
        f.render_widget(Paragraph::new(key_bar(shape, "fires", Ground::Accent, &keys, right, status.width)), status);
    }
}
