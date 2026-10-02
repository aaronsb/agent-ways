//! The sessions tab: the sessions in scope, newest first, one of which
//! the screen opens.

use agent_tui::ratatui::crossterm::event::KeyCode;
use agent_tui::ratatui::layout::{Alignment, Constraint, Layout};
use agent_tui::ratatui::style::{Modifier, Style};
use agent_tui::ratatui::text::{Line, Span};
use agent_tui::ratatui::widgets::{Cell, HighlightSpacing, Paragraph, Row, Table, TableState};
use agent_tui::ratatui::Frame as Draw;
use agent_tui::theme::{self, Ground, Shape};
use agent_tui::timeline::key_bar;

use crate::cmd::screen_host::pane;
use super::sessions::SessionInfo;

/// The sessions to pick from, newest first.
pub(crate) struct Picker {
    pub(crate) sessions: Vec<SessionInfo>,
    /// Where the sessions come from: a project, or every project.
    pub(crate) scope: String,
    pub(crate) sel: usize,
    table: TableState,
}

impl Picker {
    pub(crate) fn new(mut sessions: Vec<SessionInfo>, scope: String) -> Picker {
        sessions.reverse();
        Picker { sessions, scope, sel: 0, table: TableState::default() }
    }

    pub(crate) fn key(&mut self, k: KeyCode) {
        let last = self.sessions.len().saturating_sub(1);
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
}


pub(crate) fn draw_picker(f: &mut Draw, p: &mut Picker, tab_line: Line<'static>, shape: Shape, msg: &str) {
    let [bar, main, status] = Layout::vertical([Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)]).areas(f.area());
    f.render_widget(Paragraph::new(tab_line), bar);
    // The count first: a long scope is cut at its end, never the count.
    let title = format!(" {} sessions in {} ", p.sessions.len(), p.scope);
    if p.sessions.is_empty() {
        f.render_widget(Paragraph::new(Line::styled("no sessions recorded", theme::muted())).block(pane(title)), main);
    } else {
        let right = |t: String| Cell::from(Line::from(t).alignment(Alignment::Right));
        let header = Row::new(vec![
            Cell::from("Session"),
            Cell::from("Date"),
            Cell::from("Project"),
            right("Events".into()),
            right("Ways".into()),
            right("Duration".into()),
            Cell::from("Transcript"),
        ])
        .style(Style::new().add_modifier(Modifier::BOLD));
        let rows: Vec<Row> = p
            .sessions
            .iter()
            .map(|s| {
                let project = s.project.rsplit('/').next().unwrap_or(&s.project).to_string();
                let date = s.ts.replace('T', " ");
                let transcript = if s.transcript { Span::styled("yes", theme::ok()) } else { Span::styled("gone", theme::muted()) };
                Row::new(vec![
                    Cell::from(s.id.chars().take(12).collect::<String>()),
                    Cell::from(date.chars().take(16).collect::<String>()),
                    Cell::from(project),
                    right(s.event_count.to_string()),
                    right(s.way_fires.to_string()),
                    right(agent_fmt::when::duration(s.duration_secs)),
                    Cell::from(transcript),
                ])
            })
            .collect();
        let widths = [
            Constraint::Length(12),
            Constraint::Length(16),
            Constraint::Min(8),
            Constraint::Length(6),
            Constraint::Length(5),
            Constraint::Length(8),
            Constraint::Length(10),
        ];
        let t = Table::new(rows, widths)
            .header(header)
            .column_spacing(1)
            .block(pane(title))
            .row_highlight_style(theme::selected())
            .highlight_symbol(Line::styled(theme::SELECTED_MARK, theme::accent()))
            .highlight_spacing(HighlightSpacing::Always);
        p.table.select(Some(p.sel));
        f.render_stateful_widget(t, main, &mut p.table);
    }
    let right = if msg.is_empty() {
        vec![Span::styled(format!("{}/{}", (p.sel + 1).min(p.sessions.len()), p.sessions.len()), theme::muted())]
    } else {
        vec![Span::styled(msg.to_string(), theme::err().add_modifier(Modifier::BOLD))]
    };
    let keys = [("↑↓", "select"), ("⏎", "replay"), ("q", "quit")];
    f.render_widget(Paragraph::new(key_bar(shape, "pick", Ground::Accent, &keys, right, status.width)), status);
}

