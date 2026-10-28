//! The theme file format, its parser and its writer (ADR-504 §4).
//!
//! A theme file has the structure of the dotfiles palettes: bare uppercase
//! `THEME_*` keys, double-quoted values and `#` comments. That is valid TOML
//! as written, so it is read with the `toml` crate, keys and values with
//! their spans, and every problem names its line. A dotfiles palette file
//! loads unchanged.
//!
//! ```toml
//! # Nord — cool desaturated arctic blues.
//! THEME_NAME="nord"            # [a-z0-9-]+
//! THEME_LABEL="Nord"           # optional, defaults to the name
//! THEME_KIND="dark"            # dark | light
//! THEME_BACKGROUND="terminal"  # agent-ways: terminal (default) | fill
//!
//! THEME_BG="#2e3440"           # the ten slots, all required:
//! ...                          # BG FG DIM SUBTLE ACCENT INFO OK WARN ERR ALT
//!
//! THEME_HOT="#d08770"          # agent-ways, optional: pin a derived role
//!                              # (HOT, RULE, FADED, SELECTION)
//! THEME_VIVID="nord"           # dotfiles tool bindings: accepted, ignored
//! ```
//!
//! agent-ways' optional chrome, each key absent by default:
//!
//! ```toml
//! THEME_GRADIENT="vertical #0d0221 #4a0e4e"  # vertical | horizontal, 2-4 stops; fill only
//! THEME_BORDER="rounded"                     # plain | rounded | double | thick
//! THEME_BORDER_GLYPHS="╭╮╰╯~·││"             # or eight glyphs: corners TL TR BL BR,
//!                                            # then edges top bottom left right
//! THEME_RULE_GLYPHS="─━"                     # the rule, the selected rule
//! THEME_MARK="▌"                             # the selected row's mark
//! THEME_PRESENCE="●○"                        # live, idle
//! THEME_ORNAMENT="🌴"                         # before pane titles; may be two columns
//! THEME_EVENT_HEAD="◓"                       # a session's event glyphs, each one
//!                                            # (DUE two): HEAD MARK MATCHED INJECTED
//!                                            # SUPPRESSED REDISCLOSED WOULD_BLOCK BLOCKED
//!                                            # REFIRE_HELD CAP_HELD DUE APPROACHING
//!                                            # HALFWAY CHECK
//! ```
//!
//! Every glyph is one grapheme one column wide, but the ornament, which may
//! be two.
//!
//! Three tables may follow the bare keys, each optional. `[target.<name>]`
//! pins a target's keys (see `target`). `[words]` holds the words Claude
//! Code shows under the theme: each list is 1 to 12 entries of 1 to 24
//! characters, no control characters, and either may be left out.
//!
//! ```toml
//! [words]
//! spinner = ["Cruising", "Gliding"]  # while a turn runs
//! done = ["Cruised", "Docked"]       # the turn's done line
//! ```
//!
//! Waypoints, the marks a way's cluster wears on the replay, come last, as
//! an array of tables; each fg is held to 3:1 on the theme's grounds when
//! the theme is drawn:
//!
//! ```toml
//! [[waypoints]]
//! glyph = "🌴"      # one column, or an emoji two wide
//! fg = "#3dffa2"    # optional; a colour emoji ignores it
//! ```

use std::fmt;

use serde::de::{self, Deserialize, Deserializer, IgnoredAny, MapAccess, Visitor};
use toml::Spanned;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::model::{intern, Axis, Background, Border, Decor, Events, Gradient, Kind, Overrides, Rgb, Slots, Targets, Theme, Waypoint, Waypoints, Words};
use crate::target;

/// The dotfiles tool bindings: part of the palette structure, no meaning here.
const IGNORED: [&str; 3] = ["THEME_VIVID", "THEME_BAT", "THEME_DELTA"];

/// One problem in a theme file, with its line when it has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeError {
    pub line: Option<usize>,
    pub message: String,
}

impl ThemeError {
    fn at(line: usize, message: impl Into<String>) -> Self {
        ThemeError { line: Some(line), message: message.into() }
    }
    pub(crate) fn whole(message: impl Into<String>) -> Self {
        ThemeError { line: None, message: message.into() }
    }
}

