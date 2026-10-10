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
//! A pane that owns text ([`Pane::owns_text`]) types every plain key, so the
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
}

impl Binding {
    /// A key the footer shows.
    pub fn new(keys: impl Into<String>, label: impl Into<String>) -> Binding {
        Binding { keys: keys.into(), label: label.into(), footer: true }
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
    /// Not the pane's key: the shell's, such as Esc, which quits.
    Pass,
    /// Quit, as Esc would: the pane confirmed it.
    Quit,
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
    /// A picker over `options`, `chosen` marked; what is picked comes back
    /// through [`Pane::picked`] under `id`. Esc closes it with nothing.
    Pick { id: String, title: String, options: Vec<String>, multi: bool, chosen: Vec<String> },
}

/// What an application supplies for a screen that is not a tree. `Any`
/// lets the application reach its own pane back through
/// [`crate::App::pane_ref`].
pub trait Pane: Any {
    /// The palette the screen draws in.
    fn palette(&self) -> Palette;

    /// The tabs, in order.
    fn tabs(&mut self) -> Vec<PaneTab>;

    /// The tab shown, an index into [`Pane::tabs`].
    fn tab(&mut self) -> usize;

    /// Show tab `i`, from a click or Alt+digit. An index past the tabs
    /// does nothing.
    fn set_tab(&mut self, i: usize);

    /// Whether the pane's tabs have menus ([`Pane::tab_menu`]). Without
    /// them the shell offers no tab-bar focus (F2, Ctrl+T), no menu on a
    /// repeated jump or a right click, and names none of those keys.
    fn has_tab_menus(&self) -> bool {
        false
    }

    /// Open tab `i`'s menu, through [`Pane::take_open`]; on an action slot,
    /// its action. Nothing by default.
    fn tab_menu(&mut self, _i: usize) {}

    /// The shell took a key or a click before the pane saw it: the tab bar
    /// got the focus, a tab was clicked. A pane waiting on an answer (a
    /// `y` to confirm) drops the question.
    fn interrupted(&mut self) {}

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

    /// Whether the pane types every plain key, as a text entry does.
    fn owns_text(&self) -> bool {
        false
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
