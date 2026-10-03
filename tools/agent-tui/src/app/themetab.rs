//! The theme tab's keys and clicks: previewing and choosing a theme, the
//! file actions, and the slot editor. State is in `themestate`, drawing in
//! `themeview`.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;

use super::themestate::{Editor, Focus, NameOp, ThemeAct, CHANNELS, NEW_FROM, ROWS};
use super::items::NameFor;
use super::{App, Btn, Mode};
use crate::named::{self, Done};
use agent_theme::{Source, Theme};

impl App {
    pub(super) fn theme_tab(&self) -> usize {
        self.roots.len()
    }

    pub(super) fn on_theme_tab(&self) -> bool {
        self.pane.is_none() && self.tab == self.theme_tab()
    }

    /// Unsaved edits in the theme editor.
    pub(super) fn theme_dirty(&self) -> bool {
        self.themes.editor.as_ref().is_some_and(Editor::dirty)
    }

    /// The theme a frame draws with: on the theme tab, the one being edited
    /// or the one under the cursor; elsewhere, the active one.
    pub(super) fn shown_theme(&self) -> &Theme {
        if self.on_theme_tab() && !matches!(self.mode, Mode::Review { .. }) {
            if let Some(e) = &self.themes.editor {
                return &e.theme;
            }
            return self.themes.under_cursor().0;
        }
        self.themes.active_theme()
    }

    /// The menu's entries for the theme under the cursor: new, the
    /// named-item actions (rename and delete only for a user file), edit
    /// and shape.
    pub(super) fn theme_acts(&self) -> Vec<ThemeAct> {
        let name = &self.themes.under_cursor().0.name;
        let mut v = vec![ThemeAct::New];
        v.extend(named::acts(&self.themes, name).into_iter().map(ThemeAct::Item));
        v.extend([ThemeAct::Edit, ThemeAct::Shape]);
        v
    }

    /// Make the next lozenge shape the one in use; the adapter keeps it.
    fn next_shape(&mut self) -> Result<String, String> {
        let next = self.shape.next();
        self.adapter.choose_shape(next.name())?;
        self.own_write();
        self.shape = next;
        Ok(format!("shape {} (Nerd Font glyphs unless plain)", next.name()))
    }

    /// Make `name` the active theme: the adapter keeps the choice first.
    pub(super) fn choose_theme(&mut self, name: &str) -> Result<String, String> {
        self.adapter.choose_theme(name)?;
        self.own_write();
        self.themes.active = name.into();
        Ok(format!("{name} is the active theme"))
    }

    /// The adapter kept a choice in a file the tree may be read from. When
    /// that moved the stamp, the next watch owes a reload: it reads the
    /// files, so a change from elsewhere that landed just before this write
    /// is read too, and it keeps the message that says what was done
    /// instead of reporting the screen's own write as a change on disk.
    fn own_write(&mut self) {
        if self.adapter.stamp() != self.stamp {
            self.owed = true;
        }
    }

    /// After a copy, rename or delete: the active choice follows through
    /// the adapter, and a copy opens in the editor.
    pub(super) fn theme_done(&mut self, done: Done) {
        let adapter = &mut self.adapter;
        self.msg = self.themes.settle(&done, |n| adapter.choose_theme(n));
        self.own_write();
        if let Done::Copied { to, .. } = &done {
            if let Some(t) = self.themes.get(to).cloned() {
                self.themes.editor = Some(Editor::new(t, true));
                self.msg += "; editing it";
            }
        }
    }

    fn report(&mut self, r: Result<String, String>) {
        self.msg = match r {
            Ok(m) => m,
            Err(e) => format!("rejected: {e}"),
        };
    }

