//! One dialog for the boxes and the bar questions the shell draws (ADR-504,
//! the note on one `Dialog`): a title, a body of lines scrolled when it
//! outgrows the frame, and a row of buttons, in a box or on the bottom bar.
//! Drawing it records its rect and each button's, and its keys and clicks
//! come back as an [`Answer`], so the code that opened it acts on what was
//! answered and never on a synthesised key.

use std::borrow::Cow;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Padding, Paragraph};
use ratatui::Frame;

use super::pick::{docked, docked_box, Dock};
use super::render::{button_row, clip, modal_rect, pane, width};
use crate::wrap::pad_to;
use super::theme::{self, Ground, Seg, Shape};
use crate::wrap::str_width;
use super::Btn;

/// The rows a page key scrolls.
const PAGE: usize = 10;

/// What a key or a click on a dialog comes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Answer {
    /// A button: its hotkey, Enter on the default, Esc for the cancel, or a
    /// click on it.
    Button(Btn),
    /// A key a close-on-any-key dialog has no use for, a click inside one,
    /// or a click outside a dialog such a click closes.
    Close,
    /// A key the dialog used itself, such as a scroll, or one that is no
    /// answer. Nothing under the dialog receives it.
    Stay,
    /// A key the dialog hands to the code that opened it. Nothing under the
    /// dialog receives it either.
    Forward(KeyEvent),
    /// A menu's row picked: Enter on it, its letter, Space on a menu whose
    /// Space picks, or a click on it.
    Item(usize),
    /// A list's row marked or unmarked: Space on it, or a click on it.
    Mark(usize),
}

/// What Space does in a menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Space {
    /// Nothing beyond what a letter would.
    Letter,
    /// Picks the row, as Enter does.
    Picks,
    /// Marks the row of a list; Enter then sets the marks.
    Marks,
}

/// What a menu's keys do beyond moving and Enter: an item's letter picks
/// it, Space picks or marks, and a row that cannot be picked is drawn set
/// back, passed over, and takes neither Space nor, unless the menu marks,
/// Enter.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct MenuKeys {
    /// Each row's letter, if it has one.
    pub(super) letters: Vec<Option<char>>,
    pub(super) space: Space,
    /// Per row: it cannot be picked yet.
    pub(super) waits: Vec<bool>,
}

impl Default for MenuKeys {
    fn default() -> Self {
        MenuKeys { letters: Vec::new(), space: Space::Letter, waits: Vec::new() }
    }
}

/// A dialog's body as a menu of (label, tag) rows, one under the cursor.
#[derive(Debug, Clone, PartialEq)]
struct Menu {
    items: Vec<(String, String)>,
    sel: usize,
    keys: MenuKeys,
}

impl Menu {
    /// Whether row `i` can be picked.
    fn open(&self, i: usize) -> bool {
        !self.keys.waits.get(i).copied().unwrap_or(false)
    }

    /// The row a move from `from` lands on: the next one that can be
    /// picked that way, or `from` when there is none.
    fn step(&self, from: usize, down: bool) -> usize {
        let mut i = from;
        loop {
            i = match (down, i) {
                (true, i) if i + 1 < self.items.len() => i + 1,
                (false, i) if i > 0 => i - 1,
                _ => return from,
            };
            if self.open(i) {
                return i;
            }
        }
    }

    /// The first row that can be picked, from the top or the bottom.
    fn end(&self, top: bool) -> usize {
        let n = self.items.len();
        let found = if top { (0..n).find(|i| self.open(*i)) } else { (0..n).rev().find(|i| self.open(*i)) };
        found.unwrap_or(self.sel)
    }

    /// Enter, or Space where it picks, on the row under the cursor.
    fn pick(&self) -> Answer {
        match self.open(self.sel) || self.keys.space == Space::Marks {
            true => Answer::Item(self.sel),
            false => Answer::Stay,
        }
    }

    /// One key. Esc and the dialog's close keys close it, with any
    /// modifier held, as the menus always took them. The arrows and `k j`
    /// move over the rows that can be picked, and so do Home, End and `g G`
    /// in a menu that jumps. Any other plain character is a row's letter.
    fn key(&mut self, k: KeyEvent, close: &[char], jumps: bool) -> Answer {
        let plain = !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        match k.code {
            KeyCode::Esc => return Answer::Close,
            KeyCode::Char(c) if close.contains(&c) => return Answer::Close,
            KeyCode::Up | KeyCode::Char('k') => self.sel = self.step(self.sel, false),
            KeyCode::Down | KeyCode::Char('j') => self.sel = self.step(self.sel, true),
            KeyCode::Home | KeyCode::Char('g') if jumps => self.sel = self.end(true),
            KeyCode::End | KeyCode::Char('G') if jumps => self.sel = self.end(false),
            KeyCode::Enter => return self.pick(),
            KeyCode::Char(' ') if self.keys.space != Space::Letter => {
                return match self.keys.space {
                    _ if !self.open(self.sel) => Answer::Stay,
                    Space::Marks => Answer::Mark(self.sel),
                    _ => Answer::Item(self.sel),
                };
            }
            KeyCode::Char(c) if plain => {
                if let Some(i) = self.keys.letters.iter().position(|l| *l == Some(c)) {
                    return Answer::Item(i);
                }
            }
            _ => {}
        }
        Answer::Stay
    }

    /// A left click on row `i`: one that can be picked is marked on a list
    /// and picked elsewhere; one that cannot takes nothing.
    fn click(&mut self, i: usize) -> Answer {
        if !self.open(i) {
            return Answer::Stay;
        }
        self.sel = i;
        if self.keys.space == Space::Marks {
            Answer::Mark(i)
        } else {
            Answer::Item(i)
        }
    }