impl fmt::Display for ThemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(n) => write!(f, "line {n}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

/// A value as the theme format sees it: a string, a table or an array of
/// values with their spans, or the name of the TOML type it has instead.
enum Leaf {
    Str(String),
    Table(Vec<(Spanned<String>, Spanned<Leaf>)>),
    Array(Vec<Spanned<Leaf>>),
    Other(&'static str),
}

impl Leaf {
    /// What a value that should have been a string is, for its error.
    fn kind(&self) -> &'static str {
        match self {
            Leaf::Str(_) => "a string",
            Leaf::Table(_) => "a table",
            Leaf::Array(_) => "an array",
            Leaf::Other(t) => t,
        }
    }
}

/// toml hands a date or time over as a one-entry table under this key.
const DATETIME: &str = "$__toml_private_datetime";

struct LeafVisitor;

impl<'de> Visitor<'de> for LeafVisitor {
    type Value = Leaf;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a value")
    }
    fn visit_str<E>(self, v: &str) -> Result<Leaf, E> {
        Ok(Leaf::Str(v.to_string()))
    }
    fn visit_bool<E>(self, _: bool) -> Result<Leaf, E> {
        Ok(Leaf::Other("a boolean"))
    }
    fn visit_i64<E>(self, _: i64) -> Result<Leaf, E> {
        Ok(Leaf::Other("a number"))
    }
    fn visit_u64<E>(self, _: u64) -> Result<Leaf, E> {
        Ok(Leaf::Other("a number"))
    }
    fn visit_f64<E>(self, _: f64) -> Result<Leaf, E> {
        Ok(Leaf::Other("a number"))
    }
    fn visit_seq<A: de::SeqAccess<'de>>(self, mut s: A) -> Result<Leaf, A::Error> {
        let mut out = Vec::new();
        while let Some(v) = s.next_element::<Spanned<Leaf>>()? {
            out.push(v);
        }
        Ok(Leaf::Array(out))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Leaf, A::Error> {
        let mut out = Vec::new();
        while let Some(k) = m.next_key::<Spanned<String>>()? {
            if k.get_ref() == DATETIME {
                m.next_value::<IgnoredAny>()?;
                while m.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                return Ok(Leaf::Other("a date"));
            }
            out.push((k, m.next_value::<Spanned<Leaf>>()?));
        }
        Ok(Leaf::Table(out))
    }
}

impl<'de> Deserialize<'de> for Leaf {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Leaf, D::Error> {
        d.deserialize_any(LeafVisitor)
    }
}

/// The document's top level, in file order. Keys carry their spans and
/// values do not: toml refuses a span for a table a dotted header opens
/// (`[target.claude-code]`).
struct Top(Vec<(Spanned<String>, Leaf)>);

impl<'de> Deserialize<'de> for Top {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Top, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Top;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a theme")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Top, A::Error> {
                let mut out = Vec::new();
                while let Some(kv) = m.next_entry::<Spanned<String>, Leaf>()? {
                    out.push(kv);
                }
                Ok(Top(out))
            }
        }
        d.deserialize_map(V)
    }
}

