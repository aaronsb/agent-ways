//! The theme file format, and its parser and writer.
//!
//! The file is TOML, read by the `toml` crate. Keys and values are read with
//! their spans, so every problem names its line.
//!
//! ```toml
//! # Source or note.
//! name = "nord"            # [a-z0-9-]+
//! label = "Nord"           # optional, defaults to name
//! kind = "dark"            # dark | light
//! background = "fill"      # terminal (default) | fill
//!
//! [slots]                  # all ten required
//! bg = "#2e3440"
//! ...
//!
//! [overrides]              # optional; any derived role
//! muted = "#6b7487"
//! ```

use std::fmt;

use serde::de::{self, Deserialize, Deserializer, IgnoredAny, MapAccess, Visitor};
use toml::Spanned;

use super::model::{Background, Kind, Overrides, Rgb, Slots, Theme};

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
    fn whole(message: impl Into<String>) -> Self {
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

/// A top-level entry: a value, or a table of spanned keys and values.
enum Entry {
    Leaf(Leaf),
    Table(Vec<(Spanned<String>, Spanned<Leaf>)>),
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

struct EntryVisitor;

impl<'de> Visitor<'de> for EntryVisitor {
    type Value = Entry;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a value or a table")
    }
    fn visit_str<E>(self, v: &str) -> Result<Entry, E> {
        Ok(Entry::Leaf(Leaf::Str(v.to_string())))
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Entry, E> {
        LeafVisitor.visit_bool(v).map(Entry::Leaf)
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Entry, E> {
        LeafVisitor.visit_i64(v).map(Entry::Leaf)
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Entry, E> {
        LeafVisitor.visit_u64(v).map(Entry::Leaf)
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Entry, E> {
        LeafVisitor.visit_f64(v).map(Entry::Leaf)
    }
    fn visit_seq<A: de::SeqAccess<'de>>(self, s: A) -> Result<Entry, A::Error> {
        LeafVisitor.visit_seq(s).map(Entry::Leaf)
    }
    fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Entry, A::Error> {
        let mut out = Vec::new();
        while let Some(kv) = m.next_entry::<Spanned<String>, Spanned<Leaf>>()? {
            out.push(kv);
        }
        Ok(Entry::Table(out))
    }
}

impl<'de> Deserialize<'de> for Entry {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Entry, D::Error> {
        d.deserialize_any(EntryVisitor)
    }
}

/// The document's top level, in file order.
struct Top(Vec<(Spanned<String>, Spanned<Entry>)>);

impl<'de> Deserialize<'de> for Top {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Top, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Top;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a theme table")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Top, A::Error> {
                let mut out = Vec::new();
                while let Some(kv) = m.next_entry::<Spanned<String>, Spanned<Entry>>()? {
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

/// Every error in `src`, empty when it is a valid theme.
#[cfg(test)]
pub fn validate(src: &str) -> Vec<ThemeError> {
    parse(src).err().unwrap_or_default()
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
    let value = text.split_once('=').map(|(_, v)| v.trim());
    let hint = match value {
        Some(v) if !v.starts_with('"') => "; values are double-quoted strings",
        _ => "",
    };
    ThemeError::at(n, format!("{first}{hint}"))
}

pub fn parse(src: &str) -> Result<Theme, Vec<ThemeError>> {
    let top: Top = toml::from_str(src).map_err(|e| vec![syntax(src, &e)])?;
    let mut errs = Vec::new();
    let (mut name, mut label, mut kind, mut background) = (None, None, None, Background::Terminal);
    let mut slots: [Option<Rgb>; 10] = [None; 10];
    let mut overrides = Overrides::default();
    let at = |s: &std::ops::Range<usize>| line_of(src, s.start);

    for (k, v) in top.0 {
        let (n, key) = (at(&k.span()), k.get_ref().as_str());
        match v.get_ref() {
            Entry::Table(entries) => {
                if key != "slots" && key != "overrides" {
                    errs.push(ThemeError::at(n, format!("unknown section [{key}] (expected [slots] or [overrides])")));
                    continue;
                }
                for (sk, sv) in entries {
                    let (n, k) = (at(&sk.span()), sk.get_ref().as_str());
                    let rgb = match sv.get_ref() {
                        Leaf::Str(s) => match Rgb::from_hex(s) {
                            Some(c) => c,
                            None => {
                                errs.push(ThemeError::at(n, format!("`{s}` is not a #rrggbb colour (key `{k}`)")));
                                continue;
                            }
                        },
                        Leaf::Other(t) => {
                            errs.push(ThemeError::at(n, format!("`{k}` is {t}; a colour is a double-quoted \"#rrggbb\"")));
                            continue;
                        }
                    };
                    if key == "slots" {
                        match Slots::NAMES.iter().position(|s| *s == k) {
                            Some(p) => slots[p] = Some(rgb),
                            None => errs.push(ThemeError::at(n, format!("unknown slot `{k}`"))),
                        }
                    } else if !overrides.set(k, rgb) {
                        errs.push(ThemeError::at(n, format!("unknown override `{k}`")));
                    }
                }
            }
            Entry::Leaf(Leaf::Other(t)) => errs.push(ThemeError::at(n, format!("`{key}` is {t}; it must be a double-quoted string"))),
            Entry::Leaf(Leaf::Str(v)) => match key {
                "name" if valid_name(v) => name = Some(v.clone()),
                "name" => errs.push(ThemeError::at(n, format!("name `{v}` must be lowercase letters, digits and `-`"))),
                "label" => label = Some(v.clone()),
                "kind" => match v.as_str() {
                    "dark" => kind = Some(Kind::Dark),
                    "light" => kind = Some(Kind::Light),
                    _ => errs.push(ThemeError::at(n, format!("kind `{v}` must be dark or light"))),
                },
                "background" => match v.as_str() {
                    "terminal" => background = Background::Terminal,
                    "fill" => background = Background::Fill,
                    _ => errs.push(ThemeError::at(n, format!("background `{v}` must be terminal or fill"))),
                },
                _ => errs.push(ThemeError::at(n, format!("unknown key `{key}`"))),
            },
        }
    }

    if name.is_none() {
        errs.push(ThemeError::whole("missing `name`"));
    }
    if kind.is_none() {
        errs.push(ThemeError::whole("missing `kind`"));
    }
    for (p, s) in Slots::NAMES.iter().enumerate() {
        if slots[p].is_none() {
            errs.push(ThemeError::whole(format!("missing slot `{s}`")));
        }
    }
    if !errs.is_empty() {
        errs.sort_by_key(|e| e.line.unwrap_or(usize::MAX));
        return Err(errs);
    }
    let s: Vec<Rgb> = slots.iter().map(|c| c.unwrap()).collect();
    let name = name.unwrap();
    Ok(Theme {
        label: label.unwrap_or_else(|| name.clone()),
        name,
        kind: kind.unwrap(),
        background,
        slots: Slots { bg: s[0], fg: s[1], dim: s[2], subtle: s[3], accent: s[4], info: s[5], ok: s[6], warn: s[7], err: s[8], alt: s[9] },
        overrides,
    })
}

/// The theme as a file `parse` reads back to an equal theme.
pub fn to_toml(t: &Theme) -> String {
    let kind = if t.kind == Kind::Dark { "dark" } else { "light" };
    let bg = if t.background == Background::Fill { "fill" } else { "terminal" };
    // The label is the one free-text field; toml quotes and escapes it.
    let label = toml::Value::String(t.label.clone());
    let mut out = format!("name = \"{}\"\nlabel = {label}\nkind = \"{kind}\"\nbackground = \"{bg}\"\n\n[slots]\n", t.name);
    for n in Slots::NAMES {
        out.push_str(&format!("{n} = \"{}\"\n", t.slots.get(n).unwrap().hex()));
    }
    let set: Vec<_> = Overrides::NAMES.iter().filter_map(|n| t.overrides.get(n).map(|c| (n, c))).collect();
    if !set.is_empty() {
        out.push_str("\n[overrides]\n");
        for (n, c) in set {
            out.push_str(&format!("{n} = \"{}\"\n", c.hex()));
        }
    }
    out
}