    pub(super) fn theme_act(&mut self, act: ThemeAct) {
        let (t, src) = self.themes.under_cursor();
        let name = t.name.clone();
        match act {
            ThemeAct::New => self.mode = Mode::Name { op: NameFor::Theme(NameOp::New), buf: String::new() },
            ThemeAct::Item(a) => self.item_act(a, name),
            ThemeAct::Edit if src == Source::Bundled => self.mode = Mode::Name { op: NameFor::Theme(NameOp::EditCopy(name)), buf: String::new() },
            ThemeAct::Shape => {
                let r = self.next_shape();
                self.report(r);
            }
            ThemeAct::Edit => {
                let t = t.clone();
                self.themes.editor = Some(Editor::new(t, true));
                self.msg = format!("editing {name}");
            }
        }
    }

    /// A name was typed for a new theme or an edited copy: do what it was
    /// for, or stay and say why not. Copy and rename are the named-item
    /// flow's (`items`).
    pub(super) fn theme_named(&mut self, op: NameOp, buf: String) {
        let name = buf.trim().to_string();
        if let Err(e) = self.themes.check_name(&name) {
            self.msg = format!("rejected: {e}");
            self.mode = Mode::Name { op: NameFor::Theme(op), buf };
            return;
        }
        let from = |s: &App, n: &str| s.themes.get(n).cloned().expect("listed theme");
        let r = match &op {
            NameOp::New => {
                let base = from(self, NEW_FROM);
                self.themes.create(&base, &name)
            }
            NameOp::EditCopy(f) => {
                let base = from(self, f);
                let mut e = Editor::new(Theme { name: name.clone(), label: name.clone(), ..base }, false);
                e.origin = Some(f.clone());
                self.themes.editor = Some(e);
                Ok(format!("editing {name}, a copy of {f}; ^S writes it"))
            }
        };
        self.report(r);
    }

    pub(super) fn theme_save(&mut self) {
        let Some(e) = &self.themes.editor else { return };
        let t = e.theme.clone();
        match self.themes.save(&t) {
            Ok(path) => {
                if let Some(e) = self.themes.editor.as_mut() {
                    e.saved = t.clone();
                    e.written = true;
                }
                self.themes.focus(&t.name);
                self.msg = format!("saved {}", self.themes.show(&path));
            }
            Err(e) => self.msg = format!("rejected: {e}"),
        }
    }

    pub(super) fn close_editor(&mut self) {
        if let Some(e) = self.themes.editor.take() {
            self.themes.focus(&e.theme.name);
            if self.themes.index_of(&e.theme.name).is_none() {
                self.themes.focus(&e.saved.name);
            }
        }
    }