fn valid_name(n: &str) -> bool {
    !n.is_empty() && n.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The 1-based line of a byte offset.
fn line_of(src: &str, at: usize) -> usize {
    src[..at.min(src.len())].bytes().filter(|b| *b == b'\n').count() + 1
}

/// A syntax error, on its line. A value that is not a quoted string is the
/// likely cause in this format, so the message says what a value must be.
fn syntax(src: &str, e: &toml::de::Error) -> ThemeError {
    let first = e.message().lines().next().unwrap_or("invalid TOML").trim().to_string();
    let Some(span) = e.span() else { return ThemeError::whole(format!("not valid TOML: {first}")) };
    let n = line_of(src, span.start);
    let text = src.lines().nth(n - 1).unwrap_or("");
    // toml's own message for a date names its private datetime key.
    if first.contains(DATETIME) {
        let key = text.split_once('=').map_or("", |(k, _)| k.trim());
        return ThemeError::at(n, format!("`{key}` is a date; a value is a double-quoted string"));
    }
    let hint = match text.split_once('=').map(|(_, v)| v.trim()) {
        Some(v) if !v.starts_with('"') => "; values are double-quoted strings",
        _ => "",
    };
    ThemeError::at(n, format!("{first}{hint}"))
}

/// The slot or override a `THEME_*` key names, lowercased.
fn suffix(key: &str) -> Option<String> {
    key.strip_prefix("THEME_").map(str::to_ascii_lowercase)
}

/// `THEME_GRADIENT`'s value: an axis, then two to four stops.
fn gradient(value: &str) -> Result<Gradient, String> {
    let mut words = value.split_whitespace();
    let axis = match words.next() {
        Some("vertical") => Axis::Vertical,
        Some("horizontal") => Axis::Horizontal,
        w => return Err(format!("THEME_GRADIENT axis `{}` must be vertical or horizontal", w.unwrap_or(""))),
    };
    let mut stops = Vec::new();
    for w in words {
        stops.push(Rgb::from_hex(w).ok_or_else(|| format!("`{w}` is not a #rrggbb colour (key `THEME_GRADIENT`)"))?);
    }
    Gradient::new(axis, &stops).ok_or_else(|| format!("THEME_GRADIENT takes 2 to 4 stops; `{value}` has {}", stops.len()))
}

/// The control characters and the line and paragraph separators: a glyph
/// holding one would move the cursor or break the line it is drawn in.
fn unprintable(g: &str) -> bool {
    g.chars().any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
}

/// `g`'s width in cells by ratatui's rule (ratatui-core 0.1.2,
/// `buffer/cell_width.rs`): unicode-width's, and one more for each halfwidth
/// katakana sound mark, U+FF9E and U+FF9F, which terminals draw a cell wide.
pub(crate) fn cell_width(g: &str) -> usize {
    g.width() + g.chars().filter(|c| matches!(c, '\u{FF9E}' | '\u{FF9F}')).count()
}

/// `value` as `count` glyphs for `key`, each one grapheme one column wide,
/// or up to `widest` columns.
fn glyphs(key: &str, value: &str, count: usize, widest: usize) -> Result<Vec<&'static str>, String> {
    let g: Vec<&str> = value.graphemes(true).collect();
    if g.len() != count {
        let noun = if count == 1 { "glyph" } else { "glyphs" };
        return Err(format!("`{key}` takes {count} {noun}; `{}` has {}", value.escape_debug(), g.len()));
    }
    for x in &g {
        if unprintable(x) {
            return Err(format!("`{key}` glyph `{}` is a control character or a line separator", x.escape_debug()));
        }
        let w = cell_width(x);
        if w == 0 || w > widest {
            let must = if widest == 1 { "1".to_string() } else { format!("1 to {widest}") };
            return Err(format!("`{key}` glyph `{}` is {w} columns wide; it must be {must}", x.escape_debug()));
        }
    }
    Ok(g.into_iter().map(intern).collect())
}

