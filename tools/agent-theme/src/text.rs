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

use crate::model::{intern, Axis, Background, Border, Decor, Events, Gradient, Kind, Overrides, Rgb, Slots, Theme, Waypoint, Waypoints};

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

/// The document's top level, in file order.
struct Top(Vec<(Spanned<String>, Spanned<Leaf>)>);

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
                while let Some(kv) = m.next_entry::<Spanned<String>, Spanned<Leaf>>()? {
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

/// `value` as `count` glyphs for `key`, each one grapheme one column wide,
/// or up to `widest` columns.
fn glyphs(key: &str, value: &str, count: usize, widest: usize) -> Result<Vec<&'static str>, String> {
    let g: Vec<&str> = value.graphemes(true).collect();
    if g.len() != count {
        let noun = if count == 1 { "glyph" } else { "glyphs" };
        return Err(format!("`{key}` takes {count} {noun}; `{value}` has {}", g.len()));
    }
    for x in &g {
        let w = x.width();
        if w == 0 || w > widest {
            let must = if widest == 1 { "1".to_string() } else { format!("1 to {widest}") };
            return Err(format!("`{key}` glyph `{x}` is {w} columns wide; it must be {must}"));
        }
    }
    Ok(g.into_iter().map(intern).collect())
}

/// A waypoint's glyph: one code point, one column wide, or an emoji whose
/// default presentation is two columns. A variation selector, a joiner or
/// a combining mark would leave the width to the font, so none is taken.
fn waypoint_glyph(g: &str) -> Result<&'static str, String> {
    let mut cs = g.chars();
    let (Some(c), None) = (cs.next(), cs.next()) else {
        return Err(format!("glyph `{g}` is not one character; variation selectors, joiners and combining marks are refused"));
    };
    // unicode-width counts these blocks two columns wide only for code
    // points with Emoji_Presentation=Yes.
    let emoji = matches!(c as u32, 0x231A..=0x23FF | 0x2600..=0x27BF | 0x2B00..=0x2BFF | 0x1F000..=0x1FAFF);
    match unicode_width::UnicodeWidthChar::width(c) {
        Some(1) => Ok(intern(g)),
        Some(2) if emoji => Ok(intern(g)),
        _ => Err(format!("glyph `{g}` is neither one column wide nor an emoji drawn two columns wide")),
    }
}

/// `[[waypoints]]`: one to sixteen tables, each a glyph and an optional fg.
fn waypoints(src: &str, v: &Leaf) -> Result<Waypoints, Vec<ThemeError>> {
    let Leaf::Array(entries) = v else {
        return Err(vec![ThemeError::whole(format!("`waypoints` is {}; it is an array of tables, `[[waypoints]]`", v.kind()))]);
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
        errs.push(ThemeError::whole(format!("waypoints takes 1 to {} entries; it has none", Waypoints::MAX)));
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

    for (k, v) in top.0 {
        let (n, key) = (line_of(src, k.span().start), k.get_ref().as_str());
        if key == "waypoints" {
            match waypoints(src, v.get_ref()) {
                Ok(w) => decor.waypoints = Some(w),
                Err(e) => errs.extend(e),
            }
            continue;
        }
        let value = match v.get_ref() {
            Leaf::Str(s) => s.as_str(),
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
    })
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
    // A table comes after every bare key.
    for w in d.waypoints.iter().flat_map(|w| w.as_slice()) {
        out.push_str(&format!("\n[[waypoints]]\nglyph = {}\n", toml::Value::String(w.glyph.to_string())));
        if let Some(c) = w.fg {
            out.push_str(&format!("fg = \"{}\"\n", c.hex()));
        }
    }
    out
}
