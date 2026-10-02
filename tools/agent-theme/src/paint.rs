//! Roles, styles and the painter that turns them into terminal output
//! (ADR-504 §4, §6). The ANSI output lives here; the ratatui output, behind
//! the `ratatui` feature, maps the same resolution onto ratatui styles.
//!
//! A painter has two palettes:
//!
//! - **Terminal**, the built-in default: status roles are the 16 ANSI
//!   colours, so the terminal's own palette decides how they look, muted
//!   text is the dim attribute and selection is reverse video. It reads no
//!   file and computes nothing, which is what the hook paths use (§11).
//! - **Theme**: every role is the RGB a theme derives (contrast-lifted and
//!   kept distinct), reduced to the terminal's depth.
//!
//! At `ColorDepth::NoColor` either palette drops every colour; muted text
//! stays dim and selection becomes reverse video.

use std::cell::Cell;
use std::fmt::Display;
use std::sync::OnceLock;

use crate::color::Color;
use crate::depth::ColorDepth;
use crate::derive::Roles;
use crate::model::{Background, Theme};

/// Select Graphic Rendition reset: every attribute and colour off.
pub const RESET: &str = "\x1b[0m";

/// What a piece of output means. A role is drawn by the active palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// Ordinary text.
    Body,
    /// The theme's accent: headings, names, the thing to look at.
    Accent,
    Info,
    Ok,
    Warn,
    Err,
    /// A second accent, for content set apart from prose (code spans).
    Alt,
    /// Warn and err halfway: urgent but not failed.
    Hot,
    /// Secondary text: hints, counts, separators.
    Muted,
    /// Decorative rules.
    Rule,
    /// Text that has receded: past, inactive.
    Faded,
    /// The selected row or item.
    Selection,
}

/// The attribute bits a style carries, in SGR order.
const ATTRS: [(u8, &str); 6] = [(BOLD, "1"), (DIM, "2"), (ITALIC, "3"), (UNDERLINE, "4"), (REVERSE, "7"), (STRIKE, "9")];
const BOLD: u8 = 1;
const DIM: u8 = 2;
const ITALIC: u8 = 4;
const UNDERLINE: u8 = 8;
const REVERSE: u8 = 16;
const STRIKE: u8 = 32;

/// A colour as a style asks for it: a role the palette draws, or a fixed
/// colour (categorical colours and the banner gradient, which are not theme
/// roles).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ink {
    Role(Role),
    Color(Color),
}

/// Attributes and colours for a span of text, resolved by a [`Painter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Style {
    fg: Option<Ink>,
    bg: Option<Ink>,
    attrs: u8,
}

impl Style {
    pub const fn new() -> Style {
        Style { fg: None, bg: None, attrs: 0 }
    }
    /// Text in `role`. A role that draws a ground (selection) brings it.
    pub const fn role(self, role: Role) -> Style {
        Style { fg: Some(Ink::Role(role)), ..self }
    }
    /// Text in a fixed colour.
    pub const fn fg(self, c: Color) -> Style {
        Style { fg: Some(Ink::Color(c)), ..self }
    }
    /// A fixed background colour.
    pub const fn bg(self, c: Color) -> Style {
        Style { bg: Some(Ink::Color(c)), ..self }
    }
    pub const fn bold(self) -> Style {
        self.with(BOLD)
    }
    pub const fn dim(self) -> Style {
        self.with(DIM)
    }
    pub const fn italic(self) -> Style {
        self.with(ITALIC)
    }
    pub const fn underline(self) -> Style {
        self.with(UNDERLINE)
    }
    pub const fn reverse(self) -> Style {
        self.with(REVERSE)
    }
    pub const fn strike(self) -> Style {
        self.with(STRIKE)
    }
    const fn with(self, bit: u8) -> Style {
        Style { attrs: self.attrs | bit, ..self }
    }
    pub fn fg_ink(&self) -> Option<Ink> {
        self.fg
    }
    pub fn bg_ink(&self) -> Option<Ink> {
        self.bg
    }
    pub fn is_bold(&self) -> bool {
        self.attrs & BOLD != 0
    }
    pub fn is_dim(&self) -> bool {
        self.attrs & DIM != 0
    }
    pub fn is_italic(&self) -> bool {
        self.attrs & ITALIC != 0
    }
    pub fn is_underline(&self) -> bool {
        self.attrs & UNDERLINE != 0
    }
    pub fn is_reverse(&self) -> bool {
        self.attrs & REVERSE != 0
    }
    pub fn is_strike(&self) -> bool {
        self.attrs & STRIKE != 0
    }
}

