//! The picker: a popup list of a choice's options, the current value
//! marked, for a fixed choice and a computed one alike. A list choice marks
//! several. What it picks goes through the same check as typed text, and
//! lands on the setting it was opened on.
//!
//! A pane's menu is the same picker over its [`Item`]s: an item that cannot
//! be picked yet is drawn set back with why, the highlight passes over it,
//! and a click on it does nothing. It docks at what opened it ([`Dock`]).

use ratatui::layout::Rect;

use super::pane::{Anchor, Item};
use super::{App, Mode};
use crate::tree::{self, Kind};

/// Where a pane's picker docks: on the line under a tab or a row, or beside
/// the row of the menu it was picked from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dock {
    Under(Rect),
    Beside { menu: Rect, row: Rect },
}

/// An open picker over the setting at `path`.
#[derive(Debug, Clone)]
pub(crate) struct Pick {
    pub(crate) path: Vec<usize>,
    /// The choices, then the stored value or items no choice names, shown
    /// rather than vanishing unseen.
    pub(crate) options: Vec<String>,
    /// How many of `options` are choices; the rest are unknown items.
    pub(crate) known: usize,
    pub(crate) multi: bool,
    /// The option under the cursor.
    pub(crate) sel: usize,
    /// Per option: the current value (one), or marked to be in the list (multi).
    pub(crate) marked: Vec<bool>,
    /// Per option: why it cannot be picked yet, or `None` when it can.
    pub(crate) waits: Vec<Option<String>>,
    /// Per option: what it is called, or empty when it has no name.
    pub(crate) notes: Vec<String>,
    /// The value it was opened on.
    pub(crate) was: String,
    /// A pane's picker: the id its choice goes back under, and its title.
    /// `None` for a setting's.
    pub(crate) pane: Option<(String, String)>,
    /// Where a pane's picker docks; `None` centres it.
    pub(crate) dock: Option<Dock>,
    /// The pane opened it in answer to Esc ([`crate::Overlay::Menu`]).
    pub(crate) by_esc: bool,
}

impl Pick {
    /// A picker over `options` with `value` marked, the cursor on the
    /// first marked option.
    pub(crate) fn new(path: Vec<usize>, mut options: Vec<String>, multi: bool, value: &str) -> Pick {
        let known = options.len();
        let held = if multi { tree::list_items(value) } else { vec![value.to_string()] };
        // What is set shows, a choice or not: on one, the stored value, which
        // Enter leaves as it is; on a list, each item, kept until unmarked.
        let unknown: Vec<String> = held.iter().filter(|h| !h.is_empty() && !options[..known].contains(h)).cloned().collect();
        options.extend(unknown);
        let marked: Vec<bool> = options.iter().map(|o| held.contains(o)).collect();
        let sel = marked.iter().position(|m| *m).unwrap_or(0);
        let waits = vec![None; options.len()];
        let notes = vec![String::new(); options.len()];
        Pick { path, options, known, multi, sel, marked, waits, notes, was: value.to_string(), pane: None, dock: None, by_esc: false }
    }

    /// The picker with each option's name from `name_of`, shown beside it.
    pub(crate) fn named(mut self, name_of: impl Fn(&str) -> Option<String>) -> Pick {
        self.notes = self.options.iter().map(|o| name_of(o).unwrap_or_default()).collect();
        self
    }

    /// A picker a pane opened over `items`, the current ones marked, the
    /// choice going back to the pane under `id`. The cursor starts on the
    /// first current item, even one that cannot be picked, so an Enter at
    /// once changes nothing; with none current, on the first that can be
    /// picked.
    pub(crate) fn for_pane(id: String, title: String, items: Vec<Item>, multi: bool) -> Pick {
        let marked: Vec<bool> = items.iter().map(|i| i.current).collect();
        let waits: Vec<Option<String>> = items.iter().map(|i| (!i.enabled).then(|| i.reason.clone().unwrap_or_default())).collect();
        let options: Vec<String> = items.into_iter().map(|i| i.label).collect();
        let open = |i: &usize| waits[*i].is_none();
        let sel = (0..options.len()).find(|i| marked[*i]).or_else(|| (0..options.len()).find(open)).unwrap_or(0);
        let known = options.len();
        let notes = vec![String::new(); options.len()];
        Pick { path: Vec::new(), options, known, multi, sel, marked, waits, notes, was: String::new(), pane: Some((id, title)), dock: None, by_esc: false }
    }

