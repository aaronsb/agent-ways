//! One dialog for the boxes the shell draws (ADR-504, the note on one
//! `Dialog`): a title, a body of lines scrolled when it outgrows the frame,
//! and a row of buttons. Drawing it records its rect and each button's, and
//! its keys and clicks come back as an [`Answer`], so the code that opened
//! it acts on what was answered and never on a synthesised key.

use std::borrow::Cow;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Clear, Padding, Paragraph};
use ratatui::Frame;

use super::render::{button_row, modal_rect, pane};
use super::theme::{self, Ground, Shape};
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
    /// Answered by its keys alone, with no buttons: Esc and Enter close it.
    keys_only: bool,
    /// Plain keys that close it.
    close_keys: &'static [char],
    /// Home, End, `g` and `G` jump to the top and the last page.
    jumps: bool,
    forward: Option<fn(&KeyEvent) -> bool>,
}

/// Where a dialog was drawn: its rect, each button's, and its scroll held
/// to what the frame shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Drawn {
    pub(super) rect: Rect,
    pub(super) buttons: Vec<(Rect, Btn)>,
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
            keys_only: false,
            close_keys: &[],
            jumps: false,
            forward: None,
        })
    }

    /// The first body row in view.
    pub(super) fn scroll(&self) -> usize {
        self.scroll
    }

    /// Whether a press outside it closes it.
    pub(super) fn closes_outside(&self) -> bool {
        self.close_outside
    }

    /// The button Enter presses, which has the focus.
    fn default_button(&self) -> Option<&Button> {
        self.buttons.iter().find(|b| b.default)
    }

    /// One key. A button's key, Enter on the default and Esc for the cancel
    /// answer it, with no Ctrl or Alt held, and so does a chord the button
    /// names; a dialog answered by its keys alone closes on Esc and Enter,
    /// and any dialog on one of its close keys. A key the dialog forwards
    /// comes back as it came. The arrows, the page keys and `k j` scroll
    /// the body, and so do Home, End and `g G` in a dialog that
    /// [`DialogBuilder::jumps`]. Any other key closes a close-on-any-key
    /// dialog and stays on any other.
    pub(super) fn key(&mut self, k: KeyEvent) -> Answer {
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
            _ if !plain => {}
            KeyCode::Esc => return self.buttons.iter().find(|b| b.cancel).map_or(Answer::Close, |b| Answer::Button(b.id)),
            KeyCode::Enter => return self.default_button().map_or(Answer::Close, |b| Answer::Button(b.id)),
            _ => {}
        }
        if self.forward.is_some_and(|f| f(&k)) {
            return Answer::Forward(k);
        }
        let to = match k.code {
            KeyCode::Up => Some(self.scroll.saturating_sub(1)),
            KeyCode::Down => Some(self.scroll.saturating_add(1)),
            KeyCode::Char('k') if plain => Some(self.scroll.saturating_sub(1)),
            KeyCode::Char('j') if plain => Some(self.scroll.saturating_add(1)),
            KeyCode::PageUp => Some(self.scroll.saturating_sub(PAGE)),
            KeyCode::PageDown => Some(self.scroll.saturating_add(PAGE)),
            KeyCode::Home if self.jumps => Some(0),
            KeyCode::End if self.jumps => Some(usize::MAX),
            KeyCode::Char('g') if plain && self.jumps => Some(0),
            KeyCode::Char('G') if plain && self.jumps => Some(usize::MAX),
            _ => None,
        };
        match to {
            Some(to) => {
                self.scroll = to;
                Answer::Stay
            }
            None if self.close_on_any_key => Answer::Close,
            None => Answer::Stay,
        }
    }

    /// A press or the wheel at `at`, against where the dialog was last
    /// `drawn`. A left click on a button answers with it. The wheel over the
    /// dialog scrolls it. A press inside it closes a close-on-any-key dialog,
    /// and one outside closes a dialog that a click outside closes; every
    /// other press stays. The wheel outside is the caller's to route.
    pub(super) fn mouse(&mut self, kind: MouseEventKind, at: Position, drawn: &Drawn) -> Answer {
        let inside = drawn.contains(at);
        match kind {
            MouseEventKind::ScrollUp if inside => self.key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
            MouseEventKind::ScrollDown if inside => self.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            MouseEventKind::Down(button) => {
                if let Some(&(_, id)) = drawn.buttons.iter().find(|(r, _)| button == MouseButton::Left && r.contains(at)) {
                    return Answer::Button(id);
                }
                let closes = if inside { self.close_on_any_key } else { self.close_outside };
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
        let h = self.height.unwrap_or_else(|| (self.body.len().min(usize::from(u16::MAX - 3)) as u16) + 2 + row);
        modal_rect(area, self.width, h)
    }

    /// Draw it over `area`: the body in view, then the buttons on the last
    /// row inside the border, the default focused.
    pub(super) fn draw(&self, f: &mut Frame, area: Rect, sh: Shape) -> Drawn {
        let r = self.rect(area);
        let border = self.border.unwrap_or_else(theme::modal_border);
        let mut block = pane(self.title.clone()).border_style(border).padding(Padding::horizontal(self.padding));
        if let Some(s) = self.title_style {
            block = block.title_style(s);
        }
        let inner = block.inner(r);
        let row = u16::from(!self.buttons.is_empty() && inner.height > 0);
        let room = usize::from(inner.height - row);
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
        Drawn { rect: r, buttons, scroll, max }
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
    #[allow(dead_code)] // the bar's questions forward their keys next
    pub(super) fn forward(mut self, f: fn(&KeyEvent) -> bool) -> Self {
        self.0.forward = Some(f);
        self
    }

    /// Plain keys that close it, beside Esc and Enter.
    pub(super) fn close_keys(mut self, keys: &'static [char]) -> Self {
        self.0.close_keys = keys;
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
        if d.buttons.is_empty() && !d.keys_only {
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