/// The code points with Emoji_Presentation=Yes, from Unicode's
/// emoji-data.txt, version 17.0 (https://www.unicode.org/Public/17.0.0/emoji/emoji-data.txt),
/// the version unicode-width 0.2.2 measures by.
const EMOJI_PRESENTATION: [(u32, u32); 81] = [
    (0x231A, 0x231B), (0x23E9, 0x23EC), (0x23F0, 0x23F0), (0x23F3, 0x23F3), (0x25FD, 0x25FE),
    (0x2614, 0x2615), (0x2648, 0x2653), (0x267F, 0x267F), (0x2693, 0x2693), (0x26A1, 0x26A1),
    (0x26AA, 0x26AB), (0x26BD, 0x26BE), (0x26C4, 0x26C5), (0x26CE, 0x26CE), (0x26D4, 0x26D4),
    (0x26EA, 0x26EA), (0x26F2, 0x26F3), (0x26F5, 0x26F5), (0x26FA, 0x26FA), (0x26FD, 0x26FD),
    (0x2705, 0x2705), (0x270A, 0x270B), (0x2728, 0x2728), (0x274C, 0x274C), (0x274E, 0x274E),
    (0x2753, 0x2755), (0x2757, 0x2757), (0x2795, 0x2797), (0x27B0, 0x27B0), (0x27BF, 0x27BF),
    (0x2B1B, 0x2B1C), (0x2B50, 0x2B50), (0x2B55, 0x2B55), (0x1F004, 0x1F004), (0x1F0CF, 0x1F0CF),
    (0x1F18E, 0x1F18E), (0x1F191, 0x1F19A), (0x1F1E6, 0x1F1FF), (0x1F201, 0x1F201),
    (0x1F21A, 0x1F21A), (0x1F22F, 0x1F22F), (0x1F232, 0x1F236), (0x1F238, 0x1F23A),
    (0x1F250, 0x1F251), (0x1F300, 0x1F320), (0x1F32D, 0x1F335), (0x1F337, 0x1F37C),
    (0x1F37E, 0x1F393), (0x1F3A0, 0x1F3CA), (0x1F3CF, 0x1F3D3), (0x1F3E0, 0x1F3F0),
    (0x1F3F4, 0x1F3F4), (0x1F3F8, 0x1F43E), (0x1F440, 0x1F440), (0x1F442, 0x1F4FC),
    (0x1F4FF, 0x1F53D), (0x1F54B, 0x1F54E), (0x1F550, 0x1F567), (0x1F57A, 0x1F57A),
    (0x1F595, 0x1F596), (0x1F5A4, 0x1F5A4), (0x1F5FB, 0x1F64F), (0x1F680, 0x1F6C5),
    (0x1F6CC, 0x1F6CC), (0x1F6D0, 0x1F6D2), (0x1F6D5, 0x1F6D8), (0x1F6DC, 0x1F6DF),
    (0x1F6EB, 0x1F6EC), (0x1F6F4, 0x1F6FC), (0x1F7E0, 0x1F7EB), (0x1F7F0, 0x1F7F0),
    (0x1F90C, 0x1F93A), (0x1F93C, 0x1F945), (0x1F947, 0x1F9FF), (0x1FA70, 0x1FA7C),
    (0x1FA80, 0x1FA8A), (0x1FA8E, 0x1FAC6), (0x1FAC8, 0x1FAC8), (0x1FACD, 0x1FADC),
    (0x1FADF, 0x1FAEA), (0x1FAEF, 0x1FAF8),
];

fn emoji_presentation(c: char) -> bool {
    let c = c as u32;
    EMOJI_PRESENTATION.binary_search_by(|&(lo, hi)| if hi < c { std::cmp::Ordering::Less } else if lo > c { std::cmp::Ordering::Greater } else { std::cmp::Ordering::Equal }).is_ok()
}

/// A waypoint's glyph: one grapheme one column wide, or two wide where
/// every code point has Emoji_Presentation=Yes. A variation selector, a
/// joiner, a skin-tone modifier or a regional indicator would leave the
/// width to the font, so none is taken.
fn waypoint_glyph(g: &str) -> Result<&'static str, String> {
    if unprintable(g) {
        return Err(format!("glyph `{}` is a control character or a line separator", g.escape_debug()));
    }
    let n = g.graphemes(true).count();
    if n != 1 {
        return Err(format!("glyph `{g}` is {n} graphemes; it takes one"));
    }
    if g.contains(['\u{FE0F}', '\u{200D}']) {
        return Err(format!("glyph `{g}` carries a variation selector or a joiner, which leave its width to the font"));
    }
    if g.contains(|c| matches!(c, '\u{1F3FB}'..='\u{1F3FF}' | '\u{1F1E6}'..='\u{1F1FF}')) {
        return Err(format!("glyph `{g}` carries an emoji modifier or a regional indicator, which leave its width to the font"));
    }
    match cell_width(g) {
        1 => Ok(intern(g)),
        2 if g.chars().all(emoji_presentation) => Ok(intern(g)),
        _ => Err(format!("glyph `{g}` is neither one column wide nor an emoji drawn two columns wide")),
    }
}

