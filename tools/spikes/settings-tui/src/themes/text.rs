//! The theme file format, and its parser and writer.
//!
//! TEMPORARY: this is a hand-written reader for exactly the format below
//! (flat `key = "value"` lines, `[section]` headers, `#` comments). Phase 2
//! replaces it with the `toml` crate (`toml = "0.8"`, features `parse` and
//! `display`); the format is plain TOML so the files do not change.
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

/// `key = "value"` with an optional trailing `# comment`.
fn key_value(line: &str) -> Result<(&str, &str), String> {
    let (k, rest) = line.split_once('=').ok_or("expected `key = \"value\"`")?;
    let rest = rest.trim();
    let body = rest.strip_prefix('"').ok_or("value must be a double-quoted string")?;
    let end = body.find('"').ok_or("unterminated string")?;
    let tail = body[end + 1..].trim();
    if !tail.is_empty() && !tail.starts_with('#') {
        return Err(format!("unexpected text after the value: `{tail}`"));
    }
    Ok((k.trim(), &body[..end]))
}

fn valid_name(n: &str) -> bool {
    !n.is_empty() && n.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Every error in `src`, empty when it is a valid theme.
pub fn validate(src: &str) -> Vec<ThemeError> {
    parse(src).err().unwrap_or_default()
}

pub fn parse(src: &str) -> Result<Theme, Vec<ThemeError>> {
    let mut errs = Vec::new();
    let (mut name, mut label, mut kind, mut background) = (None, None, None, Background::Terminal);
    let mut slots: [Option<Rgb>; 10] = [None; 10];
    let mut overrides = Overrides::default();
    let mut section = "";

    for (i, raw) in src.lines().enumerate() {
        let n = i + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(h) = line.strip_prefix('[') {
            match h.strip_suffix(']').map(str::trim) {
                Some(s @ ("slots" | "overrides")) => section = s,
                Some(s) => {
                    errs.push(ThemeError::at(n, format!("unknown section [{s}] (expected [slots] or [overrides])")));
                    section = "?";
                }
                None => errs.push(ThemeError::at(n, "unterminated section header")),
            }
            continue;
        }
        let (k, v) = match key_value(line) {
            Ok(kv) => kv,
            Err(m) => {
                errs.push(ThemeError::at(n, m));
                continue;
            }
        };
        match section {
            "" => match k {
                "name" if valid_name(v) => name = Some(v.to_string()),
                "name" => errs.push(ThemeError::at(n, format!("name `{v}` must be lowercase letters, digits and `-`"))),
                "label" => label = Some(v.to_string()),
                "kind" => match v {
                    "dark" => kind = Some(Kind::Dark),
                    "light" => kind = Some(Kind::Light),
                    _ => errs.push(ThemeError::at(n, format!("kind `{v}` must be dark or light"))),
                },
                "background" => match v {
                    "terminal" => background = Background::Terminal,
                    "fill" => background = Background::Fill,
                    _ => errs.push(ThemeError::at(n, format!("background `{v}` must be terminal or fill"))),
                },
                _ => errs.push(ThemeError::at(n, format!("unknown key `{k}`"))),
            },
            "slots" | "overrides" => {
                let Some(rgb) = Rgb::from_hex(v) else {
                    errs.push(ThemeError::at(n, format!("`{v}` is not a #rrggbb colour (key `{k}`)")));
                    continue;
                };
                if section == "slots" {
                    match Slots::NAMES.iter().position(|s| *s == k) {
                        Some(p) if slots[p].is_some() => errs.push(ThemeError::at(n, format!("slot `{k}` set twice"))),
                        Some(p) => slots[p] = Some(rgb),
                        None => errs.push(ThemeError::at(n, format!("unknown slot `{k}`"))),
                    }
                } else if !overrides.set(k, rgb) {
                    errs.push(ThemeError::at(n, format!("unknown override `{k}`")));
                }
            }
            _ => {} // inside an unknown section, already reported
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
    let mut out = format!("name = \"{}\"\nlabel = \"{}\"\nkind = \"{kind}\"\nbackground = \"{bg}\"\n\n[slots]\n", t.name, t.label);
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
