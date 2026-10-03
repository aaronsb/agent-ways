//! The sessions tab: the sessions in scope, newest first, one of which
//! the screen opens. A session whose transcript is being written to is
//! marked live, from the stat [`Sampler`] (#780), and the session the
//! screen was opened from is marked as this session.

use std::time::Duration;

use agent_tui::ratatui::crossterm::event::KeyCode;
use agent_tui::ratatui::layout::{Alignment, Constraint, Layout};
use agent_tui::ratatui::style::{Modifier, Style};
use agent_tui::ratatui::text::{Line, Span};
use agent_tui::ratatui::widgets::{Cell, HighlightSpacing, Paragraph, Row, Table, TableState};
use agent_tui::ratatui::Frame as Draw;
use agent_tui::theme::{self, Ground, Shape};
use agent_tui::timeline::key_bar;

use crate::cmd::screen_host::pane;
use super::live::{Clock, Sampler, Stat, SAMPLE_TICK};
use super::sessions::SessionInfo;

/// The mark a live session's row carries.
pub(crate) const LIVE_MARK: &str = "●";

/// The sessions to pick from, newest first.
pub(crate) struct Picker {
    pub(crate) sessions: Vec<SessionInfo>,
    /// Where the sessions come from: a project, or every project.
    pub(crate) scope: String,
    pub(crate) sel: usize,
    /// The transcripts' stats, one schedule per session in list order.
    pub(crate) sampler: Sampler,
    /// The session the screen was opened from (`CLAUDE_CODE_SESSION_ID`).
    pub(crate) own: Option<String>,
    table: TableState,
}

impl Picker {
    /// The list, its transcripts not yet stated: nothing is marked live
    /// until [`Picker::watching`] states them.
    pub(crate) fn new(mut sessions: Vec<SessionInfo>, scope: String) -> Picker {
        sessions.reverse();
        let sampler = Sampler::idle(sessions.len());
        Picker { sessions, scope, sel: 0, sampler, own: None, table: TableState::default() }
    }

    /// State each session's transcript now, and re-state it on its backoff
    /// while the list is shown.
    pub(crate) fn watching(mut self, stat: Stat, clock: Clock) -> Picker {
        let paths = self.sessions.iter().map(|s| s.transcript_path.clone()).collect();
        self.sampler = Sampler::new(paths, stat, clock);
        self
    }

    /// Mark `id` as the session the screen was opened from.
    pub(crate) fn own(mut self, id: Option<String>) -> Picker {
        self.own = id;
        self
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

    /// Put the cursor on session `id`, when it is listed.
    pub(crate) fn select(&mut self, id: &str) {
        if let Some(i) = self.sessions.iter().position(|s| s.id == id) {
            self.sel = i;
        }
    }

    /// Whether the selected session is live: Enter opens it following.
    pub(crate) fn selected_live(&self) -> bool {
        self.sampler.live(self.sel)
    }

    /// The stats are taken on the screen's tick while any is still due.
    pub(crate) fn tick_every(&self) -> Option<Duration> {
        self.sampler.watching().then_some(SAMPLE_TICK)
    }

    pub(crate) fn tick(&mut self) {
        self.sampler.sample();
    }
}

/// The pane's title: the count, the scope, and how many are live. A scope
/// too long for `width` is cut at its end, so the counts stay in view.
fn title(p: &Picker, width: usize) -> String {
    let live = p.sampler.live_count();
    let head = format!(" {} sessions in ", p.sessions.len());
    let tail = if live > 0 { format!(" · {live} live ") } else { " ".to_string() };
    let room = width.saturating_sub(head.chars().count() + tail.chars().count());
    let scope: String = if p.scope.chars().count() <= room {
        p.scope.clone()
    } else {
        let mut s: String = p.scope.chars().take(room.saturating_sub(1)).collect();
        s.push('…');
        s
    };
    format!("{head}{scope}{tail}")
}

pub(crate) fn draw_picker(f: &mut Draw, p: &mut Picker, tab_line: Line<'static>, shape: Shape, msg: &str) {
    let [bar, main, status] = Layout::vertical([Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)]).areas(f.area());
    f.render_widget(Paragraph::new(tab_line), bar);
    // The counts first: a long scope is cut at its end, never a count.
    let title = title(p, main.width.saturating_sub(2) as usize);
    if p.sessions.is_empty() {
        f.render_widget(Paragraph::new(Line::styled("no sessions recorded", theme::muted())).block(pane(title)), main);
    } else {
        let right = |t: String| Cell::from(Line::from(t).alignment(Alignment::Right));
        let header = Row::new(vec![
            Cell::from(""),
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
            .enumerate()
            .map(|(i, s)| {
                let project = s.project.rsplit('/').next().unwrap_or(&s.project).to_string();
                let date = s.ts.replace('T', " ");
                let own = p.own.as_deref() == Some(s.id.as_str());
                let transcript = match (own, s.transcript) {
                    (true, _) => Span::styled("this session", theme::accent().add_modifier(Modifier::BOLD)),
                    (false, true) => Span::styled("yes", theme::ok()),
                    (false, false) => Span::styled("gone", theme::muted()),
                };
                let mark = if p.sampler.live(i) { Span::styled(LIVE_MARK, theme::ok().add_modifier(Modifier::BOLD)) } else { Span::raw("") };
                Row::new(vec![
                    Cell::from(mark),
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
            Constraint::Length(1),
            Constraint::Length(12),
            Constraint::Length(16),
            Constraint::Min(8),
            Constraint::Length(6),
            Constraint::Length(5),
            Constraint::Length(8),
            Constraint::Length(12),
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
        let mut spans = Vec::new();
        // When the selected session's transcript was last written, live or not.
        if let Some(at) = p.sampler.last_write(p.sel) {
            let ago = agent_fmt::when::ago(p.sampler.now().saturating_sub(at) / 1000);
            if p.sampler.live(p.sel) {
                spans.push(Span::styled(format!("{LIVE_MARK} live · written {ago} · "), theme::ok()));
            } else {
                spans.push(Span::styled(format!("written {ago} · "), theme::muted()));
            }
        }
        spans.push(Span::styled(format!("{}/{}", (p.sel + 1).min(p.sessions.len()), p.sessions.len()), theme::muted()));
        spans
    } else {
        vec![Span::styled(msg.to_string(), theme::err().add_modifier(Modifier::BOLD))]
    };
    let enter = if p.selected_live() { "follow" } else { "replay" };
    let keys = [("↑↓", "select"), ("⏎", enter), ("q", "quit")];
    f.render_widget(Paragraph::new(key_bar(shape, "pick", Ground::Accent, &keys, right, status.width)), status);
}