/// `[[waypoints]]`, its key on line `line`: one to sixteen tables, each a
/// glyph and an optional fg.
fn waypoints(src: &str, line: usize, v: &Leaf) -> Result<Waypoints, Vec<ThemeError>> {
    let Leaf::Array(entries) = v else {
        return Err(vec![ThemeError::at(line, format!("`waypoints` is {}; it is an array of tables, `[[waypoints]]`", v.kind()))]);
    };
    let mut errs = Vec::new();
    let mut out = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        let (at, entry) = (line_of(src, e.span().start), i + 1);
        if entry > Waypoints::MAX {
            errs.push(ThemeError::at(at, format!("waypoints takes 1 to {} entries; this is entry {entry}", Waypoints::MAX)));
            break;
        }
        let Leaf::Table(keys) = e.get_ref() else {
            errs.push(ThemeError::at(at, format!("waypoints entry {entry} is {}; each is a table", e.get_ref().kind())));
            continue;
        };
        let (mut glyph, mut fg) = (None, None);
        for (k, val) in keys {
            let n = line_of(src, k.span().start);
            let Leaf::Str(s) = val.get_ref() else {
                errs.push(ThemeError::at(n, format!("`{}` is {} (waypoints entry {entry}); a value is a double-quoted string", k.get_ref(), val.get_ref().kind())));
                continue;
            };
            match k.get_ref().as_str() {
                "glyph" => match waypoint_glyph(s) {
                    Ok(g) => glyph = Some(g),
                    Err(e) => errs.push(ThemeError::at(n, format!("{e} (waypoints entry {entry})"))),
                },
                "fg" => match Rgb::from_hex(s) {
                    Some(c) => fg = Some(c),
                    None => errs.push(ThemeError::at(n, format!("`{s}` is not a #rrggbb colour (waypoints entry {entry}, key `fg`)"))),
                },
                other => errs.push(ThemeError::at(n, format!("`{other}` is not a waypoint key; an entry takes glyph and fg (waypoints entry {entry})"))),
            }
        }
        match glyph {
            Some(glyph) => out.push(Waypoint { glyph, fg }),
            None if keys.iter().all(|(k, _)| k.get_ref() != "glyph") => errs.push(ThemeError::at(at, format!("waypoints entry {entry} has no glyph"))),
            None => {}
        }
    }
    if entries.is_empty() {
        errs.push(ThemeError::at(line, format!("waypoints takes 1 to {} entries; it has none", Waypoints::MAX)));
    }
    match Waypoints::new(&out) {
        Some(w) if errs.is_empty() => Ok(w),
        _ => Err(errs),
    }
}

/// Every error in `src`, empty when it is a valid theme.
pub fn validate(src: &str) -> Vec<ThemeError> {
    parse(src).err().unwrap_or_default()
}

