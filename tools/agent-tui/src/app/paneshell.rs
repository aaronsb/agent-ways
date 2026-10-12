//! The shell around a [`Pane`]: its tab bar, the frame between the bars,
//! the bottom bar with the footer, and the keys and clicks the shell takes
//! before the pane gets the rest. The overlays (the key help, the exit
//! guard, the response modal, the picker) are the tree shell's own; a pane
//! opens the modal, the picker and a question through [`Pane::take_open`].

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;

use super::pane::{footer_spans, Binding, Keyed, Open, Tone};
use super::pick::Pick;
use super::render::{modal_rect, pane as bordered};
use super::response::Shown;
use super::theme::{self, Seg};
use super::*;
use crate::adapter::Printed;
use crate::wrap::str_width;

fn width(spans: &[Span]) -> u16 {
    spans.iter().map(|s| s.width() as u16).sum()
}

/// `text` cut to `room` columns, ending in `…` when it was cut.
fn clip(text: &str, room: usize) -> String {
    if str_width(text) <= room {
        return text.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in text.chars() {
        let cw = str_width(c.encode_utf8(&mut [0; 4]));
        if w + cw + 1 > room {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

/// A pane's question on the bottom bar ([`Open::Ask`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Asked {
    pub id: String,
    pub question: String,
    pub yes: String,
    pub no: String,
}

/// A key the shell takes before the pane, read by [`App::shell_key`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ShellKey {
    /// Ctrl+N: the pane's new item.
    NewItem,
    /// A key on the focused tab bar, its cursor on this tab.
    Strip(usize),
    /// F2 or Ctrl+T: the tab bar takes the focus.
    StripFocus,
    /// Ctrl+digit: tab n (from 1; 0 is the bar's last tab), or its menu on
    /// the tab shown.
    TabOrMenu(usize),
    /// Alt+digit, or a plain digit beside no text: tab n (from 1; 0 the last).
    Tab(usize),
    /// F1, or `?` beside no text: the key help.
    Help,
    /// Ctrl+C, or `q` beside no text.
    Quit,
    /// Alt+m, or `m` beside no text: the mouse on or off.
    Mouse,
}

impl App {
    /// The pane's unsaved work, for the exit guard.
    /// The pane's unsaved work, and a line typed in its entry dialog, for
    /// the exit guard.
    pub(super) fn pane_unsaved(&self) -> Option<String> {
        let typed = match &self.mode {
            Mode::Entry(e) if !e.input.is_empty() => Some(format!("{} being typed", e.title)),
            Mode::Guard { .. } => self.entry_held.as_ref().map(|e| format!("{} being typed", e.title)),
            _ => None,
        };
        let own = self.pane.as_ref()?.unsaved();
        match (typed, own) {
            (Some(t), Some(o)) => Some(format!("{t}, and {o}")),
            (t, o) => t.or(o),
        }
    }

    pub(super) fn owns_text(&self) -> bool {
        self.pane.as_ref().is_some_and(|p| p.owns_text())
    }

    /// The shell's own keys around the pane, in the form they take there:
    /// Alt chords beside a pane that owns text. The key help comes first,
    /// so the footer always has room for the key that finds the rest.
    ///
    /// Beside text the footer names Ctrl+1-9 and F2 for the tabs, not the
    /// Alt digits, which Konsole and GNOME Terminal keep for their own tabs.
    pub fn shell_bindings(&self) -> Vec<Binding> {
        let text = self.owns_text();
        let k = |plain: &str, chord: &str| if text { chord.to_string() } else { plain.to_string() };
        let mut out = vec![Binding::new(k("?", "F1"), "keys")];
        let keys = self.tab_keys();
        let menus = self.has_tab_menus();
        let again = if keys.menu_on_repeat && menus { "tab (again: menu)" } else { "tab" };
        let (ctrl, alt) = (self.ctrl_shown(), self.alt_jump());
        // With reserved tabs, which carry no number, 0 is the bar's last tab.
        let digits = if self.has_reserved_tab() { "0-9" } else { "1-9" };
        match (text, ctrl, alt) {
            (false, _, _) => out.push(Binding::new(digits, "tabs")),
            (true, true, _) => out.push(Binding::new(format!("Ctrl+{digits}"), again).short("tab")),
            (true, false, true) => out.push(Binding::new(format!("Alt+{digits}"), again).short("tab")),
            (true, false, false) => {}
        }
        if let Some(f) = keys.focus_label().filter(|_| menus) {
            out.push(Binding::new(f, "tabs"));
        }
        if menus && keys.f2 && keys.ctrl_t {
            out.push(Binding::help("Ctrl+T", "the tab bar, as F2: ← → move, Enter its menu, Esc back"));
        }
        if !text && ctrl {
            out.push(Binding::help(format!("Ctrl+{digits}"), again));
        }
        if text && ctrl && alt {
            out.push(Binding::help(format!("Alt+{digits}"), "a tab, where the terminal passes Alt+digits on (Konsole keeps them)"));
        }
        if let Some(n) = self.new_item_label() {
            out.push(Binding::help("Ctrl+N", format!("{n}, from the compose box or the tab bar")));
        }
        if menus {
            let what = if keys.menu_on_repeat { "its menu; so does a click on the tab shown" } else { "its menu" };
            match self.pane.as_ref().and_then(|p| p.right_click_help()) {
                Some(h) => out.push(Binding::help("right-click", h)),
                None => out.push(Binding::help("right-click a tab", what)),
            }
        }
        let quit = self.pane.as_ref().and_then(|p| p.quit_help()).unwrap_or_else(|| "quit; asks first over unsaved work".into());
        let quit_keys = if self.pane.as_ref().is_none_or(|p| p.esc_quits()) { k("q Esc ^C", "Esc ^C") } else { k("q ^C", "^C") };
        out.extend([
            Binding::help(self.mouse_key(), "mouse on or off (off lets the terminal select text)"),
            Binding::help("Shift-drag", "selects text while the mouse is on, in most terminals"),
            Binding::help("middle-click", "pastes only while the mouse is off"),
            Binding::help(quit_keys, quit),
        ]);
        let click = self.pane.as_ref().and_then(|p| p.click_help()).unwrap_or_else(|| "a tab shows it; the wheel scrolls".into());
        out.push(Binding::help("click", click));
        out
    }

    /// Whether the pane's bar, as last drawn, has a reserved tab, which
    /// only the 0 key reaches by number.
    fn has_reserved_tab(&self) -> bool {
        self.has_reserved
    }

    /// The key that turns the mouse on or off as the bar names it: `m`, or
    /// `M-m` beside text.
    pub(super) fn mouse_key(&self) -> &'static str {
        if self.owns_text() {
            "M-m"
        } else {
            "m"
        }
    }

    /// The key help, then the pane's bindings, then the rest of the shell's.
    pub fn bindings(&self) -> Vec<Binding> {
        let mut shell = self.shell_bindings().into_iter();
        let own = self.pane.as_ref().map(|p| p.bindings()).unwrap_or_default();
        shell.next().into_iter().chain(own).chain(shell).collect()
    }

    /// One frame of a pane screen: the tab bar, the pane, an overlay, the
    /// bottom bar.
    pub(super) fn draw_pane_frame(&mut self, f: &mut Frame) {
        let [bar, main, status] = Layout::vertical([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)]).areas(f.area());
        self.draw_pane_tabs(f, bar);
        self.hits.pane = main;
        let overlaid = !matches!(self.mode, Mode::Browse);
        if let Some(p) = &mut self.pane {
            p.overlaid(overlaid);
            p.draw(f, main);
        }
        match &self.mode {
            Mode::Help { scroll } => {
                let scroll = *scroll;
                self.draw_pane_help(f, main, scroll);
            }
            Mode::Guard { .. } => self.draw_guard(f, main),
            Mode::Response(_) => self.draw_response(f, main),
            Mode::Pick(p) => {
                let p = p.clone();
                self.draw_pick(f, main, &p);
            }
            Mode::Entry(e) => {
                let e = e.clone();
                self.draw_entry(f, main, &e);
            }
            _ => {}
        }
        self.draw_status(f, status);
    }

    /// The bottom bar over a pane: its lozenge, the message, then the mouse
    /// and as many of the bindings as fit after it. The message comes
    /// first, so the shell's own parts never cut it short: the lozenge goes
    /// when the message needs its room, and a message longer than the bar
    /// ends in `…`.
    pub(super) fn pane_status(&mut self, area: Rect) -> Vec<Span<'static>> {
        let key = self.mouse_key();
        // As the settings tree names it; Shift-drag is in the key help.
        let mouse = if self.mouse { format!("mouse on ({key})") } else { format!("mouse off ({key})") };
        let footer: Vec<Binding> = self.bindings().into_iter().filter(|b| b.footer).collect();
        let focused = self.strip.is_some();
        let Some(p) = &mut self.pane else { return Vec::new() };
        // While the tab bar has the focus the bar says so, over the pane's mode.
        let (mode, ground) = if focused { ("TABS".to_string(), theme::Ground::Accent) } else { (p.mode(), p.mode_ground()) };
        let lozenge = self.shape.lozenge(&[Seg::on(format!(" {mode} "), ground).bold()]);
        // What the shell said last (the mouse toggled, a middle click's
        // paste hint) until input next reaches the pane: a key, a click in
        // it, or a pick from a menu it opened. Else the pane's own, so a
        // question the pane asks after a pick is always the one shown.
        let said = if self.msg.is_empty() { p.status() } else { Some((self.msg.clone(), Tone::Said)) };
        let said = said.filter(|(t, _)| !t.is_empty());
        let mut spans = Vec::new();
        match &said {
            Some((text, tone)) => {
                let style = match tone {
                    Tone::Err => theme::err(),
                    Tone::Said => theme::body(),
                    Tone::Back => theme::muted(),
                };
                if width(&lozenge) as usize + 1 + str_width(text) <= area.width as usize {
                    spans.extend(lozenge);
                }
                let room = (area.width as usize).saturating_sub(width(&spans) as usize + 1);
                spans.push(Span::raw(" "));
                spans.push(Span::styled(clip(text, room), style));
            }
            None => spans.extend(lozenge),
        }
        // The mouse, then the keys while they fit. The key help comes first
        // among the keys, and takes the mouse's place when only one of them
        // fits. A key with a short label takes it when that keeps a later
        // key on the bar, and its whole label wherever there is room.
        let mouse = Span::styled(mouse, theme::hint());
        let keys = |n: usize| footer_spans(footer[..n].iter().map(|b| (b.keys.as_str(), b.label.as_str())));
        let sep = |first: bool| if first && said.is_none() { Span::raw(" ") } else { theme::sep() };
        let first_keys = if footer.is_empty() { 0 } else { 1 };
        let with_mouse = width(&spans) + width(&[sep(true), mouse.clone(), theme::sep()]) + width(&keys(first_keys)) <= area.width;
        let mut first = true;
        if with_mouse || (footer.is_empty() && width(&spans) + width(&[sep(true), mouse.clone()]) <= area.width) {
            spans.extend([sep(true), mouse]);
            first = false;
        }
        let fits = |items: &[(&str, &str)]| width(&spans) + width(&[sep(first)]) + width(&footer_spans(items.iter().copied())) <= area.width;
        let mut shown: Vec<(&str, &str)> = Vec::new();
        for b in &footer {
            shown.push((b.keys.as_str(), b.short.as_deref().unwrap_or(&b.label)));
            if !fits(&shown) {
                shown.pop();
                break;
            }
        }
        for (i, b) in footer.iter().enumerate().take(shown.len()) {
            let short = std::mem::replace(&mut shown[i].1, b.label.as_str());
            if !fits(&shown) {
                shown[i].1 = short;
            }
        }
        if !shown.is_empty() {
            let keys = footer_spans(shown);
            spans.push(sep(first));
            spans.extend(keys);
        }
        spans
    }

    /// The key help over a pane: every binding, then the pane's own help.
    fn draw_pane_help(&mut self, f: &mut Frame, area: Rect, scroll: u16) {
        let bindings = self.bindings();
        let col = bindings.iter().map(|b| b.keys.chars().count()).max().unwrap_or(0) + 2;
        let mut lines: Vec<Line> =
            bindings.iter().map(|b| Line::from(vec![Span::styled(format!("{:<col$}", b.keys), theme::accent()), Span::raw(b.label.clone())])).collect();
        if let Some(text) = self.pane.as_ref().and_then(|p| p.help()) {
            lines.push(Line::raw(""));
            lines.push(Line::styled(format!("help: {}", self.title), Style::new().add_modifier(Modifier::BOLD)));
            lines.extend(text.lines().map(|l| Line::raw(l.to_string())));
        }
        if let Some(kb) = self.keyboard_line() {
            lines.push(Line::raw(""));
            lines.push(Line::raw(kb));
        }
        let widest = lines.iter().map(|l| l.width()).max().unwrap_or(0) as u16;
        let r = modal_rect(area, widest.max(56) + 2, lines.len() as u16 + 2);
        let room = r.height.saturating_sub(2);
        let scroll = scroll.min((lines.len() as u16).saturating_sub(room));
        f.render_widget(Clear, r);
        f.render_widget(Paragraph::new(lines).scroll((scroll, 0)).block(bordered("keys · ↑↓ scroll · any other key closes").border_style(theme::modal_border())), r);
        // Its rect, so a click outside it is told from one on it.
        self.hits.menu = Some((r, Vec::new()));
    }

    /// The key `k` names when the shell takes it before the pane, or None
    /// when it is the pane's.
    fn shell_key(&self, k: KeyEvent) -> Option<ShellKey> {
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            return Some(ShellKey::Quit);
        }
        if let Some(s) = self.tab_bar_key(k) {
            return Some(s);
        }
        let text = self.owns_text();
        let plain = !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        // The shell's letter and digit keys: plain, or the Alt chord, which
        // is the only form beside text.
        let shell = k.modifiers == KeyModifiers::ALT || (!text && plain);
        // Alt+digit is Ctrl+digit where Ctrl+digits do not arrive, and the
        // footer says so: again on the tab shown, its menu.
        let alt_digit = k.modifiers == KeyModifiers::ALT && self.alt_jump();
        match k.code {
            KeyCode::F(1) => Some(ShellKey::Help),
            KeyCode::Char('?') if !text && plain => Some(ShellKey::Help),
            KeyCode::Char('q') if !text && plain => Some(ShellKey::Quit),
            KeyCode::Char('m') if shell => Some(ShellKey::Mouse),
            KeyCode::Char(c @ '0'..='9') if alt_digit => Some(ShellKey::TabOrMenu(c as usize - '0' as usize)),
            KeyCode::Char(c @ '0'..='9') if !text && plain => Some(ShellKey::Tab(c as usize - '0' as usize)),
            _ => None,
        }
    }

    /// A key while a pane is shown. The shell takes its own first (a tab,
    /// the mouse, the key help, quitting); the pane gets the rest; what the
    /// pane passes on and quits, quits.
    ///
    /// A key the shell takes is one the pane never sees, so the pane is
    /// told first ([`Pane::interrupted`]): a list holding the keyboard lets
    /// go of it. Every shell key passes here, so a new one is covered by
    /// being added to [`App::shell_key`]. A question the pane asked is the
    /// shell's ([`Mode::Ask`]) and is answered before a key reaches here.
    pub(super) fn pane_key(&mut self, k: KeyEvent) -> bool {
        let taken = self.shell_key(k);
        if self.strip.is_some() && !matches!(taken, Some(ShellKey::Strip(_))) {
            // A key off the focused bar leaves it, then goes where it would.
            self.strip_blur();
        }
        if let Some(s) = taken {
            // A pane that owns the focus keeps it across the shell's keys.
            if !self.pane.as_ref().is_some_and(|p| p.owns_focus()) {
                self.interrupt_pane();
            }
            return self.shell_act(s, k);
        }
        self.msg.clear();
        let Some(p) = &mut self.pane else { return true };
        let keyed = p.key(k);
        self.take_pane_open();
        match keyed {
            Keyed::Done => true,
            Keyed::Pass if k.code == KeyCode::Esc && self.pane.as_ref().is_some_and(|p| p.esc_quits()) => self.quit(),
            Keyed::Pass => true,
            Keyed::Quit => self.quit(),
            Keyed::Tabs => {
                self.strip_focus();
                true
            }
        }
    }

    /// Act on a key the shell took. False ends the session.
    fn shell_act(&mut self, s: ShellKey, k: KeyEvent) -> bool {
        match s {
            ShellKey::NewItem => self.new_item(),
            ShellKey::Strip(at) => self.strip_key(at, k),
            ShellKey::StripFocus => self.strip_focus(),
            ShellKey::TabOrMenu(n) => {
                if let Some(i) = self.numbered(n) {
                    self.activate_tab(i, false, false);
                }
            }
            ShellKey::Tab(n) => {
                if let Some(i) = self.numbered(n) {
                    self.pane_tab(i);
                }
            }
            ShellKey::Help => self.mode = Mode::Help { scroll: 0 },
            ShellKey::Quit => return self.quit(),
            ShellKey::Mouse => {
                self.toggle_mouse();
                // The bar names the state already; only what off means is said.
                if self.mouse {
                    self.msg.clear();
                } else {
                    self.msg = "the terminal selects text".into();
                }
            }
        }
        true
    }

    /// The exit guard beside a pane that owns text, where keystrokes are in
    /// flight when it opens. `D` arms the quit and Enter (or ^C again)
    /// confirms it; no letter does, since letters are typing. Unarmed, Esc
    /// and Enter go back to the work, and a character or an editing key
    /// closes the guard and acts on the work. Armed, Esc disarms it, and a
    /// character or an editing key types the arming `D` first, then acts:
    /// typing on past an Esc reaches the draft as typed.
    pub(super) fn guard_beside_text(&mut self, k: KeyEvent, confirm: bool) -> bool {
        let plain = !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        let edits = plain
            && matches!(
                k.code,
                KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Delete | KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down | KeyCode::Home | KeyCode::End
            );
        match (confirm, k.code) {
            (true, KeyCode::Enter) => {
                self.discard_all();
                return false;
            }
            (true, KeyCode::Esc) => self.mode = Mode::Guard { confirm: false },
            (true, _) if edits => {
                // Back to typing: the D that armed the quit was typing too.
                self.pane_key(KeyEvent::new(KeyCode::Char('D'), KeyModifiers::NONE));
                return self.pane_key(k);
            }
            (true, _) => self.mode = Mode::Guard { confirm: true },
            (false, KeyCode::Char('D')) if plain => self.mode = Mode::Guard { confirm: true },
            (false, KeyCode::Esc | KeyCode::Enter) => {}
            (false, _) if edits => return self.pane_key(k),
            (false, _) => self.mode = Mode::Guard { confirm: false },
        }
        true
    }

    /// Open what the pane asked for: a report or an error in the response
    /// modal, or a picker whose choice goes back to the pane.
    ///
    /// A picker docks where its [`crate::Anchor`] says ([`App::dock`]).
    pub(super) fn take_pane_open(&mut self) {
        let Some(p) = self.pane.as_mut() else { return };
        let Some(open) = p.take_open() else { return };
        match open {
            Open::Report { label, command, text } => {
                let printed = Printed { code: Some(0), stdout: text, stderr: String::new() };
                if let Some(s) = Shown::of(&label, &command, tree::Response::Report, &Ok(()), Some(printed)) {
                    self.show_response(s);
                }
            }
            Open::Error { label, command, text } => {
                if let Some(s) = Shown::of(&label, &command, tree::Response::Write, &Err(text), None) {
                    self.show_response(s);
                }
            }
            Open::Pick { id, title, items, multi, anchor } if matches!(self.mode, Mode::Browse) && !items.is_empty() => {
                let mut pick = Pick::for_pane(id, title, items, multi);
                pick.dock = self.dock(anchor);
                self.mode = Mode::Pick(pick);
            }
            Open::Pick { .. } => {}
            Open::Ask { id, question, yes, no } if matches!(self.mode, Mode::Browse) => {
                self.mode = Mode::Ask(Asked { id, question, yes, no });
            }
            Open::Ask { .. } => {}
            Open::Entry { id, title, value, error, anchor } if matches!(self.mode, Mode::Browse) => {
                let mut input = crate::input::Input::new();
                let n = value.chars().count();
                input.set(value, n);
                let dock = self.dock(anchor);
                self.mode = Mode::Entry(Box::new(super::entry::Entry { id, title, input, error, dock }));
            }
            Open::Entry { .. } => {}
            Open::Quit => {
                if !self.quit() {
                    self.ending = true;
                }
            }
        }
    }

    /// A key while the pane's question waits. `y` answers yes; `n` and Esc
    /// answer no; any other key answers no, and then a character the pane
    /// types, or a key the shell takes, goes on as if nothing were asked.
    /// Enter and the other editing keys only answer, so an Enter meant for
    /// the question never sends a draft. False ends the session.
    pub(super) fn ask_key(&mut self, a: Asked, k: KeyEvent) -> bool {
        let plain = !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        match k.code {
            KeyCode::Char('y' | 'Y') if plain => self.answer(a, true),
            KeyCode::Char('n' | 'N') if plain => self.answer(a, false),
            KeyCode::Esc => self.answer(a, false),
            _ => {
                // A pane that owns the focus types a character wherever its focus is.
                let typed = plain && matches!(k.code, KeyCode::Char(_)) && (self.owns_text() || self.pane.as_ref().is_some_and(|p| p.owns_focus()));
                let goes_on = typed || self.shell_key(k).is_some();
                self.answer(a, false);
                if goes_on && matches!(self.mode, Mode::Browse) {
                    return self.pane_key(k);
                }
            }
        }
        true
    }

    /// Give the pane its question's answer, through [`Pane::picked`].
    pub(super) fn answer(&mut self, a: Asked, yes: bool) {
        self.mode = Mode::Browse;
        self.pane_picked(&a.id, vec![if yes { "y" } else { "n" }.to_string()]);
    }

    /// The bottom bar while the pane's question waits: the question, cut
    /// to leave room for its answers, then the answers as lozenges, each a
    /// click target, as the shell's own confirms draw theirs.
    pub(super) fn ask_status(&mut self, area: Rect, a: &Asked) -> Vec<Span<'static>> {
        let sh = self.shape;
        let yes = format!(" y {} ", a.yes);
        let no = format!(" n {} ", a.no);
        let answers = width(&sh.lozenge(&[Seg::on(yes.clone(), theme::Ground::Err)])) + width(&sh.lozenge(&[Seg::on(no.clone(), theme::Ground::Ok)])) + 2;
        let lozenge = sh.lozenge(&[Seg::on(" confirm ", theme::Ground::Warn).bold()]);
        let mut spans = Vec::new();
        // The answers are the click targets: the lozenge, then the
        // question, give way before they do.
        let room = |used: u16| (area.width.saturating_sub(used + answers + 3)) as usize;
        if str_width(&a.question) <= room(width(&lozenge)) {
            spans.extend(lozenge);
        }
        let question = clip(&a.question, room(width(&spans)));
        spans.push(Span::raw(format!(" {question}  ")));
        super::render::answer_lozenges(sh, area, &mut spans, &mut self.hits.answers, [(true, &yes, theme::Ground::Err), (false, &no, theme::Ground::Ok)]);
        spans
    }

    /// The pane's picker set `values`. The pick is input to the pane, so
    /// what the shell said before gives way to what the pane says now.
    pub(super) fn pane_picked(&mut self, id: &str, values: Vec<String>) {
        self.msg.clear();
        if let Some(p) = &mut self.pane {
            p.picked(id, values);
        }
        self.take_pane_open();
    }

    /// The mouse while a pane is shown: a click on a tab shows it, and on
    /// the tab shown opens its menu, as Ctrl+digit does and a right click
    /// does at once; the wheel and a click inside the pane are the pane's.
    /// A click or a right click on the tab bar or the bottom bar, and a
    /// right click in the pane, first closes what the pane holds open over
    /// its content ([`Pane::dismiss`]), as a left click in the pane does.
    /// An action slot (`≡`, `+`) is the exception: its own action decides,
    /// as its key does, so a click and the key land in the same state.
    /// A middle click pastes nothing while the shell has the mouse, and the
    /// bar says so.
    pub(super) fn pane_mouse(&mut self, m: MouseEvent) {
        let at = Position::new(m.column, m.row);
        let inside = self.hits.pane.contains(at);
        let right = m.kind == MouseEventKind::Down(MouseButton::Right);
        let slot = self.hits.tabs.iter().find(|(r, _)| r.contains(at)).map(|&(_, i)| i);
        let on_action = slot.is_some_and(|i| self.pane.as_mut().is_some_and(|p| p.tabs().get(i).is_some_and(|t| t.action)));
        if Self::is_press(&m) && (!inside || right) && !on_action {
            if let Some(p) = &mut self.pane {
                p.dismiss(inside.then_some(at));
            }
        }
        match m.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown if self.hits.pane.contains(at) => {
                if let Some(p) = &mut self.pane {
                    p.wheel_at(m.kind == MouseEventKind::ScrollUp, at);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(&(_, i)) = self.hits.tabs.iter().find(|(r, _)| r.contains(at)) {
                    self.interrupt_pane();
                    return self.activate_tab(i, false, true);
                }
                if self.hits.pane.contains(at) {
                    self.msg.clear();
                    if let Some(p) = &mut self.pane {
                        p.click(at);
                    }
                } else {
                    // The bottom bar: no target, but a click elsewhere
                    // still takes the focus from wherever the pane held it.
                    self.interrupt_pane();
                }
            }
            MouseEventKind::Down(MouseButton::Right) => {
                if let Some(&(_, i)) = self.hits.tabs.iter().find(|(r, _)| r.contains(at)) {
                    self.interrupt_pane();
                    return self.activate_tab(i, true, true);
                }
                // Inside the pane: the menu a left click there would reach.
                if inside {
                    self.msg.clear();
                    if let Some(p) = &mut self.pane {
                        p.context(at);
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Middle) => {
                self.interrupt_pane();
                let key = self.mouse_key();
                self.msg = format!("middle-click pastes with the mouse off: {key}, paste, {key}");
                return;
            }
            _ => return,
        }
        self.take_pane_open();
    }

    /// The wheel outside an overlay over a pane: what is under it in the
    /// pane scrolls, as with nothing open.
    pub(super) fn pane_wheel(&mut self, m: MouseEvent) {
        let at = Position::new(m.column, m.row);
        if self.hits.pane.contains(at) {
            if let Some(p) = &mut self.pane {
                p.wheel_at(m.kind == MouseEventKind::ScrollUp, at);
            }
        }
    }

    /// A click or a right click outside an overlay, which has closed: on the
    /// tab bar, or on a part of the pane that passes clicks through
    /// ([`Pane::click_through`]), it goes on and acts as it would with
    /// nothing open. Elsewhere it has done its work by closing.
    pub(super) fn click_through(&mut self, m: MouseEvent) {
        if !matches!(self.mode, Mode::Browse) || self.pane.is_none() {
            return;
        }
        let at = Position::new(m.column, m.row);
        let on_tab = self.hits.tabs.iter().any(|(r, _)| r.contains(at));
        let through = self.hits.pane.contains(at) && self.pane.as_ref().is_some_and(|p| p.click_through(at));
        if on_tab || through {
            self.pane_mouse(m);
        }
    }

    /// Whether `m` presses the left or the right button: what closes an
    /// overlay when it lands outside it.
    pub(super) fn is_press(m: &MouseEvent) -> bool {
        matches!(m.kind, MouseEventKind::Down(MouseButton::Left | MouseButton::Right))
    }

    /// A paste: the pane's, while nothing is open over it and the tab bar
    /// does not have the focus. A question waiting is answered no first, as
    /// typing answers it, and the paste goes on to the pane. Elsewhere it
    /// is dropped, never typed.
    pub fn paste(&mut self, text: &str) {
        if let Mode::Ask(a) = &self.mode {
            let a = a.clone();
            self.answer(a, false);
        }
        if matches!(self.mode, Mode::Entry(_)) {
            return self.entry_paste(text);
        }
        if !matches!(self.mode, Mode::Browse) || self.strip.is_some() {
            return;
        }
        if let Some(p) = &mut self.pane {
            p.paste(text);
        }
    }

    /// One tick of the pane, on its own schedule.
    pub fn tick_pane(&mut self) {
        if let Some(p) = &mut self.pane {
            p.tick();
        }
        self.take_pane_open();
        self.open_held();
    }

    /// How often the pane wants its tick.
    pub fn pane_tick_every(&self) -> Option<std::time::Duration> {
        self.pane.as_ref().and_then(|p| p.tick_every())
    }

    /// The palette a pane screen draws in.
    pub(super) fn pane_palette(&self) -> Option<theme::Palette> {
        self.pane.as_ref().map(|p| p.palette())
    }
}

/// The shell as a [`crate::screen::Screen`], so a headless run and the
/// tests drive it as they drive any screen: its keys, its mouse, the
/// pane's tick and a running apply's. On the terminal it runs through
/// [`crate::run`], which also reports the mouse.
impl crate::screen::Screen for App {
    fn palette(&self) -> theme::Palette {
        self.pane_palette().unwrap_or_else(|| self.themes.palette(self.shown_theme()))
    }

    fn draw(&mut self, f: &mut Frame) {
        App::draw(self, f);
    }

    fn key(&mut self, k: KeyEvent) -> bool {
        App::key(self, k)
    }

    fn mouse(&mut self, m: MouseEvent) {
        App::mouse(self, m);
    }

    fn paste(&mut self, text: &str) {
        App::paste(self, text);
    }

    fn tick_every(&self) -> Option<std::time::Duration> {
        self.pane_tick_every()
    }

    fn tick(&mut self) {
        self.tick_pane();
        App::tick(self);
    }
}

#[cfg(test)]
mod tests {
    use super::super::pane::{Anchor, Item, Pane, PaneTab};
    use super::super::panetabs::{Jump, TabKeys};
    use super::*;
    use crate::testkit;

    /// A pane of two tabs: it counts its keys and the wheel, and holds
    /// unsaved work while `dirty` is set.
    struct Two {
        tab: usize,
        keys: usize,
        wheel: i32,
        dirty: bool,
        text: bool,
        /// What the next key opens, and what a picker set.
        open: Option<Open>,
        picked: Vec<String>,
        /// The tabs whose menu was asked for, in order.
        menus: Vec<usize>,
        tab_keys: TabKeys,
        /// `Some`: the pane takes Ctrl+N, counting its uses.
        news: Option<usize>,
    }

    impl Pane for Two {
        fn context(&mut self, at: Position) {
            self.picked.push(format!("context@{},{}", at.x, at.y));
        }
        fn take_open(&mut self) -> Option<Open> {
            self.open.take()
        }
        fn picked(&mut self, id: &str, values: Vec<String>) {
            self.picked = values.into_iter().map(|v| format!("{id}={v}")).collect();
        }
        fn palette(&self) -> theme::Palette {
            theme::Palette::default()
        }
        fn tabs(&mut self) -> Vec<PaneTab> {
            vec![PaneTab::new("one"), PaneTab::new("two"), PaneTab::new("+").action()]
        }
        fn tab_menu(&mut self, i: usize) {
            self.menus.push(i);
        }
        fn has_tab_menus(&self) -> bool {
            true
        }
        fn new_item_label(&self) -> Option<&'static str> {
            self.news.map(|_| "new thing")
        }
        fn new_item(&mut self) {
            self.news = self.news.map(|n| n + 1);
        }
        fn tab_keys(&self) -> TabKeys {
            self.tab_keys
        }
        fn tab(&mut self) -> usize {
            self.tab
        }
        fn set_tab(&mut self, i: usize) {
            self.tab = i;
        }
        fn draw(&mut self, f: &mut Frame, area: Rect) {
            f.render_widget(Paragraph::new(format!("tab {} keys {}", self.tab, self.keys)), area);
        }
        fn key(&mut self, k: KeyEvent) -> Keyed {
            if k.code == KeyCode::Esc {
                return Keyed::Pass;
            }
            self.keys += 1;
            Keyed::Done
        }
        fn wheel(&mut self, up: bool) {
            self.wheel += if up { 1 } else { -1 };
        }
        fn bindings(&self) -> Vec<Binding> {
            vec![Binding::new("x", "count")]
        }
        fn owns_text(&self) -> bool {
            self.text
        }
        fn unsaved(&self) -> Option<String> {
            self.dirty.then(|| "a count".into())
        }
        fn discard(&mut self) {
            self.dirty = false;
        }
    }

    fn app(text: bool) -> App {
        App::with_pane("two", Two { tab: 0, keys: 0, wheel: 0, dirty: false, text, open: None, picked: Vec::new(), menus: Vec::new(), tab_keys: TabKeys::default(), news: None })
    }

    #[test]
    fn a_pane_opens_a_report_in_the_response_modal() {
        let mut a = app(false);
        a.pane_mut::<Two>().expect("the pane").open =
            Some(Open::Report { label: "peers".into(), command: "attend peers".into(), text: "@one\n@two".into() });
        a.key(k(KeyCode::Char('x'), KeyModifiers::NONE));
        let text = testkit::text(&testkit::render(&mut a, 80, 16));
        assert!(text.contains("attend peers") && text.contains("@two"), "{text}");
        a.key(k(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!testkit::text(&testkit::render(&mut a, 80, 16)).contains("@two"), "Esc closes it, not the screen");
    }

    #[test]
    fn a_pane_opens_a_picker_and_hears_the_choice() {
        let mut a = app(false);
        let open = Open::Pick { id: "to".into(), title: "send to".into(), items: vec![Item::new("a").current(true), Item::new("b")], multi: false, anchor: Anchor::Center };
        a.pane_mut::<Two>().expect("the pane").open = Some(open);
        a.key(k(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(testkit::text(&testkit::render(&mut a, 80, 16)).contains("pick one: send to"));
        a.key(k(KeyCode::Down, KeyModifiers::NONE));
        a.key(k(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(two(&a).picked, ["to=b"]);
        assert_eq!(two(&a).keys, 1, "the picker's keys are not the pane's");
    }

    /// A list whose first current item cannot be changed: the cursor starts
    /// on it, Space there toggles nothing, and Enter sets the list at once,
    /// the item that cannot change kept.
    #[test]
    fn enter_sets_a_list_from_a_row_that_cannot_change() {
        let mut a = app(false);
        let items = vec![Item::disabled("d", "fixed").current(true), Item::new("a"), Item::new("c").current(true)];
        a.pane_mut::<Two>().expect("the pane").open = Some(Open::Pick { id: "to".into(), title: "send to".into(), items, multi: true, anchor: Anchor::Center });
        a.key(k(KeyCode::Char('x'), KeyModifiers::NONE));
        a.key(k(KeyCode::Char(' '), KeyModifiers::NONE));
        a.key(k(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(two(&a).picked, ["to=d", "to=c"], "Space left d marked; Enter set the list");
    }

    /// One way to answer a question.
    type Answer = fn(&mut App);

    fn ask() -> Open {
        Open::Ask { id: "q".into(), question: "drop it?".into(), yes: "drop".into(), no: "keep".into() }
    }

    /// A pane's question shows on the bar with its answers as lozenges; `y`,
    /// `n`, Esc and a click on either answer all reach the pane by one path.
    #[test]
    fn a_panes_question_is_answered_by_key_or_by_a_click_on_its_answer() {
        let answers: [(&str, Answer); 5] = [
            ("y", |a: &mut App| assert!(a.key(k(KeyCode::Char('y'), KeyModifiers::NONE)))),
            ("n", |a: &mut App| assert!(a.key(k(KeyCode::Char('n'), KeyModifiers::NONE)))),
            ("n", |a: &mut App| assert!(a.key(k(KeyCode::Esc, KeyModifiers::NONE)))),
            ("y", |a: &mut App| click_text(a, " y drop ")),
            ("n", |a: &mut App| click_text(a, " n keep ")),
        ];
        for (want, answer) in answers {
            let mut a = app(true);
            a.pane_mut::<Two>().expect("the pane").open = Some(ask());
            a.key(k(KeyCode::Char('x'), KeyModifiers::NONE));
            let bar = testkit::rows(&testkit::render(&mut a, 80, 6)).pop().expect("a bar");
            assert!(bar.contains("drop it?") && bar.contains(" y drop ") && bar.contains(" n keep "), "{bar}");
            answer(&mut a);
            assert_eq!(two(&a).picked, [format!("q={want}")]);
            assert_eq!(two(&a).keys, 1, "the answer typed nothing");
            assert!(matches!(a.mode, Mode::Browse), "nothing left asking");
        }
    }

    fn click_text(a: &mut App, text: &str) {
        let buf = testkit::render(a, 80, 6);
        let (x, y) = testkit::find(&buf, text).expect("the answer is drawn");
        a.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE });
    }

    /// Any other key answers no; beside text a character then types, and
    /// Enter only answers, so it never sends what the question was over.
    #[test]
    fn another_key_answers_no_and_a_character_goes_on_as_typed() {
        let mut a = app(true);
        a.pane_mut::<Two>().expect("the pane").open = Some(ask());
        a.key(k(KeyCode::Char('x'), KeyModifiers::NONE));
        a.key(k(KeyCode::Char('z'), KeyModifiers::NONE));
        assert_eq!((two(&a).picked.clone(), two(&a).keys), (vec!["q=n".to_string()], 2), "z answered no, then typed");
        a.pane_mut::<Two>().expect("the pane").open = Some(ask());
        a.key(k(KeyCode::Char('x'), KeyModifiers::NONE));
        a.key(k(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(two(&a).keys, 3, "Enter only answered");
    }

    /// A click anywhere but an answer answers no; on a tab it then acts.
    #[test]
    fn a_click_off_the_answers_answers_no_and_on_a_tab_acts() {
        let mut a = app(true);
        a.pane_mut::<Two>().expect("the pane").open = Some(ask());
        a.key(k(KeyCode::Char('x'), KeyModifiers::NONE));
        click_text(&mut a, "2 two");
        assert_eq!((two(&a).picked.clone(), two(&a).tab), (vec!["q=n".to_string()], 1));
    }

    /// A pane asks the shell to quit: at once with nothing unsaved, which a
    /// click cannot return, so [`App::ended`] says it; over unsaved work,
    /// the exit guard asks.
    #[test]
    fn a_pane_quits_through_the_shell() {
        let mut a = app(true);
        a.pane_mut::<Two>().expect("the pane").open = Some(Open::Quit);
        assert!(!a.key(k(KeyCode::Char('x'), KeyModifiers::NONE)), "the key that quit ends the session");
        assert!(a.ended());
        let mut a = app(true);
        a.pane_mut::<Two>().expect("the pane").dirty = true;
        a.pane_mut::<Two>().expect("the pane").open = Some(Open::Quit);
        assert!(a.key(k(KeyCode::Char('x'), KeyModifiers::NONE)));
        assert!(a.guarding() && !a.ended());
    }

    /// A pane's menu closed with Esc says nothing over the bar.
    #[test]
    fn a_panes_menu_closes_saying_nothing() {
        let mut a = app(false);
        let open = Open::Pick { id: "to".into(), title: "send to".into(), items: vec![Item::new("a")], multi: false, anchor: Anchor::Center };
        a.pane_mut::<Two>().expect("the pane").open = Some(open);
        a.key(k(KeyCode::Char('x'), KeyModifiers::NONE));
        a.key(k(KeyCode::Esc, KeyModifiers::NONE));
        assert!(a.msg.is_empty(), "{}", a.msg);
        assert!(two(&a).picked.is_empty());
    }

    /// Armed, any key but y disarms the quit: D has to be followed by y.
    #[test]
    fn the_armed_quit_takes_only_y() {
        let mut a = app(false);
        a.pane_mut::<Two>().expect("the pane").dirty = true;
        a.key(k(KeyCode::Esc, KeyModifiers::NONE));
        a.key(k(KeyCode::Char('D'), KeyModifiers::NONE));
        assert!(a.key(k(KeyCode::Char('i'), KeyModifiers::NONE)));
        assert!(a.guarding() && a.key(k(KeyCode::Char('y'), KeyModifiers::NONE)), "y alone does not quit");
        assert!(two(&a).dirty);
    }

    /// Beside text, the guard answers only its own keys; the rest is typed.
    #[test]
    fn beside_text_the_guard_lets_typing_through() {
        let mut a = app(true);
        a.pane_mut::<Two>().expect("the pane").dirty = true;
        a.key(k(KeyCode::Esc, KeyModifiers::NONE));
        assert!(a.guarding());
        for c in "Did you".chars() {
            assert!(a.key(k(KeyCode::Char(c), KeyModifiers::NONE)), "{c} keeps the screen open");
        }
        assert!(!a.guarding());
        assert_eq!(two(&a).keys, 7, "the arming D reached the pane too, before the i");
        assert!(two(&a).dirty);
        // No letter confirms beside text: D then y types "Dy".
        a.key(k(KeyCode::Esc, KeyModifiers::NONE));
        a.key(k(KeyCode::Char('D'), KeyModifiers::NONE));
        assert!(a.key(k(KeyCode::Char('y'), KeyModifiers::NONE)));
        assert_eq!(two(&a).keys, 9);
        // Esc disarms, typing nothing; D then Enter quits, as does ^C armed.
        a.key(k(KeyCode::Esc, KeyModifiers::NONE));
        a.key(k(KeyCode::Char('D'), KeyModifiers::NONE));
        a.key(k(KeyCode::Esc, KeyModifiers::NONE));
        assert!(a.guarding() && two(&a).keys == 9);
        a.key(k(KeyCode::Char('D'), KeyModifiers::NONE));
        assert!(!a.key(k(KeyCode::Char('c'), KeyModifiers::CONTROL)), "^C again while armed quits");
        assert!(!two(&a).dirty);
    }

    /// Unarmed, an editing key closes the guard and acts on the work.
    #[test]
    fn beside_text_an_editing_key_closes_the_guard() {
        let mut a = app(true);
        a.pane_mut::<Two>().expect("the pane").dirty = true;
        a.key(k(KeyCode::Esc, KeyModifiers::NONE));
        assert!(a.key(k(KeyCode::Backspace, KeyModifiers::NONE)));
        assert!(!a.guarding());
        assert_eq!(two(&a).keys, 1);
        a.key(k(KeyCode::Esc, KeyModifiers::NONE));
        a.key(k(KeyCode::Char('D'), KeyModifiers::NONE));
        assert!(!a.key(k(KeyCode::Enter, KeyModifiers::NONE)), "D then Enter quits");
    }

    #[test]
    fn a_pane_chooses_whether_the_mouse_starts_on() {
        assert!(app(false).mouse_on(), "the settings default");
        struct Off;
        impl Pane for Off {
            fn palette(&self) -> theme::Palette {
                theme::Palette::default()
            }
            fn tabs(&mut self) -> Vec<PaneTab> {
                Vec::new()
            }
            fn tab(&mut self) -> usize {
                0
            }
            fn set_tab(&mut self, _: usize) {}
            fn draw(&mut self, _: &mut Frame, _: Rect) {}
            fn key(&mut self, _: KeyEvent) -> Keyed {
                Keyed::Done
            }
            fn bindings(&self) -> Vec<Binding> {
                Vec::new()
            }
            fn mouse_default(&self) -> bool {
                false
            }
        }
        assert!(!App::with_pane("off", Off).mouse_on());
    }

    #[test]
    fn a_long_message_drops_the_lozenge_and_ends_in_an_ellipsis() {
        let mut a = app(false);
        a.msg = "x".repeat(70);
        let bar = testkit::rows(&testkit::render(&mut a, 76, 6)).pop().expect("a bar");
        assert!(!bar.contains("browse") && bar.contains(&"x".repeat(70)), "{bar}");
        a.msg = "y".repeat(90);
        let bar = testkit::rows(&testkit::render(&mut a, 76, 6)).pop().expect("a bar");
        assert!(bar.trim_end().ends_with('…'), "{bar}");
    }

    fn two(a: &App) -> &Two {
        a.pane_ref().expect("the pane")
    }

    fn k(c: KeyCode, m: KeyModifiers) -> KeyEvent {
        KeyEvent::new(c, m)
    }

    #[test]
    fn the_shell_takes_plain_keys_beside_a_pane_without_text() {
        let mut a = app(false);
        assert!(a.key(k(KeyCode::Char('2'), KeyModifiers::NONE)));
        assert_eq!(two(&a).tab, 1);
        assert!(a.key(k(KeyCode::Char('9'), KeyModifiers::NONE)), "no ninth tab: nothing");
        assert_eq!(two(&a).tab, 1);
        a.key(k(KeyCode::Char('m'), KeyModifiers::NONE));
        assert!(!a.mouse_on());
        a.key(k(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(two(&a).keys, 1, "the rest is the pane's");
        assert!(!a.key(k(KeyCode::Char('q'), KeyModifiers::NONE)), "q quits with nothing unsaved");
    }

    #[test]
    fn beside_text_the_shell_keys_are_alt_chords() {
        let mut a = app(true);
        a.key(k(KeyCode::Char('2'), KeyModifiers::NONE));
        a.key(k(KeyCode::Char('m'), KeyModifiers::NONE));
        a.key(k(KeyCode::Char('q'), KeyModifiers::NONE));
        assert_eq!((two(&a).tab, two(&a).keys), (0, 3), "plain keys are text");
        assert!(a.mouse_on());
        a.key(k(KeyCode::Char('2'), KeyModifiers::ALT));
        a.key(k(KeyCode::Char('m'), KeyModifiers::ALT));
        assert_eq!(two(&a).tab, 1);
        assert!(!a.mouse_on());
        let footer = testkit::rows(&testkit::render(&mut a, 160, 6)).pop().expect("a bar");
        assert!(footer.contains("mouse off (M-m)") && footer.contains("Alt+1-9 tab (again: menu)") && footer.contains("F2 tabs") && footer.contains("F1 keys"), "{footer}");
        a.set_keyboard_enhanced(true);
        let footer = testkit::rows(&testkit::render(&mut a, 160, 6)).pop().expect("a bar");
        assert!(footer.contains("Ctrl+1-9 tab (again: menu)") && !footer.contains("Alt+1-9"), "auto: Ctrl where the terminal reports it: {footer}");
    }

    #[test]
    fn a_footer_short_of_room_shortens_a_label_before_it_drops_a_key() {
        let mut a = app(true);
        a.key(k(KeyCode::Char('m'), KeyModifiers::ALT));
        let bar = |a: &mut App, w: u16| testkit::rows(&testkit::render(a, w, 6)).pop().expect("a bar");
        let wide = bar(&mut a, 160);
        assert!(wide.contains("Alt+1-9 tab (again: menu) · F2 tabs"), "{wide}");
        let full = str_width(wide.trim_end()) as u16;
        let short = bar(&mut a, full - 5);
        assert!(short.contains("Alt+1-9 tab · F2 tabs") && !short.contains("again"), "the short label keeps both keys: {short}");
        for w in 40..=160 {
            let b = bar(&mut a, w);
            assert!(!b.contains("(again: menu)") || b.contains("F2 tabs"), "the whole label never costs a later key at {w}: {b}");
        }
    }

    #[test]
    fn the_tab_keys_follow_the_setting() {
        let mut a = app(true);
        a.pane_mut::<Two>().expect("the pane").tab_keys = TabKeys { jump: Jump::Alt, menu_on_repeat: false, f2: false, ctrl_t: true };
        a.set_keyboard_enhanced(true);
        a.key(ctrl('2'));
        assert_eq!(two(&a).tab, 0, "jump alt: Ctrl+digits do nothing");
        a.key(k(KeyCode::Char('2'), KeyModifiers::ALT));
        a.key(k(KeyCode::Char('2'), KeyModifiers::ALT));
        assert_eq!((two(&a).tab, two(&a).menus.clone()), (1, vec![]), "no menu on a repeat");
        a.key(k(KeyCode::F(2), KeyModifiers::NONE));
        assert!(!a.strip_focused(), "F2 is off");
        a.key(ctrl('t'));
        assert!(a.strip_focused(), "Ctrl+T is on");
        let footer = testkit::rows(&testkit::render(&mut a, 160, 6)).pop().expect("a bar");
        assert!(footer.contains("Alt+1-9 tab ·") && footer.contains("Ctrl+T tabs"), "{footer}");
        a.key(k(KeyCode::Esc, KeyModifiers::NONE));
        a.pane_mut::<Two>().expect("the pane").tab_keys = TabKeys { jump: Jump::None, ..TabKeys::default() };
        a.key(k(KeyCode::Char('1'), KeyModifiers::ALT));
        a.key(ctrl('1'));
        assert_eq!(two(&a).tab, 1, "jump none: no digit jumps");
    }

    fn ctrl(c: char) -> KeyEvent {
        k(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn ctrl_digit_shows_a_tab_and_again_opens_its_menu() {
        for text in [true, false] {
            let mut a = app(text);
            a.set_keyboard_enhanced(true);
            a.key(ctrl('2'));
            assert_eq!((two(&a).tab, two(&a).menus.clone()), (1, vec![]), "text {text}: the first press shows the tab");
            a.key(ctrl('2'));
            assert_eq!((two(&a).tab, two(&a).menus.clone()), (1, vec![1]), "text {text}: the second opens its menu");
            a.key(ctrl('3'));
            assert_eq!((two(&a).tab, two(&a).menus.clone()), (1, vec![1]), "text {text}: the + slot has no number");
            a.key(ctrl('9'));
            assert_eq!(two(&a).menus.len(), 1, "no ninth tab: nothing");
            assert_eq!(two(&a).keys, 0, "none reached the pane");
        }
    }

    #[test]
    fn f2_and_ctrl_t_give_the_tab_bar_the_focus() {
        let mut a = app(true);
        a.key(k(KeyCode::F(2), KeyModifiers::NONE));
        assert!(a.strip_focused());
        let bar = testkit::rows(&testkit::render(&mut a, 100, 6)).pop().expect("a bar");
        assert!(bar.contains("TABS") && bar.contains("← → move · Enter menu · Esc back") && !bar.contains("Ctrl+N"), "{bar}");
        a.key(k(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(two(&a).tab, 1, "Right shows the next tab");
        a.key(k(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(two(&a).tab, 1, "the + slot is pointed at, not shown");
        a.key(k(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(two(&a).tab, 0, "past the end, the first");
        a.key(k(KeyCode::Left, KeyModifiers::NONE));
        a.key(k(KeyCode::Left, KeyModifiers::NONE));
        a.key(k(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!((two(&a).tab, two(&a).menus.clone(), a.strip_focused()), (1, vec![1], false), "Enter opens the menu");
        a.key(ctrl('t'));
        assert!(a.strip_focused());
        assert!(a.key(k(KeyCode::Esc, KeyModifiers::NONE)), "Esc gives the focus back and quits nothing");
        assert!(!a.strip_focused());
        a.key(k(KeyCode::F(2), KeyModifiers::NONE));
        a.key(k(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!((a.strip_focused(), two(&a).keys), (false, 1), "another key gives the focus back and goes on to the pane");
    }

    /// The focused tab is in reverse video behind a `▸`, on a channel tab
    /// and on an action slot; the bar's lozenge reads TABS.
    #[test]
    fn the_focused_tab_is_marked_and_the_bar_says_tabs() {
        let mut a = app(true);
        let buf = testkit::render(&mut a, 100, 6);
        assert!(!testkit::text(&buf).contains('▸'), "no marker without the focus");
        a.key(k(KeyCode::F(2), KeyModifiers::NONE));
        let buf = testkit::render(&mut a, 100, 6);
        let (x, y) = testkit::find(&buf, "▸1 one").expect("the marker leads the focused tab");
        assert!(buf.cell((x, y)).expect("a cell").modifier.contains(Modifier::REVERSED));
        let (px, _) = testkit::find(&buf, "2 two").expect("the others keep their look");
        assert!(!buf.cell((px, y)).expect("a cell").modifier.contains(Modifier::REVERSED));
        a.key(k(KeyCode::Left, KeyModifiers::NONE));
        let buf = testkit::render(&mut a, 100, 6);
        let (x, y) = testkit::find(&buf, "▸+").expect("the cursor shows on an action slot");
        assert!(buf.cell((x, y)).expect("a cell").modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn ctrl_n_is_the_panes_only_when_it_names_an_action() {
        let mut a = app(true);
        a.key(ctrl('n'));
        assert_eq!((two(&a).keys, two(&a).news), (1, None), "a pane without the action gets the key");
        assert!(!testkit::text(&testkit::render(&mut a, 100, 20)).contains("Ctrl+N"));
        let mut a = app(true);
        a.pane_mut::<Two>().expect("the pane").news = Some(0);
        a.key(ctrl('n'));
        assert_eq!((two(&a).keys, two(&a).news), (0, Some(1)), "from the compose box the shell runs it");
        a.key(k(KeyCode::F(2), KeyModifiers::NONE));
        let bar = testkit::rows(&testkit::render(&mut a, 100, 6)).pop().expect("a bar");
        assert!(bar.contains("Ctrl+N new thing"), "{bar}");
        a.key(ctrl('n'));
        assert_eq!((two(&a).keys, two(&a).news, a.strip_focused()), (0, Some(2), false), "from the tab bar too, and the focus goes back");
        a.key(k(KeyCode::F(1), KeyModifiers::NONE));
        assert!(testkit::text(&testkit::render(&mut a, 100, 24)).contains("new thing, from the compose box or the tab bar"));
    }

    #[test]
    fn a_click_on_the_shown_tab_or_a_right_click_opens_its_menu() {
        let mut a = app(false);
        let buf = testkit::render(&mut a, 60, 8);
        let (x2, y) = testkit::find(&buf, "2 two").expect("the tab");
        let (x1, _) = testkit::find(&buf, "1 one").expect("the tab");
        let ev = |kind, column| MouseEvent { kind, column, row: y, modifiers: KeyModifiers::NONE };
        a.mouse(ev(MouseEventKind::Down(MouseButton::Left), x2));
        assert_eq!((two(&a).tab, two(&a).menus.clone()), (1, vec![]));
        a.mouse(ev(MouseEventKind::Down(MouseButton::Left), x2));
        assert_eq!(two(&a).menus, [1], "a second click on the shown tab opens its menu");
        a.mouse(ev(MouseEventKind::Down(MouseButton::Right), x1));
        assert_eq!((two(&a).tab, two(&a).menus.clone()), (1, vec![1, 0]), "a right click opens a tab's menu without showing it");
    }

    /// A right click inside the pane reaches [`Pane::context`] at its cell;
    /// one on the tab bar opens the tab's menu, as before.
    #[test]
    fn a_right_click_inside_the_pane_asks_it_for_a_menu() {
        let mut a = app(false);
        let buf = testkit::render(&mut a, 60, 8);
        let (_, y) = testkit::find(&buf, "2 two").expect("the tab");
        a.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Right), column: 5, row: y + 3, modifiers: KeyModifiers::NONE });
        assert_eq!(two(&a).picked, [format!("context@5,{}", y + 3)]);
        assert!(two(&a).menus.is_empty(), "no tab's menu");
    }

    #[test]
    fn a_tab_menu_docks_under_its_tab() {
        let mut a = app(false);
        let buf = testkit::render(&mut a, 60, 12);
        let (x2, y) = testkit::find(&buf, "2 two").expect("the tab");
        a.pane_mut::<Two>().expect("the pane").tab = 1;
        let open = Open::Pick { id: "m".into(), title: "two".into(), items: vec![Item::new("a"), Item::new("b")], multi: false, anchor: Anchor::Center };
        a.pane_mut::<Two>().expect("the pane").open = Some(open);
        a.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Right), column: x2, row: y, modifiers: KeyModifiers::NONE });
        let rows = testkit::rows(&testkit::render(&mut a, 60, 12));
        assert!(rows[usize::from(y) + 1].contains("pick one: two"), "the menu's top on the line under the tab: {rows:#?}");
    }

    #[test]
    fn the_key_help_says_whether_ctrl_digits_arrive() {
        struct Asks(Two);
        impl Pane for Asks {
            fn palette(&self) -> theme::Palette {
                self.0.palette()
            }
            fn tabs(&mut self) -> Vec<PaneTab> {
                self.0.tabs()
            }
            fn tab(&mut self) -> usize {
                0
            }
            fn set_tab(&mut self, _: usize) {}
            fn draw(&mut self, _: &mut Frame, _: Rect) {}
            fn key(&mut self, _: KeyEvent) -> Keyed {
                Keyed::Done
            }
            fn bindings(&self) -> Vec<Binding> {
                Vec::new()
            }
            fn keyboard_enhancement(&self) -> bool {
                true
            }
        }
        let two = Two { tab: 0, keys: 0, wheel: 0, dirty: false, text: true, open: None, picked: Vec::new(), menus: Vec::new(), tab_keys: TabKeys::default(), news: None };
        let mut a = App::with_pane("asks", Asks(two));
        a.key(k(KeyCode::F(1), KeyModifiers::NONE));
        let off = testkit::text(&testkit::render(&mut a, 140, 24));
        assert!(off.contains("does not report Ctrl+digits"), "{off}");
        a.set_keyboard_enhanced(true);
        let on = testkit::text(&testkit::render(&mut a, 140, 24));
        assert!(on.contains("Ctrl+digits arrive"), "{on}");
        assert!(!testkit::text(&testkit::render(&mut app(true), 140, 24)).contains("keyboard:"), "a pane that did not ask says nothing");
    }

    /// A pane whose tabs have no menus, as `ways introspect` and `ways
    /// projects`: no tab-bar focus, no menu, and none of their keys named.
    struct Plain(Two);
    impl Pane for Plain {
        fn palette(&self) -> theme::Palette {
            self.0.palette()
        }
        fn tabs(&mut self) -> Vec<PaneTab> {
            vec![PaneTab::new("one"), PaneTab::new("two")]
        }
        fn tab(&mut self) -> usize {
            self.0.tab
        }
        fn set_tab(&mut self, i: usize) {
            self.0.tab = i;
        }
        fn tab_menu(&mut self, i: usize) {
            self.0.menus.push(i);
        }
        fn draw(&mut self, _: &mut Frame, _: Rect) {}
        fn key(&mut self, _: KeyEvent) -> Keyed {
            self.0.keys += 1;
            Keyed::Done
        }
        fn bindings(&self) -> Vec<Binding> {
            Vec::new()
        }
    }

    #[test]
    fn a_pane_without_tab_menus_gets_no_tab_bar_focus_and_names_none() {
        let two = Two { tab: 0, keys: 0, wheel: 0, dirty: false, text: false, open: None, picked: Vec::new(), menus: Vec::new(), tab_keys: TabKeys::default(), news: None };
        let mut a = App::with_pane("plain", Plain(two));
        a.key(k(KeyCode::F(2), KeyModifiers::NONE));
        assert!(!a.strip_focused(), "F2 is the pane's");
        a.key(k(KeyCode::Char('2'), KeyModifiers::NONE));
        a.key(k(KeyCode::Char('2'), KeyModifiers::NONE));
        let p: &Plain = a.pane_ref().expect("the pane");
        assert_eq!((p.0.tab, p.0.menus.len()), (1, 0), "a repeated jump opens no menu");
        let footer = testkit::rows(&testkit::render(&mut a, 160, 6)).pop().expect("a bar");
        assert!(footer.contains("1-9 tabs") && !footer.contains("F2"), "{footer}");
        a.key(k(KeyCode::Char('?'), KeyModifiers::NONE));
        let help = testkit::text(&testkit::render(&mut a, 140, 30));
        assert!(!help.contains("right-click") && !help.contains("Ctrl+T"), "{help}");
    }

    /// A pane that owns text and passes every key on, but says Esc does
    /// not quit, as the chat does (ADR-409).
    struct KeepsEsc;
    impl Pane for KeepsEsc {
        fn palette(&self) -> theme::Palette {
            theme::Palette::default()
        }
        fn tabs(&mut self) -> Vec<PaneTab> {
            vec![PaneTab::new("one")]
        }
        fn tab(&mut self) -> usize {
            0
        }
        fn set_tab(&mut self, _: usize) {}
        fn draw(&mut self, _: &mut Frame, _: Rect) {}
        fn key(&mut self, _: KeyEvent) -> Keyed {
            Keyed::Pass
        }
        fn bindings(&self) -> Vec<Binding> {
            Vec::new()
        }
        fn owns_text(&self) -> bool {
            true
        }
        fn esc_quits(&self) -> bool {
            false
        }
    }

    #[test]
    fn a_pane_can_keep_esc_from_quitting_and_the_help_says_so() {
        let mut a = App::with_pane("keeps", KeepsEsc);
        assert!(a.key(k(KeyCode::Esc, KeyModifiers::NONE)), "Esc passed on quits nothing");
        assert!(!a.guarding());
        a.key(k(KeyCode::F(1), KeyModifiers::NONE));
        let help = testkit::text(&testkit::render(&mut a, 140, 30));
        assert!(help.contains("^C") && !help.contains("Esc ^C"), "{help}");
        a.key(k(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(!a.key(ctrl('c')), "Ctrl-C still quits");
    }

    #[test]
    fn the_footer_names_ctrl_digits_only_where_they_arrive() {
        let mut a = app(true);
        a.pane_mut::<Two>().expect("the pane").tab_keys = TabKeys { jump: Jump::Ctrl, ..TabKeys::default() };
        let footer = testkit::rows(&testkit::render(&mut a, 160, 6)).pop().expect("a bar");
        assert!(!footer.contains("Ctrl+1-9") && footer.contains("F2 tabs"), "not reported here: {footer}");
        a.set_keyboard_enhanced(true);
        let footer = testkit::rows(&testkit::render(&mut a, 160, 6)).pop().expect("a bar");
        assert!(footer.contains("Ctrl+1-9 tab (again: menu)"), "{footer}");
    }

    #[test]
    fn quitting_over_unsaved_work_asks_first() {
        let mut a = app(false);
        a.pane_mut::<Two>().expect("the pane").dirty = true;
        assert!(a.key(k(KeyCode::Esc, KeyModifiers::NONE)));
        assert!(a.guarding());
        let text = testkit::text(&testkit::render(&mut a, 80, 16));
        assert!(text.contains("1 unsaved") && text.contains("a count") && !text.contains("Review"), "{text}");
        a.key(k(KeyCode::Char('r'), KeyModifiers::NONE));
        assert!(!a.guarding(), "r has nothing to review: back to the pane");
        a.key(k(KeyCode::Char('c'), KeyModifiers::CONTROL));
        a.key(k(KeyCode::Char('D'), KeyModifiers::NONE));
        assert!(!a.key(k(KeyCode::Char('y'), KeyModifiers::NONE)));
        assert!(!two(&a).dirty, "the work was dropped");
    }

    #[test]
    fn a_click_on_a_tab_and_the_wheel_over_the_pane() {
        let mut a = app(false);
        let buf = testkit::render(&mut a, 60, 8);
        let (x, y) = testkit::find(&buf, "2 two").expect("the tab");
        let ev = |kind, column, row| MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE };
        a.mouse(ev(MouseEventKind::Down(MouseButton::Left), x, y));
        assert_eq!(two(&a).tab, 1);
        a.mouse(ev(MouseEventKind::ScrollUp, 3, 3));
        a.mouse(ev(MouseEventKind::ScrollUp, 3, 7));
        assert_eq!(two(&a).wheel, 1, "the bottom bar is not the pane");
    }

    /// A pane with the attend-chat bar's shape: the `≡` slot, four tabs with
    /// a lead glyph each, the `+` slot, and a channel description after it.
    struct Described(Two);
    impl Pane for Described {
        fn palette(&self) -> theme::Palette {
            self.0.palette()
        }
        fn tabs(&mut self) -> Vec<PaneTab> {
            let mut out = vec![PaneTab::new("≡").action()];
            for n in ["merged", "#open", "#deploy", "#infra"] {
                out.push(PaneTab::new(n).lead(vec![Span::raw("◆ ")]));
            }
            out.push(PaneTab::new("+").action());
            out
        }
        fn tab(&mut self) -> usize {
            self.0.tab
        }
        fn set_tab(&mut self, i: usize) {
            self.0.tab = i;
        }
        fn has_tab_menus(&self) -> bool {
            true
        }
        fn trailer(&mut self) -> Vec<Span<'static>> {
            vec![Span::raw("  — rollout of the deploy pipeline to every region, one at a time")]
        }
        fn draw(&mut self, _: &mut Frame, _: Rect) {}
        fn key(&mut self, _: KeyEvent) -> Keyed {
            Keyed::Done
        }
        fn bindings(&self) -> Vec<Binding> {
            Vec::new()
        }
    }

    fn described() -> App {
        let two = Two { tab: 3, keys: 0, wheel: 0, dirty: false, text: true, open: None, picked: Vec::new(), menus: Vec::new(), tab_keys: TabKeys::default(), news: None };
        App::with_pane("described", Described(two))
    }

    #[test]
    fn a_bar_too_narrow_for_every_number_keeps_the_tab_shown() {
        let mut a = described();
        a.pane_mut::<Described>().expect("the pane").0.tab = 4;
        let bar = testkit::rows(&testkit::render(&mut a, 20, 10))[0].clone();
        assert!(bar.contains(" 4 ") && bar.trim_end().ends_with('›'), "#infra, shown, stays; others go: {bar}");
        assert!(bar.contains('+') && bar.contains('≡'), "an action slot never goes: {bar}");
        assert!(!bar.contains(" 3 "), "the last tab other than the one shown went: {bar}");
        let &(rect, _) = a.hits.tabs.iter().find(|(_, i)| *i == 4).expect("its click target is its own tab, not its place on the bar");
        assert!(rect.right() <= 20);
    }

    #[test]
    fn a_long_trailer_is_cut_with_an_ellipsis_at_80_columns() {
        let mut a = described();
        let bar = testkit::rows(&testkit::render(&mut a, 80, 10))[0].clone();
        assert!(["1 merged", "2 #open", "3 #deploy", "4 #infra", "+"].iter().all(|t| bar.contains(t)), "every tab whole: {bar}");
        assert!(bar.contains("— rollout") && bar.trim_end().ends_with('…'), "the trailer is cut, not clipped: {bar}");
        assert_eq!(bar.chars().count(), 80);
        assert_eq!(a.hits.tabs.len(), 6);
        assert!(a.hits.tabs.iter().all(|(r, _)| r.right() <= 80 && r.width > 0), "{:?}", a.hits.tabs);
    }

    #[test]
    fn a_narrow_bar_drops_the_trailer_then_cuts_names_before_any_tab_goes() {
        let mut a = described();
        let bar = testkit::rows(&testkit::render(&mut a, 50, 10))[0].clone();
        assert!(!bar.contains("rollout"), "the trailer goes first: {bar}");
        assert!((1..=4).all(|n| bar.contains(&format!(" {n} "))) && bar.contains('+'), "every tab keeps its number: {bar}");
        assert!(bar.contains('…') && !bar.contains('›'), "names are cut: {bar}");
        let bar = testkit::rows(&testkit::render(&mut a, 20, 10))[0].clone();
        assert!(bar.trim_end().ends_with('›'), "the tabs left off are marked: {bar}");
        assert!(a.hits.tabs.len() < 6, "only the tabs drawn are click targets");
    }

    #[test]
    fn the_key_help_lists_every_binding() {
        let mut a = app(false);
        a.key(k(KeyCode::Char('?'), KeyModifiers::NONE));
        let text = testkit::text(&testkit::render(&mut a, 90, 20));
        for b in a.bindings() {
            assert!(text.contains(&b.keys) && text.contains(&b.label), "{}: {text}", b.keys);
        }
        assert_eq!(crate::binding_conflicts(&a.bindings()), Vec::<String>::new());
    }
}
