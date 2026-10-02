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

use std::fmt;

use serde::de::{self, Deserialize, Deserializer, IgnoredAny, MapAccess, Visitor};
use toml::Spanned;

use crate::model::{Background, Kind, Overrides, Rgb, Slots, Theme};

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

/// A value as the theme format sees it: a string, or the name of the TOML
/// type it has instead.
enum Leaf {
    Str(String),
    Other(&'static str),
}

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
        while s.next_element::<IgnoredAny>()?.is_some() {}
        Ok(Leaf::Other("an array"))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Leaf, A::Error> {
        while m.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(Leaf::Other("a table or date"))
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

    for (k, v) in top.0 {
        let (n, key) = (line_of(src, k.span().start), k.get_ref().as_str());
        let value = match v.get_ref() {
            Leaf::Str(s) => s.as_str(),
            Leaf::Other(t) => {
                errs.push(ThemeError::at(n, format!("`{key}` is {t}; a value is a double-quoted string")));
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
    })
}

/// The theme as a file `parse` reads back to an equal theme, in the dotfiles
/// palette layout.
pub fn to_text(t: &Theme) -> String {
    let kind = if t.kind == Kind::Dark { "dark" } else { "light" };
    let bg = if t.background == Background::Fill { "fill" } else { "terminal" };
    // The label is the one free-text field; toml quotes and escapes it.
    let label = toml::Value::String(t.label.clone());
    let mut out = format!("THEME_NAME=\"{}\"\nTHEME_LABEL={label}\nTHEME_KIND=\"{kind}\"\nTHEME_BACKGROUND=\"{bg}\"\n\n", t.name);
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
    out
}