/// A theme from the text of a theme file.
pub fn parse(src: &str) -> Result<Theme, Vec<ThemeError>> {
    let top: Top = toml::from_str(src).map_err(|e| vec![syntax(src, &e)])?;
    let mut errs = Vec::new();
    let (mut name, mut label, mut kind, mut background) = (None, None, None, Background::Terminal);
    let mut slots: [Option<Rgb>; 10] = [None; 10];
    let mut overrides = Overrides::default();
    let mut decor = Decor::default();
    let mut grad: Option<(usize, Gradient)> = None;
    let (mut border_at, mut glyphs_at) = (None, None);
    let mut targets = Targets::default();
    let mut words = Words::default();

    for (k, v) in top.0 {
        let (n, key) = (line_of(src, k.span().start), k.get_ref().as_str());
        if key == "waypoints" {
            match waypoints(src, n, &v) {
                Ok(w) => decor.waypoints = Some(w),
                Err(e) => errs.extend(e),
            }
            continue;
        }
        let value = match &v {
            Leaf::Str(s) => s.as_str(),
            // The one dispatch point for tables, by name.
            Leaf::Table(entries) => {
                match key {
                    "target" => target_tables(src, entries, &mut targets, &mut errs),
                    "words" => words_table(src, n, entries, &mut words, &mut errs),
                    _ => errs.push(ThemeError::at(n, format!("`{key}` is a table; the tables are `[target.<name>]`, `[words]` and `[[waypoints]]`"))),
                }
                continue;
            }
            other => {
                errs.push(ThemeError::at(n, format!("`{key}` is {}; a value is a double-quoted string", other.kind())));
                continue;
            }
        };
        match key {
            "THEME_NAME" if valid_name(value) => name = Some(value.to_string()),
            "THEME_NAME" => errs.push(ThemeError::at(n, format!("THEME_NAME `{value}` must be lowercase letters, digits and `-`"))),
            "THEME_LABEL" => label = Some(value.to_string()),
            "THEME_KIND" => match value {
                "dark" => kind = Some(Kind::Dark),
                "light" => kind = Some(Kind::Light),
                _ => errs.push(ThemeError::at(n, format!("THEME_KIND `{value}` must be dark or light"))),
            },
            "THEME_BACKGROUND" => match value {
                "terminal" => background = Background::Terminal,
                "fill" => background = Background::Fill,
                _ => errs.push(ThemeError::at(n, format!("THEME_BACKGROUND `{value}` must be terminal or fill"))),
            },
            "THEME_GRADIENT" => match gradient(value) {
                Ok(g) => grad = Some((n, g)),
                Err(e) => errs.push(ThemeError::at(n, e)),
            },
            "THEME_BORDER" => match Border::named(value) {
                Some(b) => {
                    decor.border = Some(b);
                    border_at = Some(n);
                }
                None => errs.push(ThemeError::at(n, format!("THEME_BORDER `{value}` must be one of {}", Border::NAMES.join(", ")))),
            },
            "THEME_BORDER_GLYPHS" | "THEME_RULE_GLYPHS" | "THEME_MARK" | "THEME_PRESENCE" | "THEME_ORNAMENT" => {
                let (count, widest) = match key {
                    "THEME_BORDER_GLYPHS" => (8, 1),
                    "THEME_RULE_GLYPHS" | "THEME_PRESENCE" => (2, 1),
                    "THEME_ORNAMENT" => (1, 2),
                    _ => (1, 1),
                };
                let g = match glyphs(key, value, count, widest) {
                    Ok(g) => g,
                    Err(e) => {
                        errs.push(ThemeError::at(n, e));
                        continue;
                    }
                };
                match key {
                    "THEME_BORDER_GLYPHS" => {
                        decor.border = Some(Border::Glyphs([g[0], g[1], g[2], g[3], g[4], g[5], g[6], g[7]]));
                        glyphs_at = Some(n);
                    }
                    "THEME_RULE_GLYPHS" => decor.rules = Some([g[0], g[1]]),
                    "THEME_PRESENCE" => decor.presence = Some([g[0], g[1]]),
                    "THEME_ORNAMENT" => decor.ornament = Some(g[0]),
                    _ => decor.mark = Some(g[0]),
                }
            }
            k if k.starts_with("THEME_EVENT_") => {
                let suffix = &k["THEME_EVENT_".len()..];
                let Some((_, count)) = Events::KEYS.iter().find(|(n, _)| *n == suffix) else {
                    errs.push(ThemeError::at(n, format!("unknown key `{key}`")));
                    continue;
                };
                match glyphs(key, value, *count, 1) {
                    Ok(g) => {
                        if let Some(slot) = decor.events.slot(suffix) {
                            slot.copy_from_slice(&g);
                        }
                    }
                    Err(e) => errs.push(ThemeError::at(n, e)),
                }
            }
            k if IGNORED.contains(&k) => {}
            _ => {
                let Some(field) = suffix(key) else {
                    errs.push(ThemeError::at(n, format!("unknown key `{key}`")));
                    continue;
                };
                let slot = Slots::NAMES.iter().position(|s| *s == field);
                let pinned = Overrides::NAMES.contains(&field.as_str());
                if slot.is_none() && !pinned {
                    errs.push(ThemeError::at(n, format!("unknown key `{key}`")));
                    continue;
                }
                let Some(rgb) = Rgb::from_hex(value) else {
                    errs.push(ThemeError::at(n, format!("`{value}` is not a #rrggbb colour (key `{key}`)")));
                    continue;
                };
                match slot {
                    Some(p) => slots[p] = Some(rgb),
                    None => {
                        overrides.set(&field, rgb);
                    }
                }
            }
        }
    }

    if let (Some(a), Some(b)) = (border_at, glyphs_at) {
        errs.push(ThemeError::at(a.max(b), "THEME_BORDER and THEME_BORDER_GLYPHS cannot both be set"));
    }
    if let Some((n, _)) = grad.filter(|_| background != Background::Fill) {
        errs.push(ThemeError::at(n, "THEME_GRADIENT needs THEME_BACKGROUND=\"fill\""));
    }
    if name.is_none() {
        errs.push(ThemeError::whole("missing `THEME_NAME`"));
    }
    if kind.is_none() {
        errs.push(ThemeError::whole("missing `THEME_KIND`"));
    }
    for (p, s) in Slots::NAMES.iter().enumerate() {
        if slots[p].is_none() {
            errs.push(ThemeError::whole(format!("missing slot `THEME_{}`", s.to_ascii_uppercase())));
        }
    }
    let (Some(name), Some(kind), true) = (name, kind, errs.is_empty()) else {
        errs.sort_by_key(|e| e.line.unwrap_or(usize::MAX));
        return Err(errs);
    };
    let s: Vec<Rgb> = slots.iter().flatten().copied().collect();
    Ok(Theme {
        label: label.unwrap_or_else(|| name.clone()),
        name,
        kind,
        background,
        slots: Slots { bg: s[0], fg: s[1], dim: s[2], subtle: s[3], accent: s[4], info: s[5], ok: s[6], warn: s[7], err: s[8], alt: s[9] },
        overrides,
        gradient: grad.map(|(_, g)| g),
        decor,
        targets,
        words,
    })
}

