//! The picker: a popup list of a choice's options, the current value
//! marked, for a fixed choice and a computed one alike. A list choice marks
//! several. What it picks goes through the same check as typed text, and
//! lands on the setting it was opened on.

use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::Frame;

use super::{App, Mode};
use crate::tree::{self, Kind};

/// An open picker over the setting at `path`.
#[derive(Debug, Clone)]
pub(crate) struct Pick {
    pub(crate) path: Vec<usize>,
    /// The choices, then on a list the stored items no choice names, which
    /// stay until unmarked rather than vanish unseen.
    pub(crate) options: Vec<String>,
    /// How many of `options` are choices; the rest are unknown items.
    pub(crate) known: usize,
    pub(crate) multi: bool,
    /// The option under the cursor.
    pub(crate) sel: usize,
    /// Per option: the current value (one), or marked to be in the list (multi).
    pub(crate) marked: Vec<bool>,
    /// The value it was opened on.
    pub(crate) was: String,
}

impl Pick {
    /// A picker over `options` with `value` marked, the cursor on the
    /// first marked option.
    pub(crate) fn new(path: Vec<usize>, mut options: Vec<String>, multi: bool, value: &str) -> Pick {
        let known = options.len();
        let held = if multi { tree::list_items(value) } else { vec![value.to_string()] };
        if multi {
            options.extend(held.iter().filter(|h| !options[..known].contains(h)).cloned().collect::<Vec<_>>());
        }
        let marked: Vec<bool> = options.iter().map(|o| held.contains(o)).collect();
        let sel = marked.iter().position(|m| *m).unwrap_or(0);
        Pick { path, options, known, multi, sel, marked, was: value.to_string() }
    }

    /// The value the picker sets: the option under the cursor, or the
    /// marked ones in the options' order, unknown items last.
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
            .enumerate()
            .map(|(i, (o, m))| {
                let tag = if i >= self.known { "  not a choice" } else if !self.multi && *m { "  current" } else { "" };
                let mark = match (self.multi, m) {
                    (true, true) => "[x]",
                    (true, false) => "[ ]",
                    (false, true) => "●",
                    (false, false) => " ",
                };
                (format!("{mark} {o}"), tag.to_string())
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
            KeyCode::Up | KeyCode::Char('k') => p.sel = p.sel.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => p.sel = (p.sel + 1).min(last),
            KeyCode::Home | KeyCode::Char('g') => p.sel = 0,
            KeyCode::End | KeyCode::Char('G') => p.sel = last,
            KeyCode::Char(' ') if p.multi => p.marked[p.sel] = !p.marked[p.sel],
            KeyCode::Enter | KeyCode::Char(' ') => return self.pick_set(p),
            _ => {}
        }
        if !matches!(k.code, KeyCode::Esc | KeyCode::Char('q')) {
            self.mode = Mode::Pick(p);
        }
    }

    /// Set the picked value on the setting the picker was opened on. A
    /// rejected value keeps the picker open, and the bottom bar says why.
    fn pick_set(&mut self, p: Pick) {
        let v = p.value();
        if v == p.was {
            self.msg = "unchanged".into();
            return;
        }
        if !self.set_value(&p.path, &v) {
            self.mode = Mode::Pick(p);
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
    use crate::tree::{Node, Setting};
    use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

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
    fn many_marks_each_listed_value_and_keeps_an_unknown_one_until_unmarked() {
        let mut p = Pick::new(vec![0], opts(), true, "[mine, gone, anthropic]");
        assert_eq!(p.marked, vec![true, false, true, true]);
        assert_eq!(p.items()[3], ("[x] gone".to_string(), "  not a choice".to_string()));
        assert_eq!(p.value(), "[anthropic, mine, gone]", "an item no choice names is kept");
        p.marked[2] = false;
        p.marked[1] = true;
        p.marked[3] = false;
        assert_eq!(p.value(), "[anthropic, openrouter]");
        assert_eq!(p.items()[2].0, "[ ] mine");
    }

    /// Two choices in one tab: the picker opens on the second.
    fn app() -> App {
        let choice = |v: &str, multi| Setting::new(Kind::Choice { options: opts(), multi }, v, "user");
        let roots = vec![Node::group(
            "gate",
            "",
            vec![Node::leaf("first", "", choice("anthropic", false)), Node::leaf("second", "", choice("[mine]", true))],
        )
        .opened()];
        App::new("t", roots)
    }

    fn press(app: &mut App, k: KeyCode) {
        app.key(KeyEvent::new(k, KeyModifiers::NONE));
    }

    fn value(app: &App, i: usize) -> String {
        app.roots[0].children[i].setting.as_ref().unwrap().value.clone()
    }

    #[test]
    fn a_pick_lands_on_the_setting_it_was_opened_on_by_key_and_by_click() {
        let mut app = app();
        // The second row: open its picker, then move the tree's cursor under it.
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        let Mode::Pick(p) = &app.mode else { panic!("no picker") };
        let opened = p.path.clone();
        app.cursor = 0;
        // Draw, so the rows are click targets; click the first option.
        let _ = crate::testkit::render(&mut app, 100, 30);
        let (_, rows) = app.hits.menu.clone().expect("the picker records its rows");
        let at = rows[0];
        app.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: at.x + 2, row: at.y, modifiers: KeyModifiers::NONE });
        let Mode::Pick(p) = &app.mode else { panic!("a click on a list marks, it does not close") };
        assert_eq!((p.path.clone(), p.marked[0]), (opened, true));
        press(&mut app, KeyCode::Enter);
        assert_eq!(value(&app, 1), "[anthropic, mine]", "set on the row the picker was opened on");
        assert_eq!(value(&app, 0), "anthropic", "not on the row under the cursor");
        // One: a click on an option sets it at once.
        app.cursor = 0;
        press(&mut app, KeyCode::Enter);
        let _ = crate::testkit::render(&mut app, 100, 30);
        let (_, rows) = app.hits.menu.clone().unwrap();
        let at = rows[1];
        app.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: at.x + 2, row: at.y, modifiers: KeyModifiers::NONE });
        assert!(matches!(app.mode, Mode::Browse), "{}", app.msg);
        assert_eq!(value(&app, 0), "openrouter");
    }
}