    /// The rows, each padded to the inside of a box `w` wide so the
    /// selection fills its row; one that cannot be picked is set back.
    fn lines(&self, w: u16) -> Vec<Line<'static>> {
        self.items
            .iter()
            .enumerate()
            .map(|(i, (label, tag))| {
                let style = match (i == self.sel, !self.open(i)) {
                    (true, _) => theme::picked(),
                    (false, true) => theme::muted(),
                    (false, false) => Style::new(),
                };
                Line::styled(pad_to(&format!(" {}{tag}", pad_to(label, 10)), w.saturating_sub(2) as usize), style)
            })
            .collect()
    }
}

/// One button: what it answers, its label, the keys that press it (the one
/// its label names first), and its ground.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Button {
    pub(super) id: Btn,
    pub(super) label: String,
    pub(super) keys: Vec<char>,
    /// Chords that press it, each with exactly its modifiers held.
    chords: Vec<(KeyModifiers, char)>,
    pub(super) ground: Ground,
    cancel: bool,
    default: bool,
}

impl Button {
    pub(super) fn new(id: Btn, label: impl Into<String>, ground: Ground) -> Self {
        Button { id, label: label.into(), keys: Vec::new(), chords: Vec::new(), ground, cancel: false, default: false }
    }

    /// A key that presses it, with no Ctrl or Alt held.
    pub(super) fn key(mut self, c: char) -> Self {
        self.keys.push(c);
        self
    }

    /// A chord that presses it: `c` with exactly `held`. A chord no button
    /// names presses nothing.
    pub(super) fn chord(mut self, held: KeyModifiers, c: char) -> Self {
        self.chords.push((held, c));
        self
    }

    /// Esc presses it.
    pub(super) fn cancel(mut self) -> Self {
        self.cancel = true;
        self
    }

    /// It has the focus and Enter presses it.
    pub(super) fn default(mut self) -> Self {
        self.default = true;
        self
    }
}

/// Where a dialog is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Placement {
    /// A box in the middle of the area, as [`modal_rect`] places one.
    Center,
    /// A box beside the row that opened it, as [`docked`] places a menu.
    Dock(Dock),
    /// The bottom bar: the title is the question, after a `confirm`
    /// lozenge, and the buttons are answer lozenges after it.
    Bar,
}

/// Why a dialog was refused where it was built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Refused {
    /// Buttons with no cancel, or with more than one: how many.
    Cancel(usize),
    /// Buttons with no default, or with more than one: how many.
    Default(usize),
    /// No buttons, on a dialog not built to be answered by its keys alone
    /// ([`DialogBuilder::keys_only`]).
    NoButtons,
}

/// A dialog being built ([`Dialog::new`]); [`DialogBuilder::build`] checks
/// its buttons.
pub(super) struct DialogBuilder<'a>(Dialog<'a>);

/// A dialog ready to draw and to answer keys and clicks.
pub(super) struct Dialog<'a> {
    placement: Placement,
    title: String,
    body: Cow<'a, [Line<'static>]>,
    buttons: Vec<Button>,
    width: u16,
    height: Option<u16>,
    border: Option<Style>,
    title_style: Option<Style>,
    padding: u16,
    /// The first body row in view. A scroll key may take it past the last
    /// page; the draw holds it there ([`Drawn::scroll`]).
    scroll: usize,
    close_on_any_key: bool,
    close_outside: bool,
    /// Its body as a menu of rows instead of lines.
    menu: Option<Menu>,
    /// Answered by its keys alone, with no buttons: Esc and Enter close it.
    keys_only: bool,
    /// Plain keys that close it.
    close_keys: &'static [char],
    /// A press of any mouse button that lands on no button closes it, inside
    /// it or outside.
    any_press: bool,
    /// Home, End, `g` and `G` jump to the top and the last page.
    jumps: bool,
    forward: Option<fn(&KeyEvent) -> bool>,
}

/// Where a dialog was drawn: its rect, each button's, and its scroll held
/// to what the frame shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Drawn {
    pub(super) rect: Rect,
    /// Inside the border, above the button row: where a body the caller
    /// draws goes.
    pub(super) body: Rect,
    pub(super) buttons: Vec<(Rect, Btn)>,
    /// A menu's rows, in order.
    pub(super) rows: Vec<Rect>,
    pub(super) scroll: usize,
    /// The last scroll that still fills the frame.
    pub(super) max: usize,
}

impl Drawn {
    pub(super) fn contains(&self, at: Position) -> bool {
        self.rect.contains(at)
    }
}

