//! The boxed modals over the shell, each a [`Dialog`]: the exit guard,
//! "close the editor", the key help and the response modal. Whichever is
//! open draws here, and its keys and clicks come back as an [`Answer`] that
//! [`App::modal_answer`] acts on. The response modal's text is wrapped in
//! `response`.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::Frame;

use super::dialog::{Answer, Button, Dialog};
use super::theme::{self, Ground};
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

impl App {
    /// The open modal as a dialog that answers keys and clicks. The key
    /// help and the response modal leave their body out: only the draw needs
    /// it.
    fn modal_dialog(&self) -> Option<Dialog<'static>> {
        match &self.mode {
            Mode::Guard => Some(self.guard_dialog()),
            Mode::ThemeUnsaved => Some(self.unsaved_dialog()),
            Mode::Help { scroll } => Some(self.help_dialog(Vec::new(), *scroll)),
            Mode::Response(s) => Some(s.dialog(&[], 0).build().expect("a response has no buttons")),
            _ => None,
        }
    }

    /// Draw the open modal over `area`, recording where it landed and the
    /// scroll the frame holds it at.
    pub(super) fn draw_modal(&mut self, f: &mut Frame, area: Rect) {
        let sh = self.shape;
        let drawn = match &self.mode {
            Mode::Guard => self.guard_dialog().draw(f, area, sh),
            Mode::ThemeUnsaved => self.unsaved_dialog().draw(f, area, sh),
            Mode::Help { scroll } => self.help_dialog(self.help_lines(), *scroll).draw(f, area, sh),
            Mode::Response(_) => return self.draw_response(f, area),
            _ => return,
        };
        self.set_modal_scroll(drawn.scroll);
        self.hits.dialog = Some(drawn);
    }

    /// Hold the open modal's scroll at `to`.
    pub(super) fn set_modal_scroll(&mut self, to: usize) {
        match &mut self.mode {
            Mode::Help { scroll } => *scroll = to,
            Mode::Response(s) => s.scroll = to,
            _ => {}
        }
    }

    /// A key while a modal is open: it owns every key. False ends the session.
    pub(super) fn modal_key(&mut self, k: KeyEvent) -> bool {
        let Some(mut d) = self.modal_dialog() else { return true };
        let a = d.key(k);
        self.set_modal_scroll(d.scroll());
        self.modal_answer(a)
    }

    /// The mouse while a modal is open. The wheel over it scrolls it. The
    /// wheel outside one that a click outside closes scrolls what is under
    /// it, and over the tree, which has nothing under a response to scroll,
    /// it scrolls the response; outside any other it does nothing. A press
    /// that `closes`, of the left button or beside a pane the right, goes to
    /// the dialog; one that closes it from outside then goes on, as
    /// [`App::click_through`] lets it.
    pub(super) fn modal_mouse(&mut self, m: MouseEvent, closes: bool) {
        let Some(mut d) = self.modal_dialog() else { return };
        // Not drawn since it opened: nothing of it is under the pointer.
        let drawn = self.hits.dialog.clone().unwrap_or_default();
        let at = Position::new(m.column, m.row);
        let inside = drawn.contains(at);
        let wheel = matches!(m.kind, MouseEventKind::ScrollUp | MouseEventKind::ScrollDown);
        if wheel && !inside {
            let up = m.kind == MouseEventKind::ScrollUp;
            match d.closes_outside() {
                false => {}
                true if self.pane.is_none() && matches!(self.mode, Mode::Response(_)) => {
                    self.modal_key(KeyEvent::new(if up { KeyCode::Up } else { KeyCode::Down }, KeyModifiers::NONE));
                }
                true => self.pane_wheel(m),
            }
            return;
        }
        if !wheel && !closes {
            return;
        }
        let a = d.mouse(m.kind, at, &drawn);
        self.set_modal_scroll(d.scroll());
        if !self.modal_answer(a) {
            // A click has no return value to end the session with.
            self.ending = true;
        }
        if a == Answer::Close && !inside {
            self.click_through(m);
        }
    }

    /// Act on what the open modal was answered. False ends the session.
    pub(super) fn modal_answer(&mut self, a: Answer) -> bool {
        match (&self.mode, a) {
            (Mode::Guard, Answer::Button(Btn::Quit)) => {
                self.mode = Mode::Browse;
                self.discard_all();
                return false;
            }
            (Mode::Guard, Answer::Button(Btn::Review)) => {
                self.mode = Mode::Browse;
                self.clear_filter();
                match (0..self.roots.len()).find(|t| self.pending_in(*t) > 0) {
                    Some(first) => {
                        self.switch_tab(first);
                        self.open_review(first);
                    }
                    // Only theme edits are unsaved: their review is the editor.
                    None if self.theme_dirty() => self.switch_tab(self.theme_tab()),
                    // A pane's work has no review: the modal stays.
                    None => self.mode = Mode::Guard,
                }
            }
            (Mode::ThemeUnsaved, Answer::Button(Btn::Save)) => {
                self.mode = Mode::Browse;
                self.theme_save();
                if !self.theme_dirty() {
                    self.close_editor();
                }
            }
            (Mode::ThemeUnsaved, Answer::Button(Btn::Drop)) => {
                self.mode = Mode::Browse;
                self.close_editor();
                self.msg = "edits discarded".into();
            }
            (Mode::Guard | Mode::ThemeUnsaved, Answer::Button(Btn::Back)) | (Mode::Help { .. }, Answer::Close) => self.mode = Mode::Browse,
            (Mode::Response(s), Answer::Close | Answer::Forward(_)) => {
                self.mode = match s.back {
                    Some(tab) => Mode::Review { tab, run: None, discard: false },
                    None => Mode::Browse,
                };
            }
            _ => {}
        }
        true
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

    /// Save, Discard or Back over the theme editor, Back focused.
    fn unsaved_dialog(&self) -> Dialog<'static> {
        let name = self.themes.editor.as_ref().map_or(String::new(), |e| e.theme.name.clone());
        let lines = vec![Line::raw(""), Line::styled(format!(" {name} has unsaved edits"), theme::changed()), Line::raw("")];
        Dialog::new("close the editor", 56)
            .body(lines)
            .button(Button::new(Btn::Save, "Save (s)", Ground::Ok).key('s').key('S'))
            .button(Button::new(Btn::Drop, "Discard (d)", Ground::Err).key('d').key('D'))
            .button(Button::new(Btn::Back, "Back (Esc)", Ground::Keep).key('b').cancel().default())
            .build()
            .expect("Back is the cancel and the default")
    }

    /// The key help over `lines`, scrolled `scroll` rows. Any key it has no
    /// use for closes it, and so does a click, on it or outside it.
    fn help_dialog(&self, lines: Vec<Line<'static>>, scroll: usize) -> Dialog<'static> {
        let (title, least) = if self.pane.is_some() { ("keys · ↑↓ scroll · any other key closes", 56) } else { ("keys and help · ↑↓ scroll · any other key closes", 72) };
        let widest = lines.iter().map(Line::width).max().unwrap_or(0).min(usize::from(u16::MAX - 2)) as u16;
        Dialog::new(title, widest.max(least) + 2).body(lines).scroll(scroll).close_on_any_key().close_outside().build().expect("the key help has no buttons")
    }

    /// The key help's lines: the keys, from the pane's bindings or the
    /// tree's own with the row's actions, then the help of the pane or of
    /// the tab shown, then what the keyboard reports.
    fn help_lines(&self) -> Vec<Line<'static>> {
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