    /// Whether option `i` can be picked.
    pub(crate) fn open(&self, i: usize) -> bool {
        self.waits.get(i).is_some_and(Option::is_none)
    }

    /// The options picked: the one under the cursor, or the marked ones in
    /// order.
    pub(crate) fn values(&self) -> Vec<String> {
        if !self.multi {
            return vec![self.options[self.sel].clone()];
        }
        self.options.iter().zip(&self.marked).filter(|(_, m)| **m).map(|(o, _)| o.clone()).collect()
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

    /// The (label, tag) rows the popup draws. An option that cannot be
    /// picked yet carries why as its tag; one with a name carries the name,
    /// then whether it is current.
    pub(crate) fn items(&self) -> Vec<(String, String)> {
        use crate::wrap::str_width;
        let mark = |m: bool| match (self.multi, m) {
            (true, true) => "[x]",
            (true, false) => "[ ]",
            (false, true) => "●",
            (false, false) => " ",
        };
        let labels: Vec<String> = self.options.iter().zip(&self.marked).map(|(o, m)| format!("{} {o}", mark(*m))).collect();
        // Where an option says why it waits, or what it is called, the
        // labels pad to the widest, in screen cells, so each starts in one
        // column.
        let aligned = self.waits.iter().any(Option::is_some) || self.notes.iter().any(|n| !n.is_empty());
        let pad = if aligned { labels.iter().map(|l| str_width(l)).max().unwrap_or(0) } else { 0 };
        labels
            .into_iter()
            .zip(&self.marked)
            .enumerate()
            .map(|(i, (label, m))| {
                let state = match (i >= self.known, !self.multi && *m) {
                    _ if !self.open(i) => self.waits[i].as_deref().unwrap_or_default(),
                    (true, true) => "current, not a choice",
                    (true, false) => "not a choice",
                    (false, true) => "current",
                    (false, false) => "",
                };
                let note = self.notes.get(i).map(String::as_str).unwrap_or_default();
                let tag = match (note, state) {
                    ("", "") => String::new(),
                    ("", s) | (s, "") => format!("  {s}"),
                    (n, s) => format!("  {n} · {s}"),
                };
                (crate::wrap::pad_to(&label, pad), tag)
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
        self.mode = Mode::Pick(Pick::new(path.to_vec(), options.clone(), *multi, &s.value).named(|o| s.label_of(o).map(str::to_string)));
        // The bottom bar keeps room for why a pick is refused.
        self.msg.clear();
    }

    /// Set the picked value on the setting the picker was opened on. A
    /// rejected value keeps the picker open, and the bottom bar says why.
    /// A pane's pick goes back to the pane, and a menu it opens in answer
    /// docks beside `parent`, the menu and the row picked.
    pub(super) fn pick_set(&mut self, p: Pick, parent: Option<(Rect, Rect)>) {
        if let Some((id, _)) = &p.pane {
            self.menu_parent = parent;
            self.pane_picked(id, p.values());
            self.menu_parent = None;
            return;
        }
        let v = p.value();
        if v == p.was {
            self.msg = "unchanged".into();
            return;
        }
        if !self.set_value(&p.path, &v) {
            self.mode = Mode::Pick(p);
        }
    }

    /// Where a pane's menu docks, from the anchor it names: under the tab a
    /// click (or Enter on the focused bar) opened it from, whatever it
    /// names; else under its tab or row, or beside the row of the menu it
    /// was picked from; else in the middle.
    pub(super) fn dock(&self, anchor: Anchor) -> Option<Dock> {
        if let Some(tab) = self.clicked_tab {
            return Some(Dock::Under(tab));
        }
        match anchor {
            Anchor::Center => None,
            Anchor::Tab(i) => self.hits.tabs.iter().find(|(_, t)| *t == i).map(|(r, _)| Dock::Under(*r)),
            Anchor::Row(r) => Some(Dock::Under(r)),
            Anchor::Parent => self.menu_parent.map(|(menu, row)| Dock::Beside { menu, row }),
        }
    }
}

/// Where a docked picker goes in `area`, as wide as its title and items
/// need. Under a row: on the line below the row, from its left edge; where
/// the lines below cannot hold it, it ends on the line above the row, so
/// the row stays in sight; where neither side can, it sits at the foot of
/// `area`. Beside a menu's row: right of the menu, its first item level
/// with the row, or left of the menu where the right has no room. It is
/// moved as far as it must to lie inside `area`, which a row left outside
/// it by a resize needs, and it is never taller or wider than `area`.
pub(super) fn docked(area: Rect, dock: Dock, title: &str, items: &[(String, String)]) -> Rect {
    use crate::wrap::str_width;
    // Borders, the item's lead space, its mark and a space, and a column
    // of padding: the rows a menu's dialog draws.
    let widest = items.iter().map(|(l, t)| 1 + str_width(l).max(10) + str_width(t) + 1).max().unwrap_or(0);
    docked_box(area, dock, title, widest, items.len())
}

/// Where a docked box goes in `area`, as [`docked`] places a menu: `inside`
/// columns and `rows` rows within its borders, and wide enough for its
/// title.
pub(super) fn docked_box(area: Rect, dock: Dock, title: &str, inside: usize, rows: usize) -> Rect {
    use crate::wrap::str_width;
    let want = (inside.max(str_width(title) + 2) + 2).max(24);
    let w = (want.min(usize::from(u16::MAX)) as u16).min(area.width);
    let h = (rows.min(usize::from(u16::MAX - 2)) as u16 + 2).min(area.height);
    let row = match dock {
        Dock::Under(row) => row,
        Dock::Beside { menu, row } => {
            // Right of the menu; else left of it; else as far right as fits.
            let x = if menu.right() + w <= area.right() {
                menu.right()
            } else if menu.x >= area.x + w {
                menu.x - w
            } else {
                area.right() - w
            };
            let y = row.y.saturating_sub(1).clamp(area.y, area.bottom() - h);
            return Rect { x: x.clamp(area.x, area.right() - w), y, width: w, height: h };
        }
    };
    let below = row.y.saturating_add(row.height.max(1));
    let y = if below + h <= area.bottom() {
        below
    } else if row.y >= area.y + h {
        row.y - h
    } else {
        area.bottom() - h
    };
    let y = y.clamp(area.y, area.bottom() - h);
    let x = row.x.clamp(area.x, area.right() - w);
    Rect { x, y, width: w, height: h }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{Node, Setting};
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

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
        // Nothing set: nothing marked, the cursor at the top.
        let p = Pick::new(vec![0], opts(), false, "");
        assert_eq!((p.sel, p.options.len()), (0, 3));
        // A value no option names shows, marked current, under the cursor.
        let p = Pick::new(vec![0], opts(), false, "gone");
        assert_eq!((p.sel, p.known, p.options.len()), (3, 3, 4));
        assert_eq!(p.items()[3], ("● gone".to_string(), "  current, not a choice".to_string()));
        assert_eq!(p.value(), "gone", "Enter on it leaves it as it is");
    }

    #[test]
    fn a_named_choice_shows_its_name_beside_it_and_the_names_line_up() {
        let names = |o: &str| (o != "mine").then(|| format!("{o} engine"));
        let p = Pick::new(vec![0], opts(), false, "openrouter").named(names);
        assert_eq!(p.items()[0], ("  anthropic ".to_string(), "  anthropic engine".to_string()));
        assert_eq!(p.items()[1], ("● openrouter".to_string(), "  openrouter engine · current".to_string()));
        assert_eq!(p.items()[2], ("  mine      ".to_string(), String::new()), "no name, no tag, padded all the same");
        assert_eq!(p.value(), "openrouter", "the value is the choice, never its name");
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
    fn a_list_with_an_unknown_item_still_marked_is_refused_until_it_is_unmarked() {
        let mut app = app();
        tree::get_mut(&mut app.roots, &[0, 1]).setting.as_mut().unwrap().value = "[mine, gone]".into();
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        // Mark openrouter, keep `gone` marked, and set.
        press(&mut app, KeyCode::Home);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Enter);
        let Mode::Pick(p) = &app.mode else { panic!("a refused pick keeps the picker open: {}", app.msg) };
        let gone = p.options.iter().position(|o| o == "gone").unwrap();
        assert!(app.msg.starts_with("rejected: gone is not one of"), "{}", app.msg);
        assert_eq!(value(&app, 1), "[mine, gone]", "nothing set");
        let text = crate::testkit::rows(&crate::testkit::render(&mut app, 100, 30)).join("\n");
        assert!(text.contains("rejected: gone is not one of"), "the bottom bar says why:\n{text}");
        // Unmark it: the list sets.
        press(&mut app, KeyCode::End);
        assert!(matches!(&app.mode, Mode::Pick(p) if p.sel == gone));
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.mode, Mode::Browse), "{}", app.msg);
        assert_eq!(value(&app, 1), "[openrouter, mine]");
    }