    /// The theme tab's keys while no modal is open. False ends the session.
    pub(super) fn theme_key(&mut self, k: KeyEvent) -> bool {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if self.themes.editor.is_some() {
            return self.editor_key(k, ctrl);
        }
        let n = self.themes.list().len();
        let cur = self.themes.cursor;
        match k.code {
            KeyCode::Char('q') => return self.quit(),
            KeyCode::Esc => {
                let active = self.themes.index_of(&self.themes.active.clone()).unwrap_or(0);
                if cur == active {
                    return self.quit();
                }
                self.themes.cursor = active;
                self.msg = format!("back to {}", self.themes.active);
            }
            KeyCode::Tab => self.switch_tab((self.tab + 1) % self.tabs()),
            KeyCode::BackTab => self.switch_tab((self.tab + self.tabs() - 1) % self.tabs()),
            KeyCode::Char(c @ '1'..='9') => {
                let i = c as usize - '1' as usize;
                if i < self.tabs() {
                    self.switch_tab(i);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => self.themes.cursor = cur.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.themes.cursor = (cur + 1).min(n - 1),
            KeyCode::Home | KeyCode::Char('g') => self.themes.cursor = 0,
            KeyCode::End | KeyCode::Char('G') => self.themes.cursor = n - 1,
            KeyCode::Enter | KeyCode::Char(' ') => {
                let name = self.themes.under_cursor().0.name.clone();
                let r = self.choose_theme(&name);
                self.report(r);
            }
            KeyCode::Char('a') => self.mode = Mode::ThemeMenu { sel: 0 },
            KeyCode::Char('e') => self.theme_act(ThemeAct::Edit),
            KeyCode::Char('m') => self.toggle_mouse(),
            KeyCode::Char('?') => self.mode = Mode::Help { scroll: 0 },
            KeyCode::Char('w' | 'c' | '/') => {
                self.msg = "a theme saves on its own: Enter makes it active, ^S in the editor writes it".into();
            }
            _ => {}
        }
        true
    }

    fn editor_key(&mut self, k: KeyEvent, ctrl: bool) -> bool {
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        let Some(e) = self.themes.editor.as_mut() else { return true };
        if let Some(buf) = e.hex.as_mut() {
            match k.code {
                KeyCode::Esc => {
                    e.hex = None;
                    self.msg = "hex entry cancelled".into();
                }
                KeyCode::Enter => {
                    let text = buf.clone();
                    match e.set_hex(&text) {
                        Ok(c) => {
                            e.hex = None;
                            self.msg = format!("{} = {}", e.slot().unwrap_or(""), c.hex());
                        }
                        Err(m) => self.msg = format!("rejected: {m}"),
                    }
                }
                KeyCode::Backspace => {
                    buf.pop();
                }
                KeyCode::Char(c) if c.is_ascii_hexdigit() || (c == '#' && buf.is_empty()) => {
                    if buf.trim_start_matches('#').len() < 6 {
                        buf.push(c.to_ascii_lowercase());
                    }
                }
                KeyCode::Char(c) => self.msg = format!("rejected: `{c}` is not a hex digit"),
                _ => {}
            }
            return true;
        }
        if ctrl && k.code == KeyCode::Char('s') {
            self.theme_save();
            return true;
        }
        let colour = e.slot().is_some();
        match (e.focus, k.code) {
            (_, KeyCode::Char('#' | 'e')) if colour => {
                e.hex = Some(String::new());
                self.msg = "type #rrggbb, Enter sets it".into();
            }
            (Focus::Slider(_), KeyCode::Esc | KeyCode::Enter) => e.focus = Focus::Rows,
            (Focus::Slider(ch), KeyCode::Up | KeyCode::Char('k')) => e.focus = Focus::Slider(ch.saturating_sub(1)),
            (Focus::Slider(ch), KeyCode::Down | KeyCode::Char('j')) => e.focus = Focus::Slider((ch + 1).min(2)),
            (Focus::Slider(ch), KeyCode::Left | KeyCode::Right | KeyCode::Char('h' | 'l')) => {
                let sign = if matches!(k.code, KeyCode::Left | KeyCode::Char('h')) { -1.0 } else { 1.0 };
                let by = if shift { CHANNELS[ch].2 } else { 1.0 };
                e.step(ch, sign * by);
            }
            (Focus::Rows, KeyCode::Esc) => {
                if e.dirty() {
                    self.mode = Mode::ThemeUnsaved;
                } else {
                    self.close_editor();
                    self.msg = "editor closed".into();
                }
            }
            (Focus::Rows, KeyCode::Up | KeyCode::Char('k')) => e.move_row(e.row.saturating_sub(1)),
            (Focus::Rows, KeyCode::Down | KeyCode::Char('j')) => e.move_row(e.row + 1),
            (Focus::Rows, KeyCode::Home | KeyCode::Char('g')) => e.move_row(0),
            (Focus::Rows, KeyCode::End | KeyCode::Char('G')) => e.move_row(ROWS - 1),
            (Focus::Rows, KeyCode::Enter | KeyCode::Right | KeyCode::Char(' ' | 'l')) if colour => e.focus = Focus::Slider(0),
            (Focus::Rows, KeyCode::Enter | KeyCode::Left | KeyCode::Right | KeyCode::Char(' ' | 'h' | 'l')) => e.toggle(),
            (_, KeyCode::Char('q')) => return self.quit(),
            (_, KeyCode::Tab) => self.switch_tab((self.tab + 1) % self.tabs()),
            (_, KeyCode::BackTab) => self.switch_tab((self.tab + self.tabs() - 1) % self.tabs()),
            (_, KeyCode::Char(c @ '1'..='9')) if (c as usize - '1' as usize) < self.tabs() => self.switch_tab(c as usize - '1' as usize),
            (_, KeyCode::Char('m')) => self.toggle_mouse(),
            (_, KeyCode::Char('?')) => self.mode = Mode::Help { scroll: 0 },
            _ => {}
        }
        true
    }

    /// Save, Discard or Back, asked by Esc in the editor with unsaved edits.
    pub(super) fn unsaved_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Char('s' | 'S') => {
                self.theme_save();
                if !self.theme_dirty() {
                    self.close_editor();
                }
            }
            KeyCode::Char('d' | 'D') => {
                self.close_editor();
                self.msg = "edits discarded".into();
            }
            KeyCode::Esc | KeyCode::Char('b') | KeyCode::Enter => {}
            _ => self.mode = Mode::ThemeUnsaved,
        }
    }

