//! The menus over the shell, each a [`Dialog`] with a menu body behind a
//! [`Modal`]: the action menu of a row, the theme tab's menu, and the
//! picker, which is a choice's options or a pane's menu ([`Open::Pick`]).
//! One [`MenuKeys`] sets what each one's keys do beyond moving and Enter.
//!
//! [`Open::Pick`]: super::pane::Open::Pick

use ratatui::text::Span;

use super::dialog::{Answer, Dialog, MenuKeys, Placement, Space};
use super::modals::{mode_lozenge, Modal};
use super::pane::Overlay;
use super::theme::{self, Ground};
use super::*;

/// The width of a menu placed in the middle.
const WIDTH: u16 = 50;

/// A row's action menu ([`Mode::Menu`]).
pub(super) struct ActionMenu;
/// The theme tab's menu ([`Mode::ThemeMenu`]).
pub(super) struct ThemeMenu;
/// The picker ([`Mode::Pick`]).
pub(super) struct PickMenu;

/// The bar under a menu whose Enter or click picks.
fn picks_status(app: &App, label: &str) -> Vec<Span<'static>> {
    let mut spans = mode_lozenge(app, label, Ground::Accent);
    spans.push(Span::styled("  ↑↓ choose · Enter or click picks · Esc closes", theme::hint()));
    spans
}

impl Modal for ActionMenu {
    /// The node's actions, each with its key and what it asks for; its
    /// letter picks it. `q` and `a` close it.
    fn dialog(&self, app: &App, _: bool) -> Dialog<'static> {
        let Mode::Menu { path, sel } = &app.mode else { unreachable!("an action menu is open") };
        let n = tree::get(&app.roots, path);
        let letters = tree::action_keys(&n.actions);
        let items = n
            .actions
            .iter()
            .zip(&letters)
            .map(|(a, k)| {
                let tag = match (&a.arg, a.confirm) {
                    (Arg::Secret, _) => "  masked".to_string(),
                    (Arg::Flow(_), _) => "  guided".to_string(),
                    (Arg::View(_), _) => "  view".to_string(),
                    (Arg::Text(p), true) => format!("  {p}, asks first"),
                    (Arg::Text(p), false) => format!("  {p}"),
                    (Arg::None, true) => "  asks first".to_string(),
                    (Arg::None, false) => String::new(),
                };
                (format!("{} {}", k.unwrap_or(' '), a.label), tag)
            })
            .collect();
        let keys = MenuKeys { letters, ..MenuKeys::default() };
        Dialog::new(format!("actions: {}", tree::label(&app.roots, path)), WIDTH).menu(items, *sel, keys).close_keys(&['q', 'a']).close_outside().build().expect("a menu is answered by its keys")
    }

    fn set_sel(&self, app: &mut App, to: usize) {
        if let Mode::Menu { sel, .. } = &mut app.mode {
            *sel = to;
        }
    }

    fn answer(&self, app: &mut App, a: Answer) -> bool {
        match a {
            Answer::Item(i) => {
                let Mode::Menu { path, .. } = std::mem::replace(&mut app.mode, Mode::Browse) else { return true };
                app.pick(path, i);
            }
            Answer::Close => app.mode = Mode::Browse,
            _ => {}
        }
        true
    }

    /// The wheel anywhere moves its cursor: the tree has nothing under it
    /// to scroll.
    fn wheel_outside(&self, app: &mut App, m: MouseEvent) {
        let up = m.kind == MouseEventKind::ScrollUp;
        app.modal_key(KeyEvent::new(if up { KeyCode::Up } else { KeyCode::Down }, KeyModifiers::NONE));
    }

    fn status(&self, app: &App) -> Option<Vec<Span<'static>>> {
        Some(picks_status(app, "action"))
    }
}