impl<'a> Dialog<'a> {
    /// A dialog titled `title`, `width` columns wide, centred, with an
    /// empty body and no buttons.
    #[allow(clippy::new_ret_no_self)]
    pub(super) fn new(title: impl Into<String>, width: u16) -> DialogBuilder<'a> {
        DialogBuilder(Dialog {
            placement: Placement::Center,
            title: title.into(),
            body: Cow::Owned(Vec::new()),
            buttons: Vec::new(),
            width,
            height: None,
            border: None,
            title_style: None,
            padding: 0,
            scroll: 0,
            close_on_any_key: false,
            close_outside: false,
            menu: None,
            keys_only: false,
            close_keys: &[],
            any_press: false,
            jumps: false,
            forward: None,
        })
    }

    /// A question on the bottom bar: its answers are the buttons.
    pub(super) fn bar(question: impl Into<String>) -> DialogBuilder<'a> {
        let mut d = Dialog::new(question, 0);
        d.0.placement = Placement::Bar;
        d
    }

    pub(super) fn placement(&self) -> Placement {
        self.placement
    }

    /// The first body row in view.
    pub(super) fn scroll(&self) -> usize {
        self.scroll
    }

    /// A menu's row under the cursor.
    pub(super) fn sel(&self) -> Option<usize> {
        self.menu.as_ref().map(|m| m.sel)
    }

    /// Whether a press outside it closes it.
    pub(super) fn closes_outside(&self) -> bool {
        self.close_outside || self.any_press
    }

    /// Whether a press of any mouse button that lands on no button closes it.
    pub(super) fn any_press(&self) -> bool {
        self.any_press
    }

    /// The button Enter presses, which has the focus.
    fn default_button(&self) -> Option<&Button> {
        self.buttons.iter().find(|b| b.default)
    }

    /// One key. A button's key and Enter on the default answer it, with no
    /// Ctrl or Alt held, and so do a chord the button names and Esc for the
    /// cancel with any modifier; a dialog answered by its keys alone closes
    /// on Esc and Enter,
    /// and any dialog on one of its close keys. A key the dialog forwards
    /// comes back as it came. The arrows, the page keys and `k j` scroll
    /// the body, and so do Home, End and `g G` in a dialog that
    /// [`DialogBuilder::jumps`]. Any other key closes a close-on-any-key
    /// dialog and stays on any other.
    pub(super) fn key(&mut self, k: KeyEvent) -> Answer {
        if let Some(m) = &mut self.menu {
            return m.key(k, self.close_keys, self.jumps);
        }
        let plain = !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        if let KeyCode::Char(c) = k.code {
            let chord = |b: &&Button| b.chords.iter().any(|&(held, at)| held == k.modifiers && at == c);
            if let Some(b) = self.buttons.iter().find(|b| (plain && b.keys.contains(&c)) || chord(b)) {
                return Answer::Button(b.id);
            }
            if plain && self.close_keys.contains(&c) {
                return Answer::Close;
            }
        }
        match k.code {
            KeyCode::Esc => return self.buttons.iter().find(|b| b.cancel).map_or(Answer::Close, |b| Answer::Button(b.id)),
            KeyCode::Enter if plain => return self.default_button().map_or(Answer::Close, |b| Answer::Button(b.id)),
            _ => {}
        }
        if self.forward.is_some_and(|f| f(&k)) {
            return Answer::Forward(k);
        }
        let by = match k.code {
            KeyCode::Up => Some(-1),
            KeyCode::Down => Some(1),
            KeyCode::Char('k') if plain => Some(-1),
            KeyCode::Char('j') if plain => Some(1),
            KeyCode::PageUp => Some(-(PAGE as isize)),
            KeyCode::PageDown => Some(PAGE as isize),
            KeyCode::Home if self.jumps => Some(isize::MIN),
            KeyCode::End if self.jumps => Some(isize::MAX),
            KeyCode::Char('g') if plain && self.jumps => Some(isize::MIN),
            KeyCode::Char('G') if plain && self.jumps => Some(isize::MAX),
            _ => None,
        };
        match by {
            Some(by) => self.scroll_by(by),
            None if self.close_on_any_key => Answer::Close,
            None => Answer::Stay,
        }
    }

    /// Scroll the body `by` rows, down when positive; a bar question has
    /// none to scroll, and the wheel over a menu moves its cursor.
    fn scroll_by(&mut self, by: isize) -> Answer {
        if let Some(m) = &mut self.menu {
            m.sel = m.step(m.sel, by > 0);
        } else if self.placement != Placement::Bar {
            self.scroll = self.scroll.saturating_add_signed(by);
        }
        Answer::Stay
    }

    /// A press or the wheel at `at`, against where the dialog was last
    /// `drawn`. A left click on a button answers with it. The wheel over the
    /// dialog scrolls it. A press inside it closes a close-on-any-key dialog,
    /// one outside closes a dialog that a click outside closes, and any
    /// press closes one that [`DialogBuilder::any_press`]; every other
    /// press stays. The wheel outside is the caller's to route.
    pub(super) fn mouse(&mut self, kind: MouseEventKind, at: Position, drawn: &Drawn) -> Answer {
        let inside = drawn.contains(at);
        match kind {
            MouseEventKind::ScrollUp if inside => self.scroll_by(-1),
            MouseEventKind::ScrollDown if inside => self.scroll_by(1),
            MouseEventKind::Down(button) => {
                if let Some(&(_, id)) = drawn.buttons.iter().find(|(r, _)| button == MouseButton::Left && r.contains(at)) {
                    return Answer::Button(id);
                }
                if let (Some(m), Some(i)) = (&mut self.menu, drawn.rows.iter().position(|r| button == MouseButton::Left && r.contains(at))) {
                    return m.click(i);
                }
                let closes = self.any_press || if inside { self.close_on_any_key } else { self.close_outside };
                if closes {
                    Answer::Close
                } else {
                    Answer::Stay
                }
            }
            _ => Answer::Stay,
        }
    }

    /// Its rect over `area`, before it is drawn: a box in the middle, as tall
    /// as its body and button row with the borders, unless it was given a
    /// height.
    pub(super) fn rect(&self, area: Rect) -> Rect {
        let row = u16::from(!self.buttons.is_empty());
        let lines = self.menu.as_ref().map_or(self.body.len(), |m| m.items.len());
        let h = self.height.unwrap_or_else(|| (lines.min(usize::from(u16::MAX - 3)) as u16) + 2 + row);
        match (self.placement, &self.menu) {
            (Placement::Dock(dock), Some(m)) => docked(area, dock, &self.title, &m.items),
            // As wide inside as it is wide less its borders.
            (Placement::Dock(dock), None) => docked_box(area, dock, &self.title, usize::from(self.width.saturating_sub(2)), usize::from(h.saturating_sub(2))),
            _ => modal_rect(area, self.width, h),
        }
    }

    /// Draw it over `area`: in a box, the body in view, then the buttons on
    /// the last row inside the border, the default focused; on the bar, the
    /// question and its answers along `area`'s first row.
    pub(super) fn draw(&self, f: &mut Frame, area: Rect, sh: Shape) -> Drawn {
        if self.placement == Placement::Bar {
            return self.draw_bar(f, Rect { height: area.height.min(1), ..area }, sh);
        }
        let r = self.rect(area);
        let border = self.border.unwrap_or_else(theme::modal_border);
        let mut block = pane(self.title.clone()).border_style(border).padding(Padding::horizontal(self.padding));
        if let Some(s) = self.title_style {
            block = block.title_style(s);
        }
        let inner = block.inner(r);
        let row = u16::from(!self.buttons.is_empty() && inner.height > 0);
        let room = usize::from(inner.height - row);
        let body = Rect { height: inner.height - row, ..inner };
        if let Some(m) = &self.menu {
            let lines = m.lines(r.width);
            let rows = (0..lines.len() as u16).map(|i| Rect { y: inner.y + i, height: 1, ..inner }.intersection(inner)).collect();
            f.render_widget(Clear, r);
            f.render_widget(Paragraph::new(lines).block(block), r);
            return Drawn { rect: r, body, rows, ..Drawn::default() };
        }
        let max = self.body.len().saturating_sub(room);
        let scroll = self.scroll.min(max);
        let shown = self.body[scroll..self.body.len().min(scroll + room)].to_vec();
        f.render_widget(Clear, r);
        f.render_widget(Paragraph::new(shown).block(block), r);
        let mut buttons = Vec::new();
        if row == 1 {
            let at = Rect { y: inner.bottom() - 1, height: 1, ..inner };
            let row: Vec<(Btn, String, Ground)> = self.buttons.iter().map(|b| (b.id, b.label.clone(), b.ground)).collect();
            let focus = self.default_button().map(|b| b.id);
            f.render_widget(Paragraph::new(button_row(sh, at, focus, &row, &mut buttons)), at);
        }
        Drawn { rect: r, body, buttons, rows: Vec::new(), scroll, max }
    }
}

