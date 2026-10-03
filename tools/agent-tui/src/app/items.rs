//! The name prompt and the named-item flow on a tab (#851). The prompt
//! takes a name for a copy or a rename of the tab's named items, or for a
//! theme tab action of its own; the y/n comes before a delete. The items
//! are whatever [`NamedItems`] the tab manages; the checks and the
//! refusals are [`crate::named`]'s, and what follows a done operation is
//! the tab's own. The theme tab is the one tab that manages named items
//! today; `render` draws the two modes.

use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::themestate::NameOp;
use super::{App, Mode};
use crate::named::{self, Done, ItemAct, ItemOp, NamedItems, Refusal};

/// What a typed name is for.
#[derive(Debug, Clone, PartialEq)]
pub enum NameFor {
    /// A copy or a rename of one of the tab's named items.
    Item(ItemOp),
    /// A new theme, or the copy a bundled theme's edit starts from.
    Theme(NameOp),
}

impl NameFor {
    pub fn prompt(&self) -> String {
        match self {
            NameFor::Item(op) => op.prompt(),
            NameFor::Theme(op) => op.prompt(),
        }
    }
}

impl App {
    /// The named items the shown tab manages, if it manages any.
    fn items(&mut self) -> Option<&mut dyn NamedItems> {
        if self.on_theme_tab() {
            return Some(&mut self.themes);
        }
        None
    }

    /// Start `act` on item `name`: the name prompt for a copy or a rename,
    /// the y/n for a delete. A bundled item's rename or delete is refused
    /// with the reason before anything is asked.
    pub(super) fn item_act(&mut self, act: ItemAct, name: String) {
        let Some(items) = self.items() else { return };
        if let Some(r) = named::refuse_bundled(items, &name, act) {
            self.msg = format!("rejected: {r}");
            return;
        }
        let noun = items.noun();
        self.mode = match act {
            ItemAct::Copy => Mode::Name { op: NameFor::Item(ItemOp::Copy(name)), buf: String::new() },
            ItemAct::Rename => Mode::Name { op: NameFor::Item(ItemOp::Rename(name)), buf: String::new() },
            ItemAct::Delete => Mode::ItemDelete { name, noun },
        };
    }

    /// A name was typed for a copy or a rename. A name the rules refuse or
    /// one that is taken keeps the prompt open with the reason.
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
                    self.mode = Mode::Name { op: NameFor::Item(op), buf };
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
    pub(super) fn prompt_key(&mut self, mode: Mode, k: KeyEvent) {
        match mode {
            Mode::Name { op, mut buf } => match k.code {
                KeyCode::Esc => self.msg = "cancelled".into(),
                KeyCode::Enter => match op {
                    NameFor::Item(op) => self.item_named(op, buf),
                    NameFor::Theme(op) => self.theme_named(op, buf),
                },
                KeyCode::Backspace => {
                    buf.pop();
                    self.mode = Mode::Name { op, buf };
                }
                KeyCode::Char(c) => {
                    buf.push(c);
                    self.mode = Mode::Name { op, buf };
                }
                _ => self.mode = Mode::Name { op, buf },
            },
            Mode::ItemDelete { name, noun } => match k.code {
                KeyCode::Char('y' | 'Y') => self.item_delete(&name),
                KeyCode::Char('n' | 'N') | KeyCode::Esc => self.msg = "kept".into(),
                _ => self.mode = Mode::ItemDelete { name, noun },
            },
            other => self.mode = other,
        }
    }
}