    #[test]
    fn a_single_value_no_option_names_shows_and_enter_keeps_it() {
        let mut app = app();
        tree::get_mut(&mut app.roots, &[0, 0]).setting.as_mut().unwrap().value = "gone".into();
        press(&mut app, KeyCode::Enter);
        let text = crate::testkit::rows(&crate::testkit::render(&mut app, 100, 30)).join("\n");
        assert!(text.contains("● gone") && text.contains("current, not a choice"), "{text}");
        press(&mut app, KeyCode::Enter);
        assert_eq!((value(&app, 0), app.msg.as_str()), ("gone".to_string(), "unchanged"));
        // Re-picked: a choice replaces it.
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Home);
        press(&mut app, KeyCode::Enter);
        assert_eq!(value(&app, 0), "anthropic");
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
        let rows = app.drawn_dialog().expect("the picker records its rows").rows.clone();
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
        let rows = app.drawn_dialog().expect("the picker records its rows").rows.clone();
        let at = rows[1];
        app.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: at.x + 2, row: at.y, modifiers: KeyModifiers::NONE });
        assert!(matches!(app.mode, Mode::Browse), "{}", app.msg);
        assert_eq!(value(&app, 0), "openrouter");
    }

    /// The reasons start in one screen column whatever the labels hold:
    /// a wide character takes two cells, an emoji two.
    #[test]
    fn reasons_line_up_by_display_width() {
        use crate::wrap::str_width;
        let items = vec![Item::new("Join"), Item::disabled("数据 channel", "needs D1"), Item::disabled("🚀 launch", "needs D4"), Item::new("Delete channel")];
        let p = Pick::for_pane("m".into(), "t".into(), items, false);
        let widths: Vec<usize> = p.items().iter().map(|(label, _)| str_width(label)).collect();
        assert!(widths.windows(2).all(|w| w[0] == w[1]), "every label pads to one width: {widths:?} {:#?}", p.items());
    }

    #[test]
    fn a_docked_menu_sits_under_its_row_else_above_it_else_at_the_foot() {
        let area = Rect { x: 0, y: 1, width: 80, height: 23 };
        let items: Vec<(String, String)> = (0..7).map(|i| (format!("  item {i}"), String::new())).collect();
        let row = |y| Rect { x: 0, y, width: 26, height: 1 };
        let r = docked(area, Dock::Under(row(8)), "pick one: #infra", &items);
        assert_eq!((r.x, r.y, r.height), (0, 9, 9), "on the line under the row");
        let r = docked(area, Dock::Under(row(20)), "pick one: #infra", &items);
        assert_eq!((r.y, r.bottom()), (11, 20), "no room below: it ends on the line above the row");
        let short = Rect { height: 10, ..area };
        let r = docked(short, Dock::Under(row(6)), "pick one: #infra", &items);
        assert_eq!((r.y, r.bottom()), (2, 11), "no room either side: at the foot, inside the area");
        let r = docked(area, Dock::Under(Rect { x: 70, y: 0, width: 8, height: 1 }), "pick one: #infra", &items);
        assert_eq!(r.right(), 80, "moved left to end inside the area");
        assert!(r.width >= 24 && r.width <= 80);
        let r = docked(Rect { height: 23, ..area }, Dock::Under(row(31)), "pick one: #infra", &items);
        assert_eq!((r.y, r.bottom()), (15, 24), "a row below the area after a resize: the menu ends at its foot");
        // Beside a menu's row: right of it, level with the row; else left of
        // it; else as far right as it fits.
        let parent = Rect { x: 0, y: 1, width: 24, height: 8 };
        let r = docked(area, Dock::Beside { menu: parent, row: Rect { x: 1, y: 3, width: 22, height: 1 } }, "pick one: theme", &items);
        assert_eq!((r.x, r.y), (24, 2), "right of the menu, its first item on the row's line");
        let right = Rect { x: 56, ..parent };
        assert_eq!(docked(area, Dock::Beside { menu: right, row: Rect { y: 3, ..right } }, "t", &items).right(), 56, "left of a menu at the right edge");
        let wide: Vec<(String, String)> = vec![("  ".to_string() + &"x".repeat(60), String::new())];
        let r = docked(area, Dock::Beside { menu: parent, row: Rect { y: 3, ..parent } }, "t", &wide);
        assert_eq!(r.right(), 80, "neither side holds it: as far right as fits");
        let narrow = Rect { width: 20, ..area };
        assert_eq!(docked(narrow, Dock::Under(row(3)), "a long title for a narrow screen", &items).width, 20, "never wider than the area");
    }
}