impl From<Role> for Style {
    fn from(r: Role) -> Style {
        Style::new().role(r)
    }
}

impl From<Color> for Style {
    fn from(c: Color) -> Style {
        Style::new().fg(c)
    }
}

/// A style after the palette and depth have had their say: concrete
/// colours, already reduced to the depth, and attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Resolved {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub reverse: bool,
    pub strike: bool,
}

/// How a palette draws one role: colours and attributes.
#[derive(Clone, Copy)]
struct Drawn {
    fg: Option<Color>,
    bg: Option<Color>,
    attrs: u8,
}

const fn drawn(fg: Option<Color>, attrs: u8) -> Drawn {
    Drawn { fg, bg: None, attrs }
}

/// The terminal palette: the ANSI colours and attributes agent-ways output
/// has always used, so the default output is unchanged by the engine.
fn terminal_role(r: Role) -> Drawn {
    match r {
        Role::Body => drawn(None, 0),
        Role::Accent => drawn(Some(Color::Ansi(6)), 0),
        Role::Info => drawn(Some(Color::Ansi(4)), 0),
        Role::Ok => drawn(Some(Color::Ansi(2)), 0),
        Role::Warn => drawn(Some(Color::Ansi(3)), 0),
        Role::Err => drawn(Some(Color::Ansi(1)), 0),
        Role::Alt => drawn(Some(Color::Ansi(3)), 0),
        Role::Hot => drawn(Some(Color::Ansi(9)), 0),
        Role::Muted | Role::Rule | Role::Faded => drawn(None, DIM),
        Role::Selection => drawn(None, REVERSE),
    }
}

fn theme_role(r: Role, roles: &Roles) -> Drawn {
    let c = |x| Some(Color::Rgb(x));
    match r {
        Role::Body => drawn(c(roles.body), 0),
        Role::Accent => drawn(c(roles.accent), 0),
        Role::Info => drawn(c(roles.info), 0),
        Role::Ok => drawn(c(roles.ok), 0),
        Role::Warn => drawn(c(roles.warn), 0),
        Role::Err => drawn(c(roles.err), 0),
        Role::Alt => drawn(c(roles.alt), 0),
        Role::Hot => drawn(c(roles.hot), 0),
        Role::Muted => drawn(c(roles.muted), 0),
        Role::Rule => drawn(c(roles.rule), 0),
        Role::Faded => drawn(c(roles.faded_text), 0),
        Role::Selection => Drawn { fg: c(roles.body), bg: c(roles.selection_bg), attrs: 0 },
    }
}

/// Resolves styles under one palette and colour depth, into ANSI SGR
/// strings or (with the `ratatui` feature) ratatui styles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Painter {
    depth: ColorDepth,
    /// `None` is the terminal palette.
    roles: Option<Roles>,
    fill: bool,
    /// No attributes either: every style is plain text.
    plain: bool,
}

impl Painter {
    /// The built-in default at `depth`: the terminal palette.
    pub const fn terminal(depth: ColorDepth) -> Painter {
        Painter { depth, roles: None, fill: false, plain: false }
    }

    /// The built-in default at the environment's depth. Reads no file.
    ///
    /// Output that is not going to a terminal is plain, with no colour and
    /// no attributes: a pipe may be machine-carried text (a hook injection,
    /// a Monitor event), and escapes there are noise. `CLICOLOR_FORCE` or
    /// `FORCE_COLOR`, set to anything but empty or `0`, keeps a pipe styled.
    pub fn detect() -> Painter {
        if !styled_output() {
            return Painter::plain();
        }
        Painter::terminal(ColorDepth::detect())
    }

    /// The painter for a chosen theme at `depth` (ADR-504, note of
    /// 2026-10-01): the theme where the terminal shows 256 colours or more,
    /// else the 16-colour terminal palette whole, not the theme reduced to
    /// 16. With no theme chosen, the terminal palette.
    pub fn select(theme: Option<&Theme>, depth: ColorDepth) -> Painter {
        match (theme, depth) {
            (Some(t), ColorDepth::TrueColor | ColorDepth::Ansi256) => Painter::themed(t, depth),
            _ => Painter::terminal(depth),
        }
    }

