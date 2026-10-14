//! A pane: a screen's content that is not a tree, drawn and driven by the
//! application inside the shell (ADR-504 §3). A chat's feed and compose
//! box is one: content that changes while nobody types, and a text entry
//! that owns most keys.
//!
//! The shell keeps what every screen shares: the tab bar and its clicks,
//! the bottom bar with the footer, the mouse and its capture toggle, the
//! exit guard over unsaved work, the key help and the response modal. The
//! pane draws the area between the bars, handles the keys the shell leaves
//! it, and declares its keys as [`Binding`]s, which the footer and the key
//! help both read.
//!
//! The overlays follow one rule. Esc, or a click outside, closes the one on
//! top (a menu, a picker, a question, the key help, the response modal)
//! and says nothing. A click that closes one on the tab bar, or on a part
//! of the pane that passes clicks through ([`Pane::click_through`]), then
//! does what it would have done with nothing open.
//!
//! A pane that owns text ([`Ctx::text`]) types every plain key, so the
//! shell's own keys take their Alt form there: Alt+1-9 for a tab, Alt+m for
//! the mouse. F1 opens the key help, and Esc and Ctrl-C quit, asking first
//! when the pane holds unsaved work.
//!
//! Every pane's tabs answer to the same keys and clicks: Ctrl+1-9 shows a
//! tab, and on the tab already shown opens its menu ([`Pane::tab_menu`]);
//! F2 or Ctrl+T moves the focus to the tab bar, where Left and Right move,
//! Enter opens the menu and Esc goes back; a click on a tab shows it, a
//! second click or a right click opens its menu. Ctrl+digits reach the
//! screen as themselves only under the kitty keyboard protocol, which the
//! shell turns on for a pane that asks ([`Pane::keyboard_enhancement`]).

use std::any::Any;
use std::time::Duration;

use ratatui::layout::{Position, Rect};
use ratatui::text::Span;
use ratatui::Frame;
use ratatui::crossterm::event::KeyEvent;

use super::theme::{self, Palette};

/// One tab of a pane's tab bar. The shell numbers it and draws it as a
/// lozenge in the shell's look.
#[derive(Debug, Clone, Default)]
pub struct PaneTab {
    /// The name on the lozenge, after its number.
    pub name: String,
    /// Spans before the lozenge, such as a channel's glyph in its colour.
    pub lead: Vec<Span<'static>>,
    /// Marked as the target of Tab completion, bold and underlined
    /// ([`crate::strip::target`]).
    pub target: bool,
    /// An action at the end of the bar, such as `+` for a new channel: no
    /// number, never shown as a tab; choosing it opens its menu.
    pub action: bool,
    /// Reserved: pinned to the bar's right end with the other reserved tabs,
    /// never cut away for width, and carrying no number. The tabs left of
    /// them give way first when the bar is short of room.
    pub reserved: bool,
    /// A view rather than a channel, such as the chat's operator view: drawn
    /// on the shaded ground ([`crate::strip::view_seg`]), not the accent.
    pub view: bool,
}

impl PaneTab {
    pub fn new(name: impl Into<String>) -> PaneTab {
        PaneTab { name: name.into(), ..PaneTab::default() }
    }

    pub fn lead(mut self, lead: Vec<Span<'static>>) -> PaneTab {
        self.lead = lead;
        self
    }

    pub fn target(mut self, on: bool) -> PaneTab {
        self.target = on;
        self
    }

    /// An action slot rather than a tab ([`PaneTab::action`]).
    pub fn action(mut self) -> PaneTab {
        self.action = true;
        self
    }

    /// Pinned to the right end of the bar ([`PaneTab::reserved`]).
    pub fn reserved(mut self) -> PaneTab {
        self.reserved = true;
        self
    }

    /// Drawn as a view ([`PaneTab::view`]).
    pub fn view(mut self) -> PaneTab {
        self.view = true;
        self
    }
}

/// One key a screen answers to and what it does: the declaration the
/// footer and the key help read, so nothing writes a key's meaning by hand
/// twice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    /// The key as the footer names it: `Enter`, `PgUp PgDn`, `M-m`.
    pub keys: String,
    pub label: String,
    /// Shown on the footer when it fits; otherwise only in the key help.
    pub footer: bool,
    /// A shorter label the footer falls back to, so the key keeps its
    /// place where the whole label would push it or a later key off.
    pub short: Option<String>,
}