impl Dialog<'_> {
    /// The bar: a `confirm` lozenge, the question, then the answers as
    /// lozenges, each a click target. The question is cut to leave the
    /// answers on screen, and the lozenge goes first when the question
    /// would not fit beside it. No answer is drawn focused.
    fn draw_bar(&self, f: &mut Frame, area: Rect, sh: Shape) -> Drawn {
        let destructive = self.buttons.iter().any(|b| b.ground == Ground::Err);
        let answers: Vec<(Btn, Vec<Span<'static>>)> =
            self.buttons.iter().map(|b| (b.id, sh.lozenge(&[Seg::on(format!(" {} ", b.label), b.ground.on_row(destructive)).bold()]))).collect();
        // Each answer is followed by a space.
        let after = answers.iter().map(|(_, a)| width(a) + 1).sum::<u16>();
        let lozenge = sh.lozenge(&[Seg::on(" confirm ", Ground::Warn).bold()]);
        let room = |used: u16| usize::from(area.width.saturating_sub(used + after + 3));
        let mut spans = Vec::new();
        if str_width(&self.title) <= room(width(&lozenge)) {
            spans.extend(lozenge);
        }
        let question = clip(&self.title, room(width(&spans)));
        spans.push(Span::raw(format!(" {question}  ")));
        let mut buttons = Vec::new();
        for (id, target) in answers {
            let x = area.x + width(&spans);
            buttons.push((Rect { x, y: area.y, width: width(&target), height: 1 }.intersection(area), id));
            spans.extend(target);
            spans.push(Span::raw(" "));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), area);
        Drawn { rect: area, buttons, ..Drawn::default() }
    }
}

