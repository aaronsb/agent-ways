//! The modals over the shell, each a [`Dialog`] behind a [`Modal`]: the
//! boxed ones here, the exit guard, "close the editor", the key help and
//! the response modal, and the questions on the bottom bar in `questions`.
//! Whichever is open draws through here, and its keys and clicks come back
//! as an [`Answer`] that its [`Modal::answer`] acts on. The response
//! modal's text is wrapped in `response`.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::Frame;

use super::dialog::{Answer, Button, Dialog, Drawn, Placement};
use super::pane::Overlay;
use super::theme::{self, Ground, Seg};
use super::*;

/// The keys the tree shell answers to, as its key help lists them.
const KEYS: &[&str] = &[
    "Tab S-Tab 1-9  switch tab; each keeps its cursor and open groups",
    "↑↓ / j k     move          PgUp PgDn g G   jump",
    "→ / l        open group    ← / h           close / parent",
    "Enter Space  toggle bool, pick a choice, edit value, open group;",
    "             on a secret, masked entry; on an action-only node, its menu",
    "e            edit as text (^U clears)      d   set to default",
    "u            revert        /               filter all tabs by key",
    "Enter        on a filter hit: jump to it in its tab (Space acts)",
    "a            menu of the row's actions, or the tab's   x   unqueue the last",
    "             each action's own key runs it; the footer and detail name them",
    "c            pending pane (changes and queued actions)",
    "w Ctrl-S     review this tab's pending items, read-only (or click the ●",
    "             bar): a applies the tab, X discards it, Tab next tab, Esc back",
    "X ↺          discard this tab's pending items (y/n); ↺ on the tab badge",
    "q Esc ^C     quit; with items pending in any tab, asks first",
    "mouse        click a tab or row; click the selected row, or ▸ ▾, to act;",
    "             wheel moves; click a menu item, an answer, or a button",
    "m            mouse on or off (off lets the terminal select text)",
    "",
    "yellow = changed · blue = differs from default · grey = read-only",
    "[a] = has actions · orange = queued · ! = a finding · y/n answers a confirm",
    "theme tab    ↑↓ preview · Enter use · a actions · e edit (^S saves)",
];

/// One modal: the dialog it shows and what its answers do. [`App::modal`]
/// is the one table from a mode to its modal, so a modal is added there
/// and in its own impl, and the key, mouse and draw paths reach it from
/// both.
pub(super) trait Modal: Sync {
    /// Its dialog. `full` is false where only its keys and buttons are
    /// needed, so a body only the draw shows is left out.
    fn dialog(&self, app: &App, full: bool) -> Dialog<'static>;

    /// Draw it over `area`.
    fn draw(&self, app: &mut App, f: &mut Frame, area: Rect) -> Drawn {
        self.dialog(app, true).draw(f, area, app.shape)
    }

    /// Hold its scroll at `to`, where its mode keeps one.
    fn set_scroll(&self, _: &mut App, _: usize) {}

    /// Act on what it was answered. False ends the session.
    fn answer(&self, app: &mut App, a: Answer) -> bool;

    /// The wheel outside it, which a press outside closes: what is under it
    /// scrolls.
    fn wheel_outside(&self, app: &mut App, m: MouseEvent) {
        app.pane_wheel(m);
    }

    /// The bottom bar under it, in place of the screen's own.
    fn status(&self, _: &App) -> Option<Vec<Span<'static>>> {
        None
    }

    /// What a pane is told is open over it.
    fn overlay(&self, _: &App) -> Overlay {
        Overlay::Report
    }
}

/// The exit guard ([`Mode::Guard`]).
struct Guard;
/// "close the editor" ([`Mode::ThemeUnsaved`]).
struct Unsaved;
/// The key help ([`Mode::Help`]).
struct Help;
/// The response modal ([`Mode::Response`]).
struct Response;

/// A mode lozenge on the bottom bar.
fn mode_lozenge(app: &App, label: &str, g: Ground) -> Vec<Span<'static>> {
    app.shape.lozenge(&[Seg::on(format!(" {label} "), g).bold()])
}

