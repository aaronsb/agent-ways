//! The picker: a popup list of a choice's options, the current value
//! marked, for a fixed choice and a computed one alike. A list choice marks
//! several. What it picks goes through the same check as typed text.

use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::Frame;

use super::{App, Mode};
use crate::tree::{self, Kind};

/// An open picker over the setting at `path`.
#[derive(Debug, Clone)]
pub(crate) struct Pick {
    pub(crate) path: Vec<usize>,
    pub(crate) options: Vec<String>,
    pub(crate) multi: bool,
    /// The option under the cursor.
    pub(crate) sel: usize,
    /// Per option: the current value (one), or marked to be in the list (multi).
    pub(crate) marked: Vec<bool>,
}

impl Pick {
    /// A picker over `options` with `value` marked, the cursor on the
    /// first marked option.
    pub(crate) fn new(path: Vec<usize>, options: Vec<String>, multi: bool, value: &str) -> Pick {
        let held = if multi { tree::list_items(value) } else { vec![value.to_string()] };
        let marked: Vec<bool> = options.iter().map(|o| held.contains(o)).collect();
        let sel = marked.iter().position(|m| *m).unwrap_or(0);
        Pick { path, options, multi, sel, marked }
    }

    /// The value the picker sets: the option under the cursor, or the
    /// marked ones in the options' order.
    pub(crate) fn value(&self) -> String {
        if !self.multi {
            return self.options[self.sel].clone();
        }
        let items: Vec<String> = self.options.iter().zip(&self.marked).filter(|(_, m)| **m).map(|(o, _)| o.clone()).collect();
        tree::list_value(&items)
    }

    /// The (label, tag) rows the popup draws.
    fn items(&self) -> Vec<(String, String)> {
        self.options
            .iter()
            .zip(&self.marked)
            .map(|(o, m)| match (self.multi, m) {
                (true, true) => (format!("[x] {o}"), String::new()),
                (true, false) => (format!("[ ] {o}"), String::new()),
                (false, true) => (format!("● {o}"), "  current".to_string()),
                (false, false) => (format!("  {o}"), String::new()),
            })
            .collect()
    }
}

impl App {
    /// Open the picker on the choice at `path`.
    pub(super) fn open_pick(&mut self, path: &[usize]) {
        let n = tree::get(&self.roots, path);
        let Some(s) = &n.setting else { return };
        let Kind::Choice { options, multi } = &s.kind else { return };
        if options.is_empty() {
            self.msg = "no choices to pick from; e edits it as text".into();
            return;
        }
        self.mode = Mode::Pick(Pick::new(path.to_vec(), options.clone(), *multi, &s.value));
    }

    /// One key in the picker. Enter sets the value; on a list, Space marks
    /// the option under the cursor. Esc closes it unchanged.
    pub(super) fn pick_key(&mut self, mut p: Pick, k: KeyEvent) {
        let last = p.options.len() - 1;
        match k.code {
            KeyCode::Esc | KeyCode::Char('q') => self.msg = "unchanged".into(),
            KeyCode::Up | KeyCode::Char('k') => {
                p.sel = p.sel.saturating_sub(1);
                self.mode = Mode::Pick(p);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                p.sel = (p.sel + 1).min(last);
                self.mode = Mode::Pick(p);
            }
            KeyCode::Home | KeyCode::Char('g') => {
                p.sel = 0;
                self.mode = Mode::Pick(p);
            }
            KeyCode::End | KeyCode::Char('G') => {
                p.sel = last;
                self.mode = Mode::Pick(p);
            }
            KeyCode::Char(' ') if p.multi => {
                p.marked[p.sel] = !p.marked[p.sel];
                self.mode = Mode::Pick(p);
            }
            KeyCode::Enter | KeyCode::Char(' ') => self.commit(&p.value()),
            _ => self.mode = Mode::Pick(p),
        }
    }

    /// The popup, its rows recorded as click targets as the action menu's are.
    pub(super) fn draw_pick(&mut self, f: &mut Frame, area: Rect, p: &Pick) {
        let title = format!("{}: {}", if p.multi { "pick any" } else { "pick one" }, tree::label(&self.roots, &p.path));
        self.draw_menu_items(f, area, title, p.items(), p.sel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> Vec<String> {
        vec!["anthropic".into(), "openrouter".into(), "mine".into()]
    }

    #[test]
    fn one_marks_the_current_value_and_sets_the_option_under_the_cursor() {
        let mut p = Pick::new(vec![0], opts(), false, "openrouter");
        assert_eq!((p.sel, p.marked.clone()), (1, vec![false, true, false]));
        assert_eq!(p.items()[1], ("● openrouter".to_string(), "  current".to_string()));
        p.sel = 2;
        assert_eq!(p.value(), "mine");
        // A value no option names: nothing marked, the cursor at the top.
        assert_eq!(Pick::new(vec![0], opts(), false, "").sel, 0);
    }

    #[test]
    fn many_marks_each_listed_value_and_sets_them_in_option_order() {
        let mut p = Pick::new(vec![0], opts(), true, "[mine, anthropic]");
        assert_eq!(p.marked, vec![true, false, true]);
        p.marked[2] = false;
        p.marked[1] = true;
        assert_eq!(p.value(), "[anthropic, openrouter]");
        assert_eq!(p.items()[2].0, "[ ] mine");
    }
}
