//! A pane's entry dialog ([`crate::Open::Entry`]): one line typed in a small
//! box docked where a menu would dock, under the tab or row it belongs to.
//! The pane's own text entry is never borrowed for it, so a chat's compose
//! box keeps its draft whatever is typed here.
//!
//! Enter gives the text to the pane, which may open the dialog again with
//! the text and what was wrong with it; Esc or a click outside closes it
//! with nothing, as every overlay closes. Typing, Backspace, Delete, the
//! arrows left and right, Home and End edit the line; a paste goes in on
//! one line.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;

use super::pick::{docked, Dock};
use super::render::pane as bordered;
use super::*;
use crate::input::Input;

/// The open dialog.
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) input: Input,
    pub(crate) error: Option<String>,
    pub(crate) dock: Option<Dock>,
}

/// The dialog's text width: wide enough for a name and a short line.
const WIDTH: u16 = 44;

impl App {
    /// A key while the entry dialog is open. False ends the session.
    pub(super) fn entry_key(&mut self, mut e: Entry, k: KeyEvent) -> bool {
        let m = k.modifiers;
        match k.code {
            KeyCode::Esc => {}
            KeyCode::Enter if !m.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) => {
                let text = e.input.text().to_string();
                self.pane_picked(&e.id, vec![text]);
            }
            // One line: a newline is never typed here.
            KeyCode::Enter | KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => self.mode = Mode::Entry(Box::new(e)),
            _ => {
                e.input.key(k);
                e.error = None;
                self.mode = Mode::Entry(Box::new(e));
            }
        }
        true
    }

    /// A paste into the open dialog, on one line.
    pub(super) fn entry_paste(&mut self, text: &str) {
        if let Mode::Entry(e) = &mut self.mode {
            let one: String = text.split(['\n', '\r']).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ");
            e.input.insert_str(&one.chars().filter(|c| !c.is_control()).collect::<String>());
            e.error = None;
        }
    }

    /// The dialog over `area`, docked where it was opened from, else in the
    /// middle: its title, the line with its cursor, then what was wrong
    /// with the last text entered or how to answer.
    pub(super) fn draw_entry(&mut self, f: &mut Frame, area: Rect, e: &Entry) {
        let hint = match &e.error {
            Some(err) => Line::styled(err.clone(), theme::err()),
            None => Line::styled("Enter sets · Esc cancels", theme::hint()),
        };
        let fake: Vec<(String, String)> = vec![("x".repeat(WIDTH as usize), String::new()); 2];
        let r = match e.dock {
            Some(dock) => docked(area, dock, &e.title, &fake),
            None => super::render::modal_rect(area, WIDTH + 4, 4),
        };
        let inner = r.inner(ratatui::layout::Margin::new(1, 1));
        let width = inner.width.saturating_sub(1).max(1);
        let rows = e.input.rows(width, theme::body());
        let row = rows.iter().position(|l| l.spans.iter().any(|s| s.style.add_modifier.contains(ratatui::style::Modifier::REVERSED))).unwrap_or(0);
        let mut line = rows.get(row).cloned().unwrap_or_default();
        line.spans.insert(0, Span::raw(" "));
        self.hits.menu = Some((r, Vec::new()));
        f.render_widget(Clear, r);
        let mut hint = hint;
        hint.spans.insert(0, Span::raw(" "));
        f.render_widget(Paragraph::new(vec![line, hint]).block(bordered(e.title.clone()).border_style(theme::modal_border())), r);
    }
}