impl Modal for Guard {
    fn dialog(&self, app: &App, _: bool) -> Dialog<'static> {
        app.guard_dialog()
    }

    fn answer(&self, app: &mut App, a: Answer) -> bool {
        match a {
            Answer::Button(Btn::Quit) => {
                app.mode = Mode::Browse;
                app.discard_all();
                return false;
            }
            Answer::Button(Btn::Review) => {
                app.mode = Mode::Browse;
                app.clear_filter();
                match (0..app.roots.len()).find(|t| app.pending_in(*t) > 0) {
                    Some(first) => {
                        app.switch_tab(first);
                        app.open_review(first);
                    }
                    // Only theme edits are unsaved: their review is the editor.
                    None if app.theme_dirty() => app.switch_tab(app.theme_tab()),
                    // A pane's work has no review: the modal stays.
                    None => app.mode = Mode::Guard,
                }
            }
            Answer::Button(Btn::Back) => app.mode = Mode::Browse,
            _ => {}
        }
        true
    }

    // The modal's buttons answer it; the bar only names their keys.
    fn status(&self, app: &App) -> Option<Vec<Span<'static>>> {
        let mut spans = mode_lozenge(app, "quit", Ground::Warn);
        // A pane's work alone has nothing to review.
        let review = app.pending() > 0 || app.theme_dirty();
        spans.push(Span::styled(if review { "  Esc back · r review · D quit and discard all" } else { "  Esc back · D quit and discard all" }, theme::hint()));
        Some(spans)
    }

    fn overlay(&self, _: &App) -> Overlay {
        Overlay::Guard
    }
}

impl Modal for Unsaved {
    /// Save, Discard or Back over the theme editor, Back focused.
    fn dialog(&self, app: &App, _: bool) -> Dialog<'static> {
        let name = app.themes.editor.as_ref().map_or(String::new(), |e| e.theme.name.clone());
        let lines = vec![Line::raw(""), Line::styled(format!(" {name} has unsaved edits"), theme::changed()), Line::raw("")];
        Dialog::new("close the editor", 56)
            .body(lines)
            // ^S saves here as it does in the editor.
            .button(Button::new(Btn::Save, "Save (s)", Ground::Ok).key('s').key('S').chord(KeyModifiers::CONTROL, 's'))
            .button(Button::new(Btn::Drop, "Discard (d)", Ground::Err).key('d').key('D'))
            .button(Button::new(Btn::Back, "Back (Esc)", Ground::Keep).key('b').cancel().default())
            .build()
            .expect("Back is the cancel and the default")
    }

    fn answer(&self, app: &mut App, a: Answer) -> bool {
        match a {
            Answer::Button(Btn::Save) => {
                app.mode = Mode::Browse;
                app.theme_save();
                if !app.theme_dirty() {
                    app.close_editor();
                }
            }
            Answer::Button(Btn::Drop) => {
                app.mode = Mode::Browse;
                app.close_editor();
                app.msg = "edits discarded".into();
            }
            Answer::Button(Btn::Back) => app.mode = Mode::Browse,
            _ => {}
        }
        true
    }

    fn status(&self, app: &App) -> Option<Vec<Span<'static>>> {
        let mut spans = mode_lozenge(app, "unsaved", Ground::Warn);
        spans.push(Span::styled("  s save · d discard · Esc back to the editor", theme::hint()));
        Some(spans)
    }
}

impl Modal for Help {
    /// The key help, scrolled. Any key it has no use for closes it, and so
    /// does a click, on it or outside it.
    fn dialog(&self, app: &App, full: bool) -> Dialog<'static> {
        let Mode::Help { scroll } = app.mode else { unreachable!("the key help is open") };
        let lines = if full { app.help_lines() } else { Vec::new() };
        let (title, least) = if app.pane.is_some() { ("keys · ↑↓ scroll · any other key closes", 56) } else { ("keys and help · ↑↓ scroll · any other key closes", 72) };
        let widest = lines.iter().map(Line::width).max().unwrap_or(0).min(usize::from(u16::MAX - 2)) as u16;
        Dialog::new(title, widest.max(least) + 2).body(lines).scroll(scroll).close_on_any_key().close_outside().keys_only().build().expect("the key help is answered by its keys")
    }

    fn set_scroll(&self, app: &mut App, to: usize) {
        if let Mode::Help { scroll } = &mut app.mode {
            *scroll = to;
        }
    }

    fn answer(&self, app: &mut App, a: Answer) -> bool {
        if a == Answer::Close {
            app.mode = Mode::Browse;
        }
        true
    }

    fn overlay(&self, _: &App) -> Overlay {
        Overlay::Help
    }
}