    /// The theme tab's mouse: a tab, a list row (a second click makes it
    /// active), an editor row, a slider (click, drag, wheel), the hex field
    /// and the Save button.
    pub(super) fn theme_mouse(&mut self, m: MouseEvent) {
        let at = Position::new(m.column, m.row);
        let down = m.kind == MouseEventKind::Down(MouseButton::Left);
        let drag = m.kind == MouseEventKind::Drag(MouseButton::Left);
        if m.kind == MouseEventKind::Up(MouseButton::Left) {
            self.drag = None;
            return;
        }
        let wheel = match m.kind {
            MouseEventKind::ScrollUp => Some(-1.0),
            MouseEventKind::ScrollDown => Some(1.0),
            _ => None,
        };
        // A slider: a click sets it where it lands, a drag follows, the wheel steps.
        let slider = self.hits.sliders.iter().find(|(r, ch)| r.contains(at) || (drag && self.drag == Some(*ch))).copied();
        if let (Some((r, ch)), Some(e)) = (slider, self.themes.editor.as_mut()) {
            if down || drag {
                let x = at.x.clamp(r.x, r.x + r.width - 1) - r.x;
                let top = CHANNELS[ch].1;
                e.focus = Focus::Slider(ch);
                e.set_channel(ch, (x as f64 / (r.width - 1).max(1) as f64 * top).round());
                self.drag = Some(ch);
                return;
            }
            if let Some(dir) = wheel {
                e.focus = Focus::Slider(ch);
                e.step(ch, dir);
                return;
            }
        }
        if let Some(dir) = wheel {
            let code = if dir < 0.0 { KeyCode::Up } else { KeyCode::Down };
            if let Some(e) = self.themes.editor.as_mut() {
                e.focus = Focus::Rows;
            }
            self.theme_key(KeyEvent::new(code, KeyModifiers::NONE));
            return;
        }
        if !down {
            return;
        }
        if let Some(&(_, tab)) = self.hits.tabs.iter().find(|(r, _)| r.contains(at)) {
            return self.switch_tab(tab);
        }
        if self.hits.buttons.iter().any(|(r, b)| r.contains(at) && *b == Btn::Save) {
            return self.theme_save();
        }
        if self.hits.hex.contains(at) {
            if let Some(e) = self.themes.editor.as_mut().filter(|e| e.slot().is_some()) {
                e.hex = Some(String::new());
                self.msg = "type #rrggbb, Enter sets it".into();
            }
            return;
        }
        let list = self.hits.list;
        if !list.contains(at) {
            return;
        }
        let i = self.list.offset() + (at.y - list.y) as usize;
        let n = self.themes.list().len();
        match self.themes.editor.as_mut() {
            Some(e) if i < ROWS => {
                if i == e.row && e.slot().is_none() {
                    e.toggle();
                }
                e.move_row(i);
                e.focus = Focus::Rows;
            }
            Some(_) => {}
            None if i < n => {
                if i == self.themes.cursor {
                    self.theme_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                } else {
                    self.themes.cursor = i;
                }
            }
            None => {}
        }
    }
}
