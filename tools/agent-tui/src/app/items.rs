//! The named-item flow on a tab (#851): the name prompt for a copy or a
//! rename and the y/n before a delete, over whatever [`NamedItems`] the
//! tab manages. The checks and the refusals are [`crate::named`]'s; what
//! follows a done operation is the tab's own. The theme tab is the one
//! tab that manages named items today; `render` draws the two modes.

use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::{App, Mode};
use crate::named::{self, Done, ItemAct, ItemOp, NamedItems, Refusal};

impl App {
    /// The named items the shown tab manages, if it manages any.
    fn items(&mut self) -> Option<&mut dyn NamedItems> {
        if self.on_theme_tab() {
            return Some(&mut self.themes);
        }
        None
    }

    /// What one of the shown tab's items is called.
    pub(super) fn item_noun(&self) -> &'static str {
        self.themes.noun()
    }

    /// Start `act` on item `name`: the name prompt for a copy or a rename,
    /// the y/n for a delete. A bundled item's rename or delete is refused
    /// with the reason before anything is asked.
    pub(super) fn item_act(&mut self, act: ItemAct, name: String) {
        let Some(items) = self.items() else { return };
        if act != ItemAct::Copy && items.bundled(&name) {
            let refused = match act {
                ItemAct::Delete => named::delete(items, &name),
                _ => named::rename(items, &name, ""),
            };
            if let Err(e) = refused {
                self.msg = format!("rejected: {e}");
            }
            return;
        }
        self.mode = match act {
            ItemAct::Copy => Mode::ItemName { op: ItemOp::Copy(name), buf: String::new() },
            ItemAct::Rename => Mode::ItemName { op: ItemOp::Rename(name), buf: String::new() },
            ItemAct::Delete => Mode::ItemDelete { name },
        };
    }

    /// A name was typed: copy or rename. A name the rules refuse or one
    /// that is taken keeps the prompt open with the reason.
    fn item_named(&mut self, op: ItemOp, buf: String) {
        let name = buf.trim().to_string();
        let Some(items) = self.items() else { return };
        let r = match &op {
            ItemOp::Copy(from) => named::copy(items, from, &name),
            ItemOp::Rename(from) => named::rename(items, from, &name),
        };
        match r {
            Ok(done) => self.item_done(done),
            Err(e) => {
                self.msg = format!("rejected: {e}");
                if matches!(e.why, Refusal::Name | Refusal::Taken) {
                    self.mode = Mode::ItemName { op, buf };
                }
            }
        }
    }

    fn item_delete(&mut self, name: &str) {
        let Some(items) = self.items() else { return };
        match named::delete(items, name) {
            Ok(done) => self.item_done(done),
            Err(e) => self.msg = format!("rejected: {e}"),
        }
    }

    /// What follows a done operation is the tab's.
    fn item_done(&mut self, done: Done) {
        if self.on_theme_tab() {
            self.theme_done(done);
        }
    }

    /// The keys of the name prompt and the delete y/n, with the mode taken
    /// out of the app: each arm puts back the mode that stays.
    pub(super) fn item_key(&mut self, mode: Mode, k: KeyEvent) {
        match mode {
            Mode::ItemName { op, mut buf } => match k.code {
                KeyCode::Esc => self.msg = "cancelled".into(),
                KeyCode::Enter => self.item_named(op, buf),
                KeyCode::Backspace => {
                    buf.pop();
                    self.mode = Mode::ItemName { op, buf };
                }
                KeyCode::Char(c) => {
                    buf.push(c);
                    self.mode = Mode::ItemName { op, buf };
                }
                _ => self.mode = Mode::ItemName { op, buf },
            },
            Mode::ItemDelete { name } => match k.code {
                KeyCode::Char('y' | 'Y') => self.item_delete(&name),
                KeyCode::Char('n' | 'N') | KeyCode::Esc => self.msg = "kept".into(),
                _ => self.mode = Mode::ItemDelete { name },
            },
            other => self.mode = other,
        }
    }
}