impl Modal for Response {
    fn dialog(&self, app: &App, _: bool) -> Dialog<'static> {
        let Mode::Response(s) = &app.mode else { unreachable!("the response is open") };
        s.dialog(&[], 0).build().expect("a response is answered by its keys")
    }

    fn draw(&self, app: &mut App, f: &mut Frame, area: Rect) -> Drawn {
        app.draw_response(f, area)
    }

    fn set_scroll(&self, app: &mut App, to: usize) {
        if let Mode::Response(s) = &mut app.mode {
            s.scroll = to;
        }
    }

    fn answer(&self, app: &mut App, a: Answer) -> bool {
        let Mode::Response(s) = &app.mode else { return true };
        if a == Answer::Close {
            app.mode = match s.back {
                Some(tab) => Mode::Review { tab, run: None, discard: false },
                None => Mode::Browse,
            };
        }
        true
    }

    /// Over the tree, which has nothing under a response to scroll, the
    /// wheel outside it scrolls the response.
    fn wheel_outside(&self, app: &mut App, m: MouseEvent) {
        if app.pane.is_some() {
            return app.pane_wheel(m);
        }
        let up = m.kind == MouseEventKind::ScrollUp;
        app.modal_key(KeyEvent::new(if up { KeyCode::Up } else { KeyCode::Down }, KeyModifiers::NONE));
    }

    fn status(&self, app: &App) -> Option<Vec<Span<'static>>> {
        let Mode::Response(s) = &app.mode else { return None };
        let mut spans = mode_lozenge(app, s.verdict.word(), s.verdict.ground());
        spans.extend([Span::styled("  ↑↓ PgUp PgDn scroll · Esc Enter q close", theme::hint()), app.msg_span()]);
        Some(spans)
    }
}