impl Binding {
    /// A key the footer shows.
    pub fn new(keys: impl Into<String>, label: impl Into<String>) -> Binding {
        Binding { keys: keys.into(), label: label.into(), footer: true, short: None }
    }

    /// The same key, with a shorter label for a footer short of room.
    pub fn short(self, label: impl Into<String>) -> Binding {
        Binding { short: Some(label.into()), ..self }
    }

    /// A key only the key help lists.
    pub fn help(keys: impl Into<String>, label: impl Into<String>) -> Binding {
        Binding { footer: false, ..Binding::new(keys, label) }
    }
}

/// How the bottom bar shows a pane's message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// A failure, in the error role.
    Err,
    /// A result just said.
    Said,
    /// Set back: help for what is being typed, or a result said a while ago.
    Back,
}

/// What a key did in a pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keyed {
    /// The pane acted on it.
    Done,
    /// Not the pane's key: the shell's, such as Esc, which quits unless the
    /// pane says otherwise ([`Ctx::esc_quits`]).
    Pass,
    /// Quit, as Esc would: the pane confirmed it.
    Quit,
    /// Give the tab bar the focus, as F2 would: Up past the top of a list
    /// below the bar ([`Pane::below_strip`]) goes back up to it.
    Tabs,
}

/// One row of a pane's menu ([`Open::Pick`]): its label, which comes back
/// through [`Pane::picked`] when it is picked; whether it can be picked, and
/// if not why, which the row says set back; and whether it is the current
/// value, which the row marks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    pub enabled: bool,
    pub reason: Option<String>,
    pub current: bool,
}

impl Item {
    /// An item that can be picked.
    pub fn new(label: impl Into<String>) -> Item {
        Item { label: label.into(), enabled: true, reason: None, current: false }
    }

    /// An item that cannot be picked yet, and why: drawn set back with the
    /// reason, skipped by the highlight, and deaf to a click.
    pub fn disabled(label: impl Into<String>, reason: impl Into<String>) -> Item {
        Item { label: label.into(), enabled: false, reason: Some(reason.into()), current: false }
    }

    /// Marked as the current value, or not.
    pub fn current(self, on: bool) -> Item {
        Item { current: on, ..self }
    }
}

impl<S: Into<String>> From<S> for Item {
    fn from(label: S) -> Item {
        Item::new(label)
    }
}

/// Where a pane's menu docks ([`Open::Pick`]). A menu opened by a click on
/// its tab, or Enter on the focused tab bar, docks under that tab whatever
/// it names, so it opens where the input came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Anchor {
    /// In the middle of the pane.
    #[default]
    Center,
    /// On the line under the pane's tab `i`.
    Tab(usize),
    /// On the line under a row of the pane's own, in screen cells.
    Row(Rect),
    /// Beside the row of the menu it was picked from: a submenu.
    Parent,
}

/// What a pane asks the shell to open over it ([`Pane::take_open`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Open {
    /// Text to read, scrolled in the response modal. `command` is the
    /// command line that prints the same text, which the modal names, so
    /// the screen stays a view over a command.
    Report { label: String, command: String, text: String },
    /// A failure, in the response modal as an error.
    Error { label: String, command: String, text: String },
    /// A menu or picker over `items`, docked at `anchor`; the label of what
    /// is picked comes back through [`Pane::picked`] under `id`. Esc or a
    /// click outside closes it with nothing.
    Pick { id: String, title: String, items: Vec<Item>, multi: bool, anchor: Anchor },
    /// A question on the bottom bar ([`Question`]), answered with `y` or
    /// `n`, or a click on either answer, which the bar draws as lozenges.
    /// Esc, Enter, a click elsewhere, or any other key answers no; a
    /// character the pane would type, and a key the shell takes, then go on
    /// as typed. The answer comes back through [`Pane::picked`] under its
    /// id, as `["y"]` or `["n"]`, and as `[answer, EXPIRED]` when its
    /// [`Expiry`] ran out. Opened again under the id of the question that is
    /// open, it updates that question in place: its text, its answers, and
    /// whether yes waits.
    Ask(Question),
    /// A line typed in a small dialog docked at `anchor`, as a menu docks:
    /// a name, a description. `value` is the text it opens with and `error`
    /// what was wrong with the last one entered, said inside the dialog.
    /// Enter gives the text back through [`Pane::picked`] under `id`, as
    /// `[text]`; the pane checks it and, to refuse it, opens the entry
    /// again with the text and the reason. Esc or a click outside closes it
    /// with nothing. The pane's own text entry is never borrowed for it.
    Entry { id: String, title: String, value: String, error: Option<String>, anchor: Anchor },
    /// Quit, as Ctrl-C does: asking first over unsaved work.
    Quit,
    /// Put `text` on the terminal's clipboard, by OSC 52. A terminal that
    /// does not allow it ignores the sequence; nothing opens.
    Copy { text: String },
}