    /// No colour and no attributes: every style renders as plain text.
    pub const fn plain() -> Painter {
        Painter { depth: ColorDepth::NoColor, roles: None, fill: false, plain: true }
    }

    /// A theme's derived roles at `depth`.
    pub fn themed(theme: &Theme, depth: ColorDepth) -> Painter {
        Painter { depth, roles: Some(Roles::derive(theme)), fill: theme.background == Background::Fill, plain: false }
    }

    /// The theme named `name`, the active choice its caller read from the
    /// settings registry (`theme.active`, ADR-503), at the environment's
    /// depth, chosen by [`Painter::select`]; plain when output is not a
    /// terminal, as [`Painter::detect`]. A warning from [`Painter::named_in`]
    /// goes to stderr. This reads the theme directory, so hook paths never
    /// call it (ADR-504 §11).
    pub fn named(name: Option<&str>) -> Painter {
        if !styled_output() {
            return Painter::plain();
        }
        let depth = ColorDepth::detect();
        let Some(dir) = crate::bundled::user_dir() else { return Painter::terminal(depth) };
        let (p, warning) = Painter::named_in(name, &dir, depth);
        if let Some(w) = warning {
            eprintln!("agent-ways theme: {w}");
        }
        p
    }

    /// The theme `name` from the bundled themes and the user themes in
    /// `dir`, at `depth`, and a warning when the choice could not be
    /// honoured as written: the named theme does not exist, or a user file
    /// meant to override it failed to load. No name, or the default's
    /// ([`crate::TERMINAL`]), is the terminal palette.
    pub fn named_in(name: Option<&str>, dir: &std::path::Path, depth: ColorDepth) -> (Painter, Option<String>) {
        let Some(name) = name.map(str::trim).filter(|n| !n.is_empty() && *n != crate::bundled::TERMINAL) else {
            return (Painter::terminal(depth), None);
        };
        let set = crate::bundled::ThemeSet::load(Some(dir));
        let rejected = set.rejected.iter().find(|(file, _)| {
            std::path::Path::new(file).file_stem().and_then(|s| s.to_str()) == Some(name)
        });
        let warning = match (set.get(name), rejected) {
            (None, None) => Some(format!("theme `{name}` not found; using the default")),
            (found, Some((file, errs))) => {
                let first = errs.first().map(|e| e.to_string()).unwrap_or_default();
                let using = if found.is_some() { "the bundled theme of that name" } else { "the default" };
                Some(format!("{file} did not load ({first}); using {using}"))
            }
            (Some(_), None) => None,
        };
        (Painter::select(set.get(name), depth), warning)
    }

    pub fn depth(&self) -> ColorDepth {
        self.depth
    }

    /// The derived roles, when a theme is in use.
    pub fn roles(&self) -> Option<&Roles> {
        self.roles.as_ref()
    }

    /// Whether the theme paints its background behind every cell
    /// (`THEME_BACKGROUND="fill"`). Screens honour it; plain output does not.
    pub fn fills_background(&self) -> bool {
        self.fill && self.roles.is_some() && self.depth.has_color()
    }

    fn draw(&self, r: Role) -> Drawn {
        match (&self.roles, self.depth) {
            (Some(roles), d) if d.has_color() => theme_role(r, roles),
            _ => terminal_role(r),
        }
    }

    /// `style` with its roles drawn and its colours reduced to the depth.
    pub fn resolve(&self, style: impl Into<Style>) -> Resolved {
        let s = style.into();
        let mut attrs = s.attrs;
        let (mut fg, mut bg) = (None, None);
        match s.fg {
            Some(Ink::Role(r)) => {
                let d = self.draw(r);
                fg = d.fg;
                bg = d.bg;
                attrs |= d.attrs;
            }
            Some(Ink::Color(c)) => fg = Some(c),
            None => {}
        }
        match s.bg {
            Some(Ink::Role(r)) => {
                let d = self.draw(r);
                bg = d.bg.or(d.fg);
                attrs |= d.attrs;
            }
            Some(Ink::Color(c)) => bg = Some(c),
            None => {}
        }
        if self.plain {
            return Resolved::default();
        }
        Resolved {
            fg: fg.and_then(|c| c.at(self.depth)),
            bg: bg.and_then(|c| c.at(self.depth)),
            bold: attrs & BOLD != 0,
            dim: attrs & DIM != 0,
            italic: attrs & ITALIC != 0,
            underline: attrs & UNDERLINE != 0,
            reverse: attrs & REVERSE != 0,
            strike: attrs & STRIKE != 0,
        }
    }