impl App {
    /// The open modal: the one table from a mode to its modal.
    pub(super) fn modal(&self) -> Option<&'static dyn Modal> {
        Some(match &self.mode {
            Mode::Guard => &Guard,
            Mode::ThemeUnsaved => &Unsaved,
            Mode::Help { .. } => &Help,
            Mode::Response(_) => &Response,
            Mode::Confirm { .. } => &questions::Confirm,
            Mode::DiscardTab { .. } => &questions::DiscardTab,
            Mode::Review { discard: true, .. } => &questions::ReviewDiscard,
            Mode::ItemDelete { .. } => &questions::ItemDelete,
            Mode::Ask(_) => &questions::Ask,
            _ => return None,
        })
    }

    /// Where the open modal was last drawn, if it was drawn since its mode
    /// opened: a drawing of another mode's dialog is not this one's.
    pub(super) fn drawn_dialog(&self) -> Option<&Drawn> {
        self.hits.dialog.as_ref().filter(|(of, _)| *of == std::mem::discriminant(&self.mode)).map(|(_, d)| d)
    }

    /// Draw the open modal over `area` when it is placed `on` it, recording
    /// where it landed and the scroll the frame holds it at. Whether it
    /// was drawn.
    pub(super) fn draw_modal(&mut self, f: &mut Frame, area: Rect, on: Placement) -> bool {
        let Some(m) = self.modal().filter(|m| m.dialog(self, false).placement() == on) else { return false };
        let drawn = m.draw(self, f, area);
        m.set_scroll(self, drawn.scroll);
        self.hits.dialog = Some((std::mem::discriminant(&self.mode), drawn));
        true
    }

    /// A key while a modal is open: it owns every key. False ends the session.
    pub(super) fn modal_key(&mut self, k: KeyEvent) -> bool {
        let Some(m) = self.modal() else { return true };
        let mut d = m.dialog(self, false);
        let a = d.key(k);
        m.set_scroll(self, d.scroll());
        self.modal_answer(m, a)
    }

    /// The mouse while a modal is open. The wheel over it scrolls it. The
    /// wheel outside one that a click outside closes goes to what is under
    /// it ([`Modal::wheel_outside`]); outside any other it does nothing. A
    /// press that `closes`, of the left button or beside a pane the right,
    /// goes to the dialog; one that closes it from outside then goes on, as
    /// [`App::click_through`] lets it. A dialog that any press closes takes
    /// a press of any button.
    pub(super) fn modal_mouse(&mut self, m: MouseEvent, closes: bool) {
        let Some(modal) = self.modal() else { return };
        let mut d = modal.dialog(self, false);
        let closes = closes || (d.any_press() && matches!(m.kind, MouseEventKind::Down(_)));
        // Not drawn since it opened: nothing of it is under the pointer.
        let drawn = self.drawn_dialog().cloned().unwrap_or_default();
        let at = Position::new(m.column, m.row);
        let inside = drawn.contains(at);
        let wheel = matches!(m.kind, MouseEventKind::ScrollUp | MouseEventKind::ScrollDown);
        if wheel && !inside {
            if d.closes_outside() {
                modal.wheel_outside(self, m);
            }
            return;
        }
        if !wheel && !closes {
            return;
        }
        let a = d.mouse(m.kind, at, &drawn);
        modal.set_scroll(self, d.scroll());
        if !self.modal_answer(modal, a) {
            // A click has no return value to end the session with.
            self.ending = true;
        }
        if a == Answer::Close && !inside {
            self.click_through(m);
        }
    }

    /// Act on what modal `m` was answered. Once answered, its drawing is
    /// no target for the next press. False ends the session.
    fn modal_answer(&mut self, m: &dyn Modal, a: Answer) -> bool {
        if a != Answer::Stay {
            self.hits.dialog = None;
        }
        m.answer(self, a)
    }

    /// The exit guard: every tab with pending items and its count, then the
    /// buttons. `D` or Quit discards it all at once; Esc, `b`, Enter or Back
    /// goes back to the work as it was; `r` or Review opens the review where
    /// there is one.
    pub(super) fn guard_dialog(&self) -> Dialog<'static> {
        let mut tabs: Vec<(String, usize, String)> =
            self.roots.iter().enumerate().map(|(i, r)| (r.name.clone(), self.pending_in(i), String::new())).filter(|t| t.1 > 0).collect();
        // Unsaved theme edits are listed last, as the theme tab is.
        if let Some(e) = self.themes.editor.as_ref().filter(|e| e.dirty()) {
            tabs.push(("theme".into(), 1, format!("  edits to {}", e.theme.name)));
        }
        let reviewable = !tabs.is_empty();
        // A pane's unsaved work, under the screen's title.
        if let Some(what) = self.pane_unsaved() {
            tabs.push((self.title.clone(), 1, format!("  {what}")));
        }
        let total: usize = tabs.iter().map(|t| t.1).sum();
        // A pane's work alone is in no tab.
        let heading = if reviewable { format!(" {total} unsaved in {} tabs", tabs.len()) } else { format!(" {total} unsaved") };
        let mut lines = vec![Line::raw(""), Line::from(Span::styled(heading, theme::changed()))];
        lines.extend(tabs.iter().map(|(name, n, note)| Line::from(vec![Span::raw(format!("   {name:<12}")), Span::styled(format!("●{n}"), theme::changed()), Span::styled(note.clone(), theme::hint())])));
        lines.push(Line::raw(""));
        let mut d = Dialog::new("quit with unsaved changes", 74).body(lines).button(Button::new(Btn::Back, "Back (Esc)", Ground::Keep).key('b').cancel().default());
        // Nothing in a pane is reviewed: back to it, or quit.
        if reviewable {
            d = d.button(Button::new(Btn::Review, format!("Review {} (r)", tabs[0].0), Ground::Accent).key('r'));
        }
        d.button(Button::new(Btn::Quit, "Quit and discard all (D)", Ground::Err).key('D')).build().expect("Back is the cancel and the default")
    }

    /// The key help's lines: the keys, from the pane's bindings or the
    /// tree's own with the row's actions, then the help of the pane or of
    /// the tab shown, then what the keyboard reports.
    pub(super) fn help_lines(&self) -> Vec<Line<'static>> {
        let bold = Style::new().add_modifier(Modifier::BOLD);
        let (mut lines, help) = match &self.pane {
            Some(p) => {
                let bindings = self.bindings();
                let col = bindings.iter().map(|b| b.keys.chars().count()).max().unwrap_or(0) + 2;
                let lines = bindings.iter().map(|b| Line::from(vec![Span::styled(format!("{:<col$}", b.keys), theme::accent()), Span::raw(b.label.clone())])).collect();
                (lines, p.help().map(|text| (self.title.clone(), text)))
            }
            None => {
                let mut lines: Vec<Line<'static>> = KEYS.iter().map(|l| Line::raw(*l)).collect();
                if let Some(here) = self.footer_actions() {
                    lines.extend([Line::raw(""), Line::styled("actions here", bold), Line::from(here)]);
                }
                let tab = self.tab_names().get(self.tab).cloned().unwrap_or_default();
                (lines, self.adapter.help(&tab).map(|text| (tab, text)))
            }
        };
        if let Some((topic, text)) = help {
            lines.extend([Line::raw(""), Line::styled(format!("help: {topic}"), bold)]);
            lines.extend(text.lines().map(|l| Line::raw(l.to_string())));
        }
        if let Some(kb) = self.keyboard_line() {
            lines.extend([Line::raw(""), Line::raw(kb)]);
        }
        lines
    }
}