impl Modal for ThemeMenu {
    /// What the theme under the cursor can do. `q` and `a` close it.
    fn dialog(&self, app: &App, _: bool) -> Dialog<'static> {
        let Mode::ThemeMenu { sel } = app.mode else { unreachable!("the theme menu is open") };
        let items = app.theme_acts().iter().map(|a| (a.label().to_string(), app.theme_act_tag(*a))).collect();
        let title = format!("actions: theme {}", app.themes.under_cursor().0.name);
        Dialog::new(title, WIDTH).menu(items, sel, MenuKeys::default()).close_keys(&['q', 'a']).close_outside().build().expect("a menu is answered by its keys")
    }

    fn set_sel(&self, app: &mut App, to: usize) {
        if let Mode::ThemeMenu { sel } = &mut app.mode {
            *sel = to;
        }
    }

    fn answer(&self, app: &mut App, a: Answer) -> bool {
        match a {
            Answer::Item(i) => {
                app.mode = Mode::Browse;
                let acts = app.theme_acts();
                app.theme_act(acts[i.min(acts.len() - 1)]);
            }
            Answer::Close => app.mode = Mode::Browse,
            _ => {}
        }
        true
    }

    fn wheel_outside(&self, app: &mut App, m: MouseEvent) {
        ActionMenu.wheel_outside(app, m);
    }

    fn status(&self, app: &App) -> Option<Vec<Span<'static>>> {
        Some(picks_status(app, "action"))
    }
}

impl Modal for PickMenu {
    /// The options, the current marked: on a list Space and a click mark a
    /// row and Enter sets the marks; on one choice Space, Enter and a click
    /// pick. A row that cannot be picked yet takes none of them. A pane's
    /// menu docks where it opened.
    fn dialog(&self, app: &App, _: bool) -> Dialog<'static> {
        let Mode::Pick(p) = &app.mode else { unreachable!("the picker is open") };
        let what = match &p.pane {
            Some((_, title)) => title.clone(),
            None => tree::label(&app.roots, &p.path),
        };
        let title = format!("{}: {what}", if p.multi { "pick any" } else { "pick one" });
        let waits = (0..p.options.len()).map(|i| !p.open(i)).collect();
        let keys = MenuKeys { letters: Vec::new(), space: if p.multi { Space::Marks } else { Space::Picks }, waits };
        Dialog::new(title, WIDTH).menu(p.items(), p.sel, keys).dock(p.dock).close_keys(&['q']).jumps().close_outside().build().expect("a menu is answered by its keys")
    }

    fn placement(&self, app: &App) -> Placement {
        match &app.mode {
            Mode::Pick(p) => p.dock.map_or(Placement::Center, Placement::Dock),
            _ => Placement::Center,
        }
    }

    fn set_sel(&self, app: &mut App, to: usize) {
        if let Mode::Pick(p) = &mut app.mode {
            p.sel = to;
        }
    }

    /// A pick sets the value, or goes back to the pane; a mark flips the
    /// row. Closed, a setting's picker says nothing changed, and a pane's
    /// menu says nothing, as every overlay over a pane does.
    fn answer(&self, app: &mut App, a: Answer) -> bool {
        match a {
            Answer::Item(i) => {
                // A menu the pick leads to (Invite agent ▸) docks beside the
                // row picked, as the frame last drew it.
                let parent = app.drawn_dialog().and_then(|d| d.rows.get(i).map(|row| (d.rect, *row)));
                let Mode::Pick(mut p) = std::mem::replace(&mut app.mode, Mode::Browse) else { return true };
                p.sel = i;
                app.pick_set(p, parent);
            }
            Answer::Mark(i) => {
                if let Mode::Pick(p) = &mut app.mode {
                    p.marked[i] = !p.marked[i];
                }
            }
            Answer::Close => {
                let Mode::Pick(p) = std::mem::replace(&mut app.mode, Mode::Browse) else { return true };
                if p.pane.is_some() {
                    app.msg.clear();
                } else {
                    app.msg = "unchanged".into();
                }
            }
            _ => {}
        }
        true
    }

    fn status(&self, app: &App) -> Option<Vec<Span<'static>>> {
        let Mode::Pick(p) = &app.mode else { return None };
        let mut spans = mode_lozenge(app, "pick", Ground::Accent);
        let hint = if p.multi { "  ↑↓ move · Space or click marks · Enter sets · Esc closes" } else { "  ↑↓ choose · Enter or click sets · Esc closes" };
        spans.extend([Span::styled(hint, theme::hint()), app.msg_span()]);
        Some(spans)
    }

    fn overlay(&self, app: &App) -> Overlay {
        let Mode::Pick(p) = &app.mode else { unreachable!("the picker is open") };
        Overlay::Menu { id: p.pane.as_ref().map(|(id, _)| id.clone()).unwrap_or_default(), by_esc: p.by_esc }
    }

    /// A pane's menu, by its id: a click outside it on what opens the same
    /// menu only closes it.
    fn identity(&self, app: &App) -> Option<String> {
        let Mode::Pick(p) = &app.mode else { return None };
        p.pane.as_ref().map(|(id, _)| id.clone())
    }
}