    /// The SGR sequence that turns `style` on, or `""` when it changes
    /// nothing. Attributes come first, then the foreground, then the ground,
    /// in one sequence.
    pub fn sgr(&self, style: impl Into<Style>) -> String {
        if self.is_plain() {
            return String::new();
        }
        let r = self.resolve(style);
        let mut params: Vec<String> = Vec::new();
        let flags = [r.bold, r.dim, r.italic, r.underline, r.reverse, r.strike];
        for ((_, code), on) in ATTRS.iter().zip(flags) {
            if on {
                params.push((*code).to_string());
            }
        }
        if let Some(c) = r.fg {
            params.push(c.sgr_params(false));
        }
        if let Some(c) = r.bg {
            params.push(c.sgr_params(true));
        }
        if params.is_empty() {
            String::new()
        } else {
            format!("\x1b[{}m", params.join(";"))
        }
    }

    /// `text` in `style`, followed by a reset; `text` alone when the style
    /// changes nothing.
    pub fn paint(&self, style: impl Into<Style>, text: impl Display) -> String {
        let on = self.sgr(style);
        if on.is_empty() {
            text.to_string()
        } else {
            format!("{on}{text}{RESET}")
        }
    }

    /// The SGR that turns `style` on and the reset that turns it off, both
    /// empty when the style changes nothing. For spans built up in pieces.
    pub fn pair(&self, style: impl Into<Style>) -> (String, &'static str) {
        let on = self.sgr(style);
        let off = if on.is_empty() { "" } else { RESET };
        (on, off)
    }

    /// The reset, or `""` for a plain painter.
    pub fn reset(&self) -> &'static str {
        if self.is_plain() {
            ""
        } else {
            RESET
        }
    }

    /// Whether this painter writes no escapes at all.
    pub fn is_plain(&self) -> bool {
        self.plain
    }
}

/// Whether output should be styled: stdout is a terminal, or a caller
/// forces colour with `CLICOLOR_FORCE` or `FORCE_COLOR`.
fn styled_output() -> bool {
    use std::io::IsTerminal;
    let forced = |k: &str| std::env::var(k).is_ok_and(|v| !v.is_empty() && v != "0");
    forced("CLICOLOR_FORCE") || forced("FORCE_COLOR") || std::io::stdout().is_terminal()
}

static GLOBAL: OnceLock<Painter> = OnceLock::new();

thread_local! {
    static SCOPED: Cell<Option<Painter>> = const { Cell::new(None) };
}

/// The painter output in this process uses: a scoped override on this
/// thread if one is set, else the one installed with [`install`], else the
/// built-in default at the environment's depth ([`Painter::detect`]).
pub fn painter() -> Painter {
    SCOPED.with(Cell::get).unwrap_or_else(|| *GLOBAL.get_or_init(Painter::detect))
}

/// Set the process painter. Returns `false`, changing nothing, when output
/// has already used one. Call it first thing in `main`.
pub fn install(p: Painter) -> bool {
    GLOBAL.set(p).is_ok()
}

/// Restores this thread's previous painter when dropped.
pub struct Scope {
    previous: Option<Painter>,
}

impl Drop for Scope {
    fn drop(&mut self) {
        SCOPED.with(|s| s.set(self.previous));
    }
}

/// Use `p` on this thread until the returned guard drops. Tests use it to
/// pin output independent of the environment they run in.
#[must_use = "the override ends when the guard drops"]
pub fn scoped(p: Painter) -> Scope {
    Scope { previous: SCOPED.with(|s| s.replace(Some(p))) }
}

/// `text` in `style` with the process painter.
pub fn paint(style: impl Into<Style>, text: impl Display) -> String {
    painter().paint(style, text)
}

/// The SGR for `style` with the process painter.
pub fn sgr(style: impl Into<Style>) -> String {
    painter().sgr(style)
}

/// On and off sequences for `style` with the process painter.
pub fn pair(style: impl Into<Style>) -> (String, &'static str) {
    painter().pair(style)
}

/// The reset with the process painter.
pub fn reset() -> &'static str {
    painter().reset()
}