/// How the shell routes a key beside a pane ([`Pane::keymap_ctx`]): which
/// of its own keys it takes, and what Esc does. One answer, asked of the
/// pane, in place of a question per key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ctx {
    /// The pane types every plain key, as a text entry does: the shell's
    /// single keys (`?`, `q`, `m`, the digits) take their Alt form, and F1
    /// opens the key help.
    pub text: bool,
    /// Beside no text, a plain digit is the shell's: it shows a tab, or
    /// goes to [`Pane::jump`] for a pane that numbers its own list. A pane
    /// that owns the bar types digits as text where it says no.
    pub digits: bool,
    /// The pane holds the keyboard focus itself, the tab bar's included:
    /// one focus owner. The shell then keeps no focus of its own on the
    /// bar. F2, Ctrl+T and [`Keyed::Tabs`] go to [`Pane::focus_tabs`], the
    /// bar draws its cursor where [`Pane::focus_tab`] says, a key the shell
    /// takes leaves the focus where it is, and a plain `q` asks first.
    pub owns_bar: bool,
    /// Esc the pane passes on quits. Without it the pane keeps quitting on
    /// Ctrl-C (and `q` beside no text), and the key help leaves Esc out of
    /// the quit keys.
    pub esc_quits: bool,
}

impl Default for Ctx {
    /// No text, the digits the shell's, the focus the shell's, Esc quits.
    fn default() -> Ctx {
        Ctx { text: false, digits: true, owns_bar: false, esc_quits: true }
    }
}

/// A pane's question ([`Open::Ask`]).
///
/// A question can wait (`pending`): yes is drawn set back and neither its
/// key nor a click presses it, while no answers as ever, until the pane
/// opens the question again, under the same id, with `pending` false. The
/// work the question is about can be under way meanwhile, and its text say
/// so; the update is drawn in place.
///
/// A question can expire ([`Expiry`]): once it is ready to answer, the
/// shell draws the seconds left beside it, counting down on its tick, and
/// at none answers it with the expiry's answer, marked [`EXPIRED`], so the
/// pane can say so. A question that sat unanswered while the operator was
/// away cannot be answered by the next key typed on coming back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub id: String,
    pub question: String,
    /// What yes does ("delete"), or `None` for a question only no
    /// dismisses: one that says why there is nothing to do.
    pub yes: Option<String>,
    /// What no does ("keep").
    pub no: String,
    /// Yes waits: nothing presses it.
    pub pending: bool,
    pub expiry: Option<Expiry>,
}

impl Question {
    /// A question answered at once, by `y` doing `yes` or `n` doing `no`.
    pub fn new(id: impl Into<String>, question: impl Into<String>, yes: impl Into<String>, no: impl Into<String>) -> Question {
        Question { id: id.into(), question: question.into(), yes: Some(yes.into()), no: no.into(), pending: false, expiry: None }
    }

    /// Yes waits until the question is opened again ready.
    pub fn pending(self) -> Question {
        Question { pending: true, ..self }
    }

    /// No yes: only no dismisses it.
    pub fn dismiss_only(self) -> Question {
        Question { yes: None, ..self }
    }

    /// It expires as `expiry` says, counted from when it is ready.
    pub fn expires(self, expiry: Expiry) -> Question {
        Question { expiry: Some(expiry), ..self }
    }
}

/// When a question ([`Question`]) closes on its own, and how: `after` it is
/// ready to answer, it answers `answer` (no, unless set), marked
/// [`EXPIRED`]. `label` and `short` say the seconds left, `{s}` standing for
/// them; the bar draws `label` where it fits and `short` where it does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expiry {
    pub after: std::time::Duration,
    pub answer: bool,
    pub label: String,
    pub short: String,
}