/// The `[target.<name>]` tables: each names a target agent-ways knows, and
/// pins its keys to `#rrggbb` colours.
fn target_tables(src: &str, entries: &[(Spanned<String>, Spanned<Leaf>)], targets: &mut Targets, errs: &mut Vec<ThemeError>) {
    for (name, body) in entries {
        let (n, name) = (line_of(src, name.span().start), name.get_ref());
        let Some(known) = target::keys(name) else {
            let names: Vec<&str> = target::TARGETS.iter().map(|(t, _)| *t).collect();
            errs.push(ThemeError::at(n, format!("unknown target `{name}`; the targets are {}", names.join(", "))));
            continue;
        };
        let Leaf::Table(pins) = body.get_ref() else {
            errs.push(ThemeError::at(n, format!("`target.{name}` must be a table")));
            continue;
        };
        for (key, v) in pins {
            let (n, key) = (line_of(src, key.span().start), key.get_ref());
            if !known.contains(&key.as_str()) {
                errs.push(ThemeError::at(n, format!("`{key}` is not a {name} key")));
                continue;
            }
            match v.get_ref() {
                Leaf::Str(s) => match Rgb::from_hex(s) {
                    Some(c) => targets.set(name, key, c),
                    None => errs.push(ThemeError::at(n, format!("`{s}` is not a #rrggbb colour (key `target.{name}.{key}`)"))),
                },
                _ => errs.push(ThemeError::at(n, format!("`target.{name}.{key}` must be a double-quoted #rrggbb colour"))),
            }
        }
    }
}

/// The `[words]` table, its header on line `at`: `spinner` and `done`,
/// each a list of 1 to 12 strings of 1 to 24 characters with no control
/// character.
fn words_table(src: &str, at: usize, entries: &[(Spanned<String>, Spanned<Leaf>)], words: &mut Words, errs: &mut Vec<ThemeError>) {
    if entries.is_empty() {
        errs.push(ThemeError::at(at, "`[words]` sets neither spinner nor done"));
    }
    for (key, v) in entries {
        let (n, key) = (line_of(src, key.span().start), key.get_ref());
        let err = |message: String| ThemeError::at(n, message);
        if !Words::KEYS.contains(&key.as_str()) {
            errs.push(err(format!("unknown key `words.{key}`; the keys are spinner and done")));
            continue;
        }
        let strings: Option<Vec<String>> = match v.get_ref() {
            Leaf::Array(items) => items.iter().map(|i| if let Leaf::Str(s) = i.get_ref() { Some(s.clone()) } else { None }).collect(),
            _ => None,
        };
        let Some(list) = strings else {
            errs.push(err(format!("`words.{key}` must be an array of double-quoted strings")));
            continue;
        };
        match Words::check(&list) {
            Ok(()) if key == "spinner" => words.spinner = list,
            Ok(()) => words.done = list,
            Err(e) => errs.push(err(format!("`words.{key}` {e}"))),
        }
    }
}