impl<'a> DialogBuilder<'a> {
    /// The body's lines, owned or borrowed.
    pub(super) fn body(mut self, lines: impl Into<Cow<'a, [Line<'static>]>>) -> Self {
        self.0.body = lines.into();
        self
    }

    pub(super) fn button(mut self, b: Button) -> Self {
        self.0.buttons.push(b);
        self
    }

    /// Rows, borders included, in place of the body's own height.
    pub(super) fn height(mut self, h: u16) -> Self {
        self.0.height = Some(h);
        self
    }

    /// A border in place of the modal border.
    pub(super) fn border(mut self, s: Style) -> Self {
        self.0.border = Some(s);
        self
    }

    pub(super) fn title_style(mut self, s: Style) -> Self {
        self.0.title_style = Some(s);
        self
    }

    /// Columns of space inside the border, each side.
    pub(super) fn padding(mut self, n: u16) -> Self {
        self.0.padding = n;
        self
    }

    /// The first body row in view, as the last draw held it.
    pub(super) fn scroll(mut self, scroll: usize) -> Self {
        self.0.scroll = scroll;
        self
    }

    /// A key it has no use for closes it, and so does a press inside it.
    pub(super) fn close_on_any_key(mut self) -> Self {
        self.0.close_on_any_key = true;
        self
    }

    /// Home and `g` jump to the top, End and `G` to the last page.
    pub(super) fn jumps(mut self) -> Self {
        self.0.jumps = true;
        self
    }

    /// A press outside it closes it.
    pub(super) fn close_outside(mut self) -> Self {
        self.0.close_outside = true;
        self
    }

    /// Keys `f` picks go back to the code that opened it.
    pub(super) fn forward(mut self, f: fn(&KeyEvent) -> bool) -> Self {
        self.0.forward = Some(f);
        self
    }

    /// A menu of (label, tag) rows as its body, `sel` under the cursor, its
    /// keys set by `keys`. A menu is answered by its keys and clicks.
    pub(super) fn menu(mut self, items: Vec<(String, String)>, sel: usize, keys: MenuKeys) -> Self {
        self.0.menu = Some(Menu { items, sel, keys });
        self
    }

    /// Placed beside the row that opened it, or in the middle with none.
    pub(super) fn dock(mut self, dock: Option<Dock>) -> Self {
        if let Some(dock) = dock {
            self.0.placement = Placement::Dock(dock);
        }
        self
    }

    /// Plain keys that close it, beside Esc and Enter.
    pub(super) fn close_keys(mut self, keys: &'static [char]) -> Self {
        self.0.close_keys = keys;
        self
    }

    /// A press of any mouse button that lands on no button closes it,
    /// inside it or outside.
    pub(super) fn any_press(mut self) -> Self {
        self.0.any_press = true;
        self
    }

    /// It has no buttons and is answered by its keys alone: Esc and Enter
    /// close it.
    pub(super) fn keys_only(mut self) -> Self {
        self.0.keys_only = true;
        self
    }

    /// The dialog, or why it is refused: a dialog with buttons has exactly
    /// one cancel and exactly one default, which may be one button. A
    /// dialog with none is refused unless it is built answered by its keys
    /// alone.
    pub(super) fn build(self) -> Result<Dialog<'a>, Refused> {
        let d = self.0;
        if d.buttons.is_empty() && !d.keys_only && d.menu.is_none() {
            return Err(Refused::NoButtons);
        }
        if !d.buttons.is_empty() {
            let cancels = d.buttons.iter().filter(|b| b.cancel).count();
            if cancels != 1 {
                return Err(Refused::Cancel(cancels));
            }
            let defaults = d.buttons.iter().filter(|b| b.default).count();
            if defaults != 1 {
                return Err(Refused::Default(defaults));
            }
        }
        Ok(d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::Terminal;

    /// One frame of `draw` at `w` by `h`.
    fn draw_with(w: u16, h: u16, draw: impl FnOnce(&mut Frame)) -> Buffer {
        let mut term = Terminal::new(TestBackend::new(w, h)).expect("a test backend");
        term.draw(draw).expect("a frame");
        term.backend().buffer().clone()
    }

    fn press(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    /// Back (cancel and default), Review on `r`, Quit on `D`.
    fn guard() -> Dialog<'static> {
        Dialog::new("quit", 60)
            .body(vec![Line::raw("one"), Line::raw("two")])
            .button(Button::new(Btn::Back, "Back (Esc)", Ground::Keep).key('b').cancel().default())
            .button(Button::new(Btn::Review, "Review (r)", Ground::Accent).key('r'))
            .button(Button::new(Btn::Quit, "Quit (D)", Ground::Err).key('D'))
            .build()
            .expect("one cancel, one default")
    }

    /// Draw `d` alone on a `w` by `h` screen.
    fn drawn(d: &Dialog, w: u16, h: u16) -> (Drawn, Vec<String>) {
        let mut out = Drawn::default();
        let buf = draw_with(w, h, |f| out = d.draw(f, f.area(), Shape::PLAIN));
        (out, testkit::rows(&buf))
    }

    fn click(d: &mut Dialog, drawn: &Drawn, x: u16, y: u16) -> Answer {
        d.mouse(MouseEventKind::Down(MouseButton::Left), Position::new(x, y), drawn)
    }

    #[test]
    fn buttons_without_exactly_one_cancel_and_one_default_are_refused() {
        let b = |id| Button::new(id, "x", Ground::Accent);
        let none = Dialog::new("t", 40).button(b(Btn::Save).default()).build();
        assert_eq!(none.err(), Some(Refused::Cancel(0)));
        let two = Dialog::new("t", 40).button(b(Btn::Save).cancel().default()).button(b(Btn::Back).cancel()).build();
        assert_eq!(two.err(), Some(Refused::Cancel(2)));
        let no_default = Dialog::new("t", 40).button(b(Btn::Back).cancel()).build();
        assert_eq!(no_default.err(), Some(Refused::Default(0)));
        let two_defaults = Dialog::new("t", 40).button(b(Btn::Back).cancel().default()).button(b(Btn::Save).default()).build();
        assert_eq!(two_defaults.err(), Some(Refused::Default(2)));
        assert!(Dialog::new("t", 40).button(b(Btn::Back).cancel()).button(b(Btn::Save).default()).build().is_ok());
        assert_eq!(Dialog::new("t", 40).build().err(), Some(Refused::NoButtons), "no buttons, and not built to be answered by its keys");
        let mut keys = Dialog::new("t", 40).keys_only().build().expect("answered by its keys alone");
        assert_eq!((keys.key(press(KeyCode::Esc)), keys.key(press(KeyCode::Enter))), (Answer::Close, Answer::Close));
    }

    #[test]
    fn a_hotkey_and_a_click_on_its_button_give_the_same_answer() {
        let mut d = guard();
        let (at, _) = drawn(&d, 80, 20);
        for (c, id) in [('b', Btn::Back), ('r', Btn::Review), ('D', Btn::Quit)] {
            assert_eq!(d.key(press(KeyCode::Char(c))), Answer::Button(id), "{c}");
            let (r, _) = at.buttons.iter().find(|(_, b)| *b == id).copied().expect("each button is drawn");
            assert_eq!(click(&mut d, &at, r.x + 1, r.y), Answer::Button(id), "a click on {id:?}");
        }
        assert_eq!(d.key(press(KeyCode::Esc)), Answer::Button(Btn::Back), "Esc presses the cancel");
        assert_eq!(d.key(press(KeyCode::Enter)), Answer::Button(Btn::Back), "Enter presses the default");
    }

    #[test]
    fn a_key_that_is_no_answer_stays_and_a_chord_never_presses_a_button() {
        let mut d = guard();
        assert_eq!(d.key(press(KeyCode::Char('x'))), Answer::Stay);
        assert_eq!(d.key(press(KeyCode::Char('d'))), Answer::Stay, "a hotkey is matched exactly");
        assert_eq!(d.key(KeyEvent::new(KeyCode::Char('D'), KeyModifiers::ALT)), Answer::Stay);
        let (at, _) = drawn(&d, 80, 20);
        assert_eq!(click(&mut d, &at, at.rect.x + 2, at.rect.y + 1), Answer::Stay, "a click inside, off the buttons");
        assert_eq!(click(&mut d, &at, 0, 0), Answer::Stay, "a click outside a dialog that it does not close");
        let (r, _) = at.buttons[2];
        assert_eq!(d.mouse(MouseEventKind::Down(MouseButton::Right), Position::new(r.x + 1, r.y), &at), Answer::Stay, "a right click presses nothing");
    }

    #[test]
    fn a_close_on_any_key_dialog_closes_on_a_key_and_on_a_press_inside_and_scrolls_on_its_own() {
        let lines: Vec<Line<'static>> = (0..40).map(|i| Line::raw(format!("line {i}"))).collect();
        let build = |scroll| Dialog::new("keys", 30).body(lines.clone()).scroll(scroll).jumps().close_on_any_key().close_outside().keys_only().build().unwrap();
        let (at, rows) = drawn(&build(0), 80, 12);
        assert_eq!(at.max, 40 - 10, "40 lines in the 10 rows inside the border");
        assert!(rows.iter().any(|r| r.contains("line 0")) && !rows.iter().any(|r| r.contains("line 10")));
        let mut d = build(0);
        assert_eq!(d.key(press(KeyCode::Down)), Answer::Stay);
        assert_eq!(d.scroll(), 1);
        assert_eq!(d.key(press(KeyCode::End)), Answer::Stay);
        let (held, rows) = drawn(&d, 80, 12);
        assert_eq!(held.scroll, 30, "the draw holds the scroll at the last page");
        assert!(rows.iter().any(|r| r.contains("line 39")));
        let mut d = build(held.scroll);
        assert_eq!(d.mouse(MouseEventKind::ScrollUp, Position::new(at.rect.x + 1, at.rect.y + 1), &at), Answer::Stay);
        assert_eq!(d.scroll(), 29, "the wheel over it scrolls it");
        assert_eq!(d.key(press(KeyCode::Char('x'))), Answer::Close);
        assert_eq!(d.key(press(KeyCode::Esc)), Answer::Close);
        assert_eq!(click(&mut d, &at, at.rect.x + 1, at.rect.y + 1), Answer::Close, "a press inside closes it");
        let mut plain = Dialog::new("keys", 30).body(lines.clone()).close_on_any_key().keys_only().build().unwrap();
        assert_eq!(plain.key(press(KeyCode::End)), Answer::Close, "End closes a dialog that does not jump");
        // A body that fits leaves room outside the box.
        let mut d = Dialog::new("keys", 30).body(lines[..3].to_vec()).close_on_any_key().close_outside().keys_only().build().unwrap();
        let (at, _) = drawn(&d, 80, 12);
        assert!(!at.contains(Position::new(0, 0)));
        assert_eq!(d.mouse(MouseEventKind::ScrollDown, Position::new(0, 0), &at), Answer::Stay);
        assert_eq!(d.scroll(), 0, "the wheel outside it is the caller's");
        assert_eq!(click(&mut d, &at, 0, 0), Answer::Close, "a press outside closes it");
    }

    #[test]
    fn a_forwarded_key_goes_back_to_the_caller_and_a_close_key_closes() {
        let mut d = Dialog::new("r", 30).forward(|k| k.code == KeyCode::Char('f')).close_keys(&['q']).keys_only().build().unwrap();
        let f = press(KeyCode::Char('f'));
        assert_eq!(d.key(f), Answer::Forward(f));
        assert_eq!(d.key(press(KeyCode::Char('q'))), Answer::Close);
        assert_eq!(d.key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::ALT)), Answer::Stay, "a close key is plain");
        assert_eq!(d.key(press(KeyCode::Char('x'))), Answer::Stay);
    }

    #[test]
    fn a_chord_presses_only_the_button_that_names_it() {
        let mut d = Dialog::new("close", 60)
            .button(Button::new(Btn::Save, "Save (s)", Ground::Ok).key('s').chord(KeyModifiers::CONTROL, 's'))
            .button(Button::new(Btn::Drop, "Discard (d)", Ground::Err).key('d'))
            .button(Button::new(Btn::Back, "Back (Esc)", Ground::Keep).cancel().default())
            .build()
            .unwrap();
        assert_eq!(d.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)), Answer::Button(Btn::Save));
        assert_eq!(d.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)), Answer::Stay, "no button names ^D");
        assert_eq!(d.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL | KeyModifiers::ALT)), Answer::Stay, "exactly the chord's modifiers");
    }

    #[test]
    fn the_keep_ground_is_green_only_beside_a_button_on_the_error_ground() {
        let ground_of = |d: &Dialog, id: Btn| {
            let mut out = Drawn::default();
            let buf = draw_with(80, 12, |f| out = d.draw(f, f.area(), Shape::PLAIN));
            let (r, _) = out.buttons.iter().find(|(_, b)| *b == id).copied().unwrap();
            buf[(r.x + 1, r.y)].bg
        };
        theme::set(theme::Palette::default());
        let d = guard();
        assert_eq!(ground_of(&d, Btn::Back), Ground::Ok.bg(), "Back beside Quit");
        let calm = Dialog::new("t", 60)
            .button(Button::new(Btn::Back, "Back", Ground::Keep).cancel())
            .button(Button::new(Btn::Save, "Save", Ground::Ok).default())
            .build()
            .unwrap();
        assert_eq!(ground_of(&calm, Btn::Back), Ground::AccentDim.bg(), "Back beside nothing destructive");
        assert_eq!(ground_of(&d, Btn::Quit), Ground::Err.bg());
    }

    /// A drop question on the bar: drop on the error ground, keep beside it.
    fn bar(q: &str) -> Dialog<'static> {
        Dialog::bar(q)
            .button(Button::new(Btn::Yes, "y drop", Ground::Err).key('y'))
            .button(Button::new(Btn::No, "n keep", Ground::Keep).key('n').cancel().default())
            .build()
            .unwrap()
    }

    #[test]
    fn a_bar_question_draws_on_the_first_row_and_keeps_its_answers_on_screen() {
        theme::set(theme::Palette::default());
        let mut out = Drawn::default();
        let buf = draw_with(60, 3, |f| out = bar("drop it?").draw(f, Rect { y: 2, height: 1, ..f.area() }, Shape::PLAIN));
        let rows = testkit::rows(&buf);
        assert!(rows[2].starts_with(" confirm  drop it?   y drop   n keep "), "{}", rows[2]);
        assert_eq!(out.rect, Rect::new(0, 2, 60, 1));
        let (yes, no) = (out.buttons[0], out.buttons[1]);
        assert_eq!((yes.1, no.1), (Btn::Yes, Btn::No));
        assert_eq!((buf[(yes.0.x + 1, 2)].bg, buf[(no.0.x + 1, 2)].bg), (Ground::Err.bg(), Ground::Ok.bg()), "keep is green beside drop");
        let mut d = bar("drop it?");
        assert_eq!(click(&mut d, &out, no.0.x, 2), Answer::Button(Btn::No));
        assert_eq!(d.key(press(KeyCode::Enter)), Answer::Button(Btn::No), "Enter gives the default");
        // Too narrow for the question: the lozenge goes, then the question is cut.
        let long = "drop every message in this channel and its history for good?";
        let buf = draw_with(40, 1, |f| out = bar(long).draw(f, f.area(), Shape::PLAIN));
        let row = &testkit::rows(&buf)[0];
        assert!(!row.contains("confirm") && row.contains('…') && row.contains(" y drop ") && row.contains(" n keep "), "{row}");
        assert!(out.buttons.iter().all(|(r, _)| r.right() <= 40 && r.width > 0), "the answers stay on screen");
    }

    #[test]
    fn a_dialog_that_any_press_closes_closes_on_any_button_off_its_answers() {
        let mut d = Dialog::bar("q").button(Button::new(Btn::No, "n", Ground::Keep).cancel().default()).any_press().forward(|_| true).build().unwrap();
        let drawn = Drawn { rect: Rect::new(0, 9, 40, 1), buttons: vec![(Rect::new(10, 9, 3, 1), Btn::No)], ..Drawn::default() };
        assert!(d.closes_outside() && d.any_press());
        for (button, x, y) in [(MouseButton::Left, 0, 0), (MouseButton::Right, 1, 9), (MouseButton::Middle, 11, 9)] {
            assert_eq!(d.mouse(MouseEventKind::Down(button), Position::new(x, y), &drawn), Answer::Close, "{button:?} at {x},{y}");
        }
        let x = press(KeyCode::Char('x'));
        assert_eq!(d.key(x), Answer::Forward(x), "every key that is no answer goes back");
        for held in [KeyModifiers::ALT, KeyModifiers::CONTROL] {
            assert_eq!(d.key(KeyEvent::new(KeyCode::Esc, held)), Answer::Button(Btn::No), "Esc with {held:?} is the cancel");
        }
        // The wheel over a bar question scrolls nothing and is no key.
        for kind in [MouseEventKind::ScrollUp, MouseEventKind::ScrollDown] {
            assert_eq!(d.mouse(kind, Position::new(1, 9), &drawn), Answer::Stay, "{kind:?}");
        }
    }

    /// Four rows, the second waiting, `b` the third's letter.
    fn menu(space: Space) -> Dialog<'static> {
        let items = ["one", "two", "three", "four"].map(|l| (l.to_string(), String::new())).to_vec();
        let keys = MenuKeys { letters: vec![None, None, Some('b'), None], space, waits: vec![false, true, false, false] };
        Dialog::new("menu", 30).menu(items, 0, keys).close_keys(&['q']).jumps().close_outside().build().expect("a menu is answered by its keys")
    }

    #[test]
    fn a_menu_moves_over_rows_that_wait_and_picks_by_enter_letter_and_click() {
        let mut d = menu(Space::Picks);
        assert_eq!(d.key(press(KeyCode::Down)), Answer::Stay);
        assert_eq!(d.sel(), Some(2), "the waiting row is passed over");
        assert_eq!(d.key(press(KeyCode::Up)), Answer::Stay);
        assert_eq!(d.sel(), Some(0));
        assert_eq!(d.key(press(KeyCode::End)), Answer::Stay);
        assert_eq!(d.sel(), Some(3));
        assert_eq!(d.key(press(KeyCode::Enter)), Answer::Item(3));
        assert_eq!(d.key(press(KeyCode::Char(' '))), Answer::Item(3), "Space picks where it picks");
        assert_eq!(d.key(press(KeyCode::Char('b'))), Answer::Item(2), "a row's letter picks it");
        assert_eq!(d.key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::ALT)), Answer::Stay, "a letter is plain");
        assert_eq!(d.key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL)), Answer::Close, "a close key with any modifier");
        assert_eq!(d.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::ALT)), Answer::Close);
        let (at, rows) = drawn(&d, 80, 20);
        assert_eq!(at.rows.len(), 4);
        assert!(rows[at.rows[0].y as usize].contains(" one"));
        let row = |i: usize| at.rows[i];
        assert_eq!(click(&mut d, &at, row(2).x + 1, row(2).y), Answer::Item(2));
        assert_eq!(d.sel(), Some(2));
        assert_eq!(click(&mut d, &at, row(1).x + 1, row(1).y), Answer::Stay, "a waiting row takes no click");
        assert_eq!(d.sel(), Some(2));
        assert_eq!(d.mouse(MouseEventKind::ScrollDown, Position::new(row(0).x, row(0).y), &at), Answer::Stay);
        assert_eq!(d.sel(), Some(3), "the wheel over it moves the cursor");
        assert_eq!(click(&mut d, &at, 0, 0), Answer::Close, "a click outside closes it");
    }

    #[test]
    fn a_list_marks_by_space_and_click_and_a_waiting_row_takes_neither() {
        let mut d = menu(Space::Marks);
        assert_eq!(d.key(press(KeyCode::Char(' '))), Answer::Mark(0));
        let (at, _) = drawn(&d, 80, 20);
        assert_eq!(click(&mut d, &at, at.rows[3].x + 1, at.rows[3].y), Answer::Mark(3));
        assert_eq!(d.key(press(KeyCode::Enter)), Answer::Item(3), "Enter sets the marks");
        // Land on the waiting row as the picker can open on it.
        let items = ["one", "two"].map(|l| (l.to_string(), String::new())).to_vec();
        let waits = MenuKeys { letters: Vec::new(), space: Space::Marks, waits: vec![false, true] };
        let mut list = Dialog::new("m", 30).menu(items.clone(), 1, waits).build().unwrap();
        assert_eq!(list.key(press(KeyCode::Char(' '))), Answer::Stay, "no Space on a waiting row");
        assert_eq!(list.key(press(KeyCode::Enter)), Answer::Item(1), "a list's Enter sets the marks as they are");
        let one = MenuKeys { letters: Vec::new(), space: Space::Picks, waits: vec![false, true] };
        let mut one = Dialog::new("m", 30).menu(items, 1, one).build().unwrap();
        assert_eq!((one.key(press(KeyCode::Enter)), one.key(press(KeyCode::Char(' ')))), (Answer::Stay, Answer::Stay), "one choice: a waiting row picks nothing");
        let mut letter = menu(Space::Letter);
        assert_eq!(letter.key(press(KeyCode::Char(' '))), Answer::Stay, "Space is no letter here");
    }

    #[test]
    fn a_docked_menu_sits_where_docked_places_it() {
        let items: Vec<(String, String)> = (0..3).map(|i| (format!("  item {i}"), String::new())).collect();
        let dock = Dock::Under(Rect::new(4, 2, 10, 1));
        let d = Dialog::new("pick one: x", 50).menu(items.clone(), 0, MenuKeys::default()).dock(Some(dock)).build().unwrap();
        assert_eq!(d.placement(), Placement::Dock(dock));
        let area = Rect::new(0, 1, 80, 20);
        assert_eq!(d.rect(area), docked(area, dock, "pick one: x", &items));
        let centred = Dialog::new("pick one: x", 50).menu(items, 0, MenuKeys::default()).dock(None).build().unwrap();
        assert_eq!((centred.placement(), centred.rect(area)), (Placement::Center, modal_rect(area, 50, 5)));
    }

    #[test]
    fn the_button_row_is_the_last_row_inside_the_border_even_when_the_body_overflows() {
        let lines: Vec<Line<'static>> = (0..30).map(|i| Line::raw(format!("tab {i}"))).collect();
        let d = Dialog::new("quit", 60).body(lines).button(Button::new(Btn::Back, "Back (Esc)", Ground::Keep).cancel().default()).build().unwrap();
        let (at, rows) = drawn(&d, 80, 10);
        let (r, _) = at.buttons[0];
        assert_eq!(r.y, at.rect.bottom() - 2, "on the row above the bottom border");
        assert!(rows[r.y as usize].contains("Back (Esc)"));
        assert_eq!(at.max, 30 - 7, "the body scrolls in the rows the buttons leave");
    }
}