impl Expiry {
    /// Answers no after `after`, saying "closes in 58 s", or "58 s" where
    /// the bar is short of room.
    pub fn new(after: std::time::Duration) -> Expiry {
        Expiry { after, answer: false, label: "closes in {s} s".into(), short: "{s} s".into() }
    }
}

/// The second value of an answer an [`Expiry`] gave.
pub const EXPIRED: &str = "expired";

/// What the shell holds open over a pane ([`Pane::overlay_opened`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlay {
    /// A menu or picker the pane opened ([`Open::Pick`]), by its id.
    /// `by_esc`: the pane opened it in answer to Esc, so a pane that cycles
    /// on Esc knows that closing it moves the cycle on.
    Menu { id: String, by_esc: bool },
    /// The entry dialog the pane opened ([`Open::Entry`]), by its id.
    Entry { id: String },
    /// A question on the bottom bar, the pane's ([`Open::Ask`]) or the
    /// shell's own before a plain `q` quits, by its id.
    Ask { id: String },
    /// The key help.
    Help,
    /// The exit guard over unsaved work.
    Guard,
    /// The response modal ([`Open::Report`], [`Open::Error`]), or another
    /// of the shell's dialogs, which only a tree screen opens.
    Report,
}

/// What an application supplies for a screen that is not a tree. `Any`
/// lets the application reach its own pane back through
/// [`crate::App::pane_ref`].
pub trait Pane: Any {
    /// The palette the screen draws in.
    fn palette(&self) -> Palette;

    /// The area the pane is drawn in this frame, told before the tab bar
    /// asks for [`Pane::tabs`], so the tabs can follow what the pane lays
    /// out at that size. Nothing by default.
    fn layout(&mut self, _area: Rect) {}

    /// The tabs, in order.
    fn tabs(&mut self) -> Vec<PaneTab>;

    /// The tab shown, an index into [`Pane::tabs`].
    fn tab(&mut self) -> usize;

    /// Show tab `i`, from a click or Alt+digit. An index past the tabs
    /// does nothing.
    fn set_tab(&mut self, i: usize);