/// The theme as a file `parse` reads back to an equal theme, in the dotfiles
/// palette layout.
pub fn to_text(t: &Theme) -> String {
    let kind = if t.kind == Kind::Dark { "dark" } else { "light" };
    let bg = if t.background == Background::Fill { "fill" } else { "terminal" };
    // The label is the one free-text field; toml quotes and escapes it.
    let label = toml::Value::String(t.label.clone());
    let mut out = format!("THEME_NAME=\"{}\"\nTHEME_LABEL={label}\nTHEME_KIND=\"{kind}\"\nTHEME_BACKGROUND=\"{bg}\"\n", t.name);
    if let Some(g) = &t.gradient {
        let stops: Vec<String> = g.stops().iter().map(|c| c.hex()).collect();
        out.push_str(&format!("THEME_GRADIENT=\"{} {}\"\n", g.axis.name(), stops.join(" ")));
    }
    out.push('\n');
    for n in Slots::NAMES {
        if let Some(c) = t.slots.get(n) {
            out.push_str(&format!("THEME_{}=\"{}\"\n", n.to_ascii_uppercase(), c.hex()));
        }
    }
    let pinned: Vec<_> = Overrides::NAMES.iter().filter_map(|n| t.overrides.get(n).map(|c| (n, c))).collect();
    if !pinned.is_empty() {
        out.push('\n');
        for (n, c) in pinned {
            out.push_str(&format!("THEME_{}=\"{}\"\n", n.to_ascii_uppercase(), c.hex()));
        }
    }
    let d = &t.decor;
    let mut chrome: Vec<(String, String)> = Vec::new();
    match d.border {
        Some(Border::Glyphs(g)) => chrome.push(("BORDER_GLYPHS".into(), g.concat())),
        Some(b) => chrome.push(("BORDER".into(), b.name().unwrap_or("plain").to_string())),
        None => {}
    }
    if let Some(g) = d.rules {
        chrome.push(("RULE_GLYPHS".into(), g.concat()));
    }
    if let Some(g) = d.mark {
        chrome.push(("MARK".into(), g.to_string()));
    }
    if let Some(g) = d.presence {
        chrome.push(("PRESENCE".into(), g.concat()));
    }
    if let Some(g) = d.ornament {
        chrome.push(("ORNAMENT".into(), g.to_string()));
    }
    for (k, _) in Events::KEYS {
        let (mine, default) = (d.events.get(k), Events::DEFAULT.get(k));
        if let Some(g) = mine.filter(|g| Some(g) != default.as_ref()) {
            chrome.push((format!("EVENT_{k}"), g.concat()));
        }
    }
    if !chrome.is_empty() {
        out.push('\n');
        for (k, v) in chrome {
            // A glyph may be a quote or a backslash; toml escapes it.
            out.push_str(&format!("THEME_{k}={}\n", toml::Value::String(v)));
        }
    }
    // Tables follow every bare key, as TOML requires.
    if !t.words.is_empty() {
        out.push_str("\n[words]\n");
        for key in Words::KEYS {
            let list = t.words.get(key).unwrap_or_default();
            if !list.is_empty() {
                let quoted: Vec<String> = list.iter().map(|w| toml::Value::String(w.clone()).to_string()).collect();
                out.push_str(&format!("{key} = [{}]\n", quoted.join(", ")));
            }
        }
    }
    for name in t.targets.names() {
        out.push_str(&format!("\n[target.{name}]\n"));
        for (k, c) in t.targets.pins(name) {
            out.push_str(&format!("{k}=\"{}\"\n", c.hex()));
        }
    }
    for w in d.waypoints.iter().flat_map(|w| w.as_slice()) {
        out.push_str(&format!("\n[[waypoints]]\nglyph = {}\n", toml::Value::String(w.glyph.to_string())));
        if let Some(c) = w.fg {
            out.push_str(&format!("fg = \"{}\"\n", c.hex()));
        }
    }
    out
}