    /// What the pane numbers in place of its tabs, in a word ("channel"),
    /// for a pane whose own list carries the numbers: the tab bar then
    /// draws none, and Ctrl+digit, Alt+digit and a plain digit the shell
    /// takes ([`Ctx::digits`]) go to [`Pane::jump`]. `None`, the default,
    /// numbers the tabs.
    fn own_numbers(&self) -> Option<&'static str> {
        None
    }

    /// Item `n` of the pane's own numbering ([`Pane::own_numbers`]): by
    /// Ctrl or Alt with a digit (`menu`, which on the item already shown
    /// may open its menu), or a plain digit. Nothing by default.
    fn jump(&mut self, _n: usize, _menu: bool) {}

    /// Whether the pane's tabs have menus ([`Pane::tab_menu`]). Without
    /// them the shell offers no tab-bar focus (F2, Ctrl+T), no menu on a
    /// repeated jump or a right click, and names none of those keys.
    fn has_tab_menus(&self) -> bool {
        false
    }

    /// Open tab `i`'s menu, through [`Pane::take_open`]; on an action slot,
    /// its action. Nothing by default.
    fn tab_menu(&mut self, _i: usize) {}

    /// Tab `i` activated: by its digit or a left click on it (`menu`
    /// false), or by a right click on it or Enter on the focused tab bar
    /// (`menu` true). The one place a tab's input becomes showing it or
    /// opening its menu. By default the shell's rule: a menu when asked
    /// for, on an action slot, or on the tab shown when
    /// [`crate::TabKeys::menu_on_repeat`] says so; else the tab shown. A
    /// pane with targets of its own beyond the tabs (the chat's column)
    /// routes them all through one path of its own here.
    fn activate_tab(&mut self, i: usize, menu: bool) {
        let Some(t) = self.tabs().get(i).cloned() else { return };
        let repeat = self.tab() == i && self.tab_keys().menu_on_repeat;
        if self.has_tab_menus() && (menu || t.action || repeat) {
            self.tab_menu(i);
        } else if !menu {
            self.set_tab(i);
        }
    }

    /// A right click at `at` inside the pane, outside any overlay: open the
    /// menu a left click there would eventually reach, through
    /// [`Pane::take_open`]. Nothing by default.
    fn context(&mut self, _at: Position) {}

    /// What lies below the tab bar for Down to reach, in a word or two
    /// ("the column"), when the pane has such a list. Only a pane with tab
    /// menus is asked; the tab bar's hint names it.
    fn below_strip(&self) -> Option<&'static str> {
        None
    }

    /// Down on the focused tab bar: take the focus into the list
    /// [`Pane::below_strip`] names. False leaves the focus on the bar.
    fn focus_below(&mut self) -> bool {
        false
    }

    /// What Ctrl+N does in this pane, in a few words ("new channel"), when
    /// the pane has such an action. Only a pane with tab menus
    /// ([`Pane::has_tab_menus`]) is asked; with a label the shell takes
    /// Ctrl+N, from the compose box or the tab bar, calls
    /// [`Pane::new_item`] and names the key in the help and the tab bar's
    /// hint. `None` leaves Ctrl+N to the pane.
    fn new_item_label(&self) -> Option<&'static str> {
        None
    }

    /// Ctrl+N: do what [`Pane::new_item_label`] names, through
    /// [`Pane::take_open`] if it opens something.
    fn new_item(&mut self) {}

    /// The shell took a key or a click before the pane saw it: the tab bar
    /// got the focus, a tab was clicked. A pane holding the keyboard in a
    /// list of its own lets go of it.
    fn interrupted(&mut self) {}

    /// How the key help describes a click, when the pane's targets say more
    /// than the shell's line (a tab shows it, the wheel scrolls).
    fn click_help(&self) -> Option<String> {
        None
    }

    /// How the key help describes a right click, when the pane's reaches
    /// more than a tab: it then takes the shell's `right-click a tab` line.
    fn right_click_help(&self) -> Option<String> {
        None
    }

    /// How the key help describes quitting, when the pane's differs from
    /// the shell's (asking first over unsaved work).
    fn quit_help(&self) -> Option<String> {
        None
    }

    /// How the tabs are reached from the keyboard, from the pane's settings.
    fn tab_keys(&self) -> super::panetabs::TabKeys {
        super::panetabs::TabKeys::default()
    }

    /// Whether the pane wants Ctrl+digits and the other ambiguous keys
    /// reported as themselves (the kitty keyboard protocol's first flag).
    fn keyboard_enhancement(&self) -> bool {
        false
    }

    /// Whether the terminal took the keyboard enhancement the pane asked
    /// for: without it Ctrl+3 arrives as Esc and Ctrl+8 as Backspace.
    fn set_keyboard_enhanced(&mut self, _on: bool) {}

    /// How the shell routes a key beside the pane, asked before each key
    /// and each frame ([`Ctx`]).
    fn keymap_ctx(&self) -> Ctx {
        Ctx::default()
    }

    /// The tab the pane's focus is on, for a pane that owns the bar
    /// ([`Ctx::owns_bar`]): where the bar draws its cursor.
    fn focus_tab(&self) -> Option<usize> {
        None
    }

    /// F2 or Ctrl+T beside a pane that owns the focus: move it to the tab
    /// bar, or back from it.
    fn focus_tabs(&mut self) {}

    /// An overlay opened over the pane, on top of those open already. The
    /// shell tells the pane before each frame what opened and closed since
    /// the last, closing first, top down: the keyboard is the overlay's, so
    /// the pane draws no focus of its own, such as a text entry's caret.
    fn overlay_opened(&mut self, _o: Overlay) {}

    /// The overlay on top closed ([`Pane::overlay_opened`]).
    fn overlay_closed(&mut self, _o: Overlay) {}

    /// Text after the tabs, such as what the shown channel is for.
    fn trailer(&mut self) -> Vec<Span<'static>> {
        Vec::new()
    }

    /// Draw the area between the tab bar and the bottom bar.
    fn draw(&mut self, f: &mut Frame, area: Rect);

    /// A key the shell did not take. The shell takes F1, F2, Ctrl+T,
    /// Ctrl+1-9, its Alt chords (Alt+1-9 for a tab, Alt+m for the mouse)
    /// and, while the tab bar has the focus, every key, on every pane; beside
    /// a pane that owns no text its plain forms too: `?`, `q`, `m` and the
    /// digits. A pane binds none of these.
    fn key(&mut self, k: KeyEvent) -> Keyed;

    /// The mouse wheel over the pane: `up` toward older content.
    fn wheel(&mut self, _up: bool) {}

    /// The mouse wheel at `at`, inside the pane's area, for a pane whose
    /// parts scroll apart; [`Pane::wheel`] by default.
    fn wheel_at(&mut self, up: bool, _at: Position) {
        self.wheel(up);
    }

    /// A left click at `at`, inside the pane's area.
    fn click(&mut self, _at: Position) {}

    /// Whether a click at `at`, inside the pane's area, that closes an
    /// overlay goes on to act, as a click on the tab bar does: true for a
    /// list of targets beside the content, such as the chat's column.
    /// Elsewhere such a click only closes the overlay.
    fn click_through(&self, _at: Position) -> bool {
        false
    }

    /// A left or right click that is not the pane's to act on: on the tab
    /// bar or the bottom bar (`None`), or a right click inside the pane at
    /// `at`. Close what the pane holds open over its own content, such as a
    /// text entry lent to a name, when the click is outside it, and say
    /// nothing. A left click inside the pane reaches [`Pane::click`], which
    /// does the same.
    fn dismiss(&mut self, _at: Option<Position>) {}

    /// Text the terminal pasted, whole, newlines and all. Nothing by
    /// default.
    fn paste(&mut self, _text: &str) {}

    /// The pane's keys, for the footer and the key help.
    fn bindings(&self) -> Vec<Binding>;

    /// The bottom bar's lozenge: what the pane is doing.
    fn mode(&self) -> String {
        "browse".into()
    }

    /// The lozenge's ground: the accent, or a state's own colour, such as
    /// a live view's ok and a paused one's warning.
    fn mode_ground(&self) -> theme::Ground {
        theme::Ground::Accent
    }


    /// The bottom bar's message and how it reads, asked each frame.
    fn status(&mut self) -> Option<(String, Tone)> {
        None
    }

    /// Whether the shell reports the mouse when the screen opens. With it
    /// on, the terminal's own selection and middle-click paste need Shift
    /// or the toggle; a pane people copy text from may start with it off.
    fn mouse_default(&self) -> bool {
        true
    }

    /// Something to open over the pane: the response modal or a picker.
    /// The shell asks after each key, click and tick that reaches the pane.
    fn take_open(&mut self) -> Option<Open> {
        None
    }

    /// The picker opened under `id` set `values`.
    fn picked(&mut self, _id: &str, _values: Vec<String>) {}

    /// The shell took a key of its own (the key help, the mouse, a tab,
    /// quitting, a new item): the operator pressed a key the pane never
    /// sees.
    fn shell_key_taken(&mut self) {}

    /// The shell could not open `open` (a menu, a question or an entry
    /// dialog) because something else was open: what it asked for will
    /// never be picked or answered.
    fn open_dropped(&mut self, _open: Open) {}

    /// Work that quitting would lose, named for the exit guard.
    fn unsaved(&self) -> Option<String> {
        None
    }

    /// Drop the unsaved work: the operator chose to quit anyway.
    fn discard(&mut self) {}

    /// Text for the key help, below the keys.
    fn help(&self) -> Option<String> {
        None
    }

    /// How often [`Pane::tick`] runs; `None` never.
    fn tick_every(&self) -> Option<Duration> {
        None
    }

    /// Time has passed: read a source again.
    fn tick(&mut self) {}
}

/// Bindings as the footer draws them, key in the accent and label set back,
/// each separated by a dot: the one look of every screen's footer.
pub(crate) fn footer_spans<'a>(items: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (i, (key, label)) in items.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", theme::hint()));
        }
        if !key.is_empty() {
            spans.push(Span::styled(format!("{key} "), theme::accent()));
        }
        spans.push(Span::styled(label.to_string(), theme::hint()));
    }
    spans
}

/// What is wrong with a screen's bindings: a key bound twice, or bound to
/// nothing. Empty when sound.
pub fn binding_conflicts(bindings: &[Binding]) -> Vec<String> {
    let mut out = Vec::new();
    for (i, b) in bindings.iter().enumerate() {
        if b.keys.trim().is_empty() || b.label.trim().is_empty() {
            out.push(format!("{:?}: a key and a label both", b.keys));
        }
        if bindings[..i].iter().any(|o| o.keys == b.keys) {
            out.push(format!("{}: bound twice", b.keys));
        }
    }
    out
}
