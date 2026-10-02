//! A YAML text editor that changes only the keys it is told to (ADR-503 §6).
//!
//! `serde_yaml` round trips drop comments and order, so edits are made on the
//! text: a key's line, or its block, is replaced, inserted or removed, and
//! every other line stays byte for byte. Each edit is mirrored on a parsed
//! copy of the document; [`Doc::verify`] parses the edited text and refuses it
//! unless it equals that copy, so a text edit can never change a key it did not
//! name.
//!
//! Handled: block mappings at any indent, quoted keys, inline comments on a
//! scalar line (kept when the scalar changes), block sequences at the key's
//! indent or deeper, CRLF line endings. A value written in flow style (`{…}`,
//! `[…]`) or as a block scalar is replaced whole when a key inside it changes.

use serde_yaml::{Mapping, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum EditError {
    /// The text does not parse; `line` is 1-based.
    Parse { line: Option<usize>, message: String },
    /// The document's root is not a mapping.
    NotMapping,
    /// The edited text did not parse back to the intended document.
    RoundTrip(String),
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditError::Parse { line: Some(l), message } => write!(f, "line {l}: {message}"),
            EditError::Parse { line: None, message } => f.write_str(message),
            EditError::NotMapping => f.write_str("the document is not a mapping"),
            EditError::RoundTrip(m) => write!(f, "the edit did not round-trip ({m})"),
        }
    }
}

impl std::error::Error for EditError {}

/// Parse settings text. Empty, comment-only and `null` documents are an empty
/// mapping; any other non-mapping root is an error.
pub fn parse(text: &str) -> Result<Value, EditError> {
    if text.trim().is_empty() {
        return Ok(Value::Mapping(Mapping::new()));
    }
    match serde_yaml::from_str::<Value>(text) {
        Ok(Value::Null) => Ok(Value::Mapping(Mapping::new())),
        Ok(v @ Value::Mapping(_)) => Ok(v),
        Ok(_) => Err(EditError::NotMapping),
        Err(e) => Err(EditError::Parse { line: e.location().map(|l| l.line()), message: e.to_string() }),
    }
}

/// An editable document: its lines and the parsed value they must equal.
#[derive(Debug, Clone)]
pub struct Doc {
    lines: Vec<String>,
    crlf: bool,
    value: Value,
}

#[derive(Debug, Clone)]
struct Entry {
    line: usize,
    /// Exclusive end: one past the entry's last content line.
    end: usize,
    indent: usize,
    key_raw: String,
    inline: String,
    /// Whitespace and comment after the inline value, or empty.
    comment: String,
}

enum Style {
    Null,
    Inline,
    BlockMap(usize),
    BlockSeq,
}

#[derive(Clone, Copy)]
struct Block {
    start: usize,
    end: usize,
    indent: usize,
}

impl Doc {
    pub fn parse(text: &str) -> Result<Doc, EditError> {
        let crlf = text.contains("\r\n");
        let t = if crlf { text.replace("\r\n", "\n") } else { text.to_string() };
        let value = parse(&t)?;
        Ok(Doc { lines: t.lines().map(str::to_string).collect(), crlf, value })
    }

    /// The parsed document the text must equal.
    pub fn value(&self) -> &Value {
        &self.value
    }

    pub fn text(&self) -> String {
        if self.lines.is_empty() {
            return String::new();
        }
        let mut s = self.lines.join("\n");
        s.push('\n');
        if self.crlf {
            s = s.replace('\n', "\r\n");
        }
        s
    }

    pub fn get(&self, path: &[String]) -> Option<&Value> {
        value_at(&self.value, path)
    }

    /// Parse the text and require it to equal the intended document.
    pub fn verify(&self) -> Result<(), EditError> {
        let got = parse(&self.text()).map_err(|e| EditError::RoundTrip(e.to_string()))?;
        if got == self.value {
            Ok(())
        } else {
            Err(EditError::RoundTrip("the parsed result differs from the intended one".into()))
        }
    }

    /// Set `path` to `v`. A mapping value replaces the mapping there: keys it
    /// lacks are removed, and keys it shares are edited in place.
    pub fn set(&mut self, path: &[String], v: &Value) -> Result<(), EditError> {
        self.sync(path, Some(v))
    }

    /// Remove `path`. A mapping left empty by the removal is removed too.
    /// Comment lines inside a removed block stay.
    pub fn unset(&mut self, path: &[String]) -> Result<bool, EditError> {
        let had = self.get(path).is_some();
        self.sync(path, None)?;
        Ok(had)
    }

    /// The 1-based line a key path starts on, when it is written in block style.
    pub fn line_of(&self, path: &[String]) -> Option<usize> {
        let mut block = self.root_block();
        let mut found = None;
        for seg in path {
            let e = self.find_in(block, seg)?;
            found = Some(e.line + 1);
            match self.style(&e) {
                Style::BlockMap(ci) => block = Block { start: e.line + 1, end: e.end, indent: ci },
                _ => return found,
            }
        }
        found
    }

    fn sync(&mut self, path: &[String], desired: Option<&Value>) -> Result<(), EditError> {
        let cur = self.get(path).cloned();
        if cur.as_ref() == desired {
            return Ok(());
        }
        if path.is_empty() {
            let Some(Value::Mapping(dm)) = desired else { return Err(EditError::NotMapping) };
            let cm = match &cur {
                Some(Value::Mapping(m)) => m.clone(),
                _ => Mapping::new(),
            };
            for (k, v) in dm {
                let k = key_string(k)?;
                self.sync(&[k], Some(v))?;
            }
            for k in cm.keys() {
                let k = key_string(k)?;
                if !dm.contains_key(k.as_str()) {
                    self.sync(&[k], None)?;
                }
            }
            return Ok(());
        }
        if self.flow_root() {
            match desired {
                Some(d) => v_set(&mut self.value, path, d.clone()),
                None => v_remove(&mut self.value, path),
            }
            self.rerender_root();
            return Ok(());
        }
        match desired {
            None => {
                let mut next = self.value.clone();
                v_remove(&mut next, path);
                self.text_remove(path, &next);
                self.value = next;
            }
            Some(d) => {
                if let (Some(Value::Mapping(cm)), Value::Mapping(dm)) = (&cur, d) {
                    if !dm.is_empty() && self.is_block_map(path) {
                        for (k, v) in dm {
                            let mut p = path.to_vec();
                            p.push(key_string(k)?);
                            self.sync(&p, Some(v))?;
                        }
                        for k in cm.keys() {
                            let k = key_string(k)?;
                            if !dm.contains_key(k.as_str()) {
                                let mut p = path.to_vec();
                                p.push(k);
                                self.sync(&p, None)?;
                            }
                        }
                        return Ok(());
                    }
                }
                let mut next = self.value.clone();
                v_set(&mut next, path, d.clone());
                self.text_set(path, d, &next);
                self.value = next;
            }
        }
        Ok(())
    }

    // ── text operations ─────────────────────────────────────────

    fn text_set(&mut self, path: &[String], d: &Value, next: &Value) {
        let mut block = self.root_block();
        for (idx, seg) in path.iter().enumerate() {
            let last = idx + 1 == path.len();
            match self.find_in(block, seg) {
                Some(e) => {
                    if last {
                        self.replace_entry(&e, d);
                        return;
                    }
                    match self.style(&e) {
                        Style::BlockMap(ci) => block = Block { start: e.line + 1, end: e.end, indent: ci },
                        Style::Null => {
                            let lines = render_path(&path[idx + 1..], d, e.indent + 2);
                            self.lines.splice(e.end..e.end, lines);
                            return;
                        }
                        Style::Inline | Style::BlockSeq => {
                            let sub = value_at(next, &path[..=idx]).cloned().unwrap_or(Value::Null);
                            self.replace_entry(&e, &sub);
                            return;
                        }
                    }
                }
                None => {
                    let lines = render_path(&path[idx..], d, block.indent);
                    let at = if idx == 0 { self.lines.len() } else { block.end };
                    self.lines.splice(at..at, lines);
                    return;
                }
            }
        }
    }

    fn text_remove(&mut self, path: &[String], next: &Value) {
        // Remove the deepest entry that `next` no longer has: the leaf, or the
        // highest ancestor pruned with it. An ancestor written inline is
        // rewritten whole from `next`.
        let mut block = self.root_block();
        for (idx, seg) in path.iter().enumerate() {
            let Some(e) = self.find_in(block, seg) else { return };
            let gone = value_at(next, &path[..=idx]).is_none();
            if gone {
                self.remove_content(e.line, e.end);
                return;
            }
            match self.style(&e) {
                Style::BlockMap(ci) => block = Block { start: e.line + 1, end: e.end, indent: ci },
                _ => {
                    let sub = value_at(next, &path[..=idx]).cloned().unwrap_or(Value::Null);
                    self.replace_entry(&e, &sub);
                    return;
                }
            }
        }
    }

    /// Drop the content and blank lines of `[a, b)`, keeping comment lines.
    fn remove_content(&mut self, a: usize, b: usize) {
        let kept: Vec<String> = self.lines[a..b].iter().filter(|l| is_comment(l)).cloned().collect();
        self.lines.splice(a..b, kept);
    }

    fn replace_entry(&mut self, e: &Entry, d: &Value) {
        let simple = e.end == e.line + 1 && !e.inline.starts_with(['|', '>']);
        if simple {
            if let Some(s) = inline_scalar(d) {
                self.lines[e.line] = format!("{}{}: {s}{}", " ".repeat(e.indent), e.key_raw, e.comment);
                return;
            }
        }
        let new = render_entry(&e.key_raw, d, e.indent, &e.comment);
        self.lines.splice(e.line..e.end, new);
    }

    fn rerender_root(&mut self) {
        let mut out = Vec::new();
        if let Value::Mapping(m) = &self.value {
            for (k, v) in m {
                out.extend(render_entry(&quote(&key_string(k).unwrap_or_default()), v, 0, ""));
            }
        }
        self.lines = out;
    }

    // ── structure ───────────────────────────────────────────────

    fn first_content(&self) -> Option<&String> {
        self.lines.iter().find(|l| !is_blank(l) && !is_comment(l) && l.trim() != "---")
    }

    fn flow_root(&self) -> bool {
        self.first_content().is_some_and(|l| l.trim_start().starts_with(['{', '[']))
    }

    fn root_block(&self) -> Block {
        let indent = self.first_content().map(|l| indent_of(l)).unwrap_or(0);
        Block { start: 0, end: self.lines.len(), indent }
    }

    fn is_block_map(&self, path: &[String]) -> bool {
        let mut block = self.root_block();
        let mut style = None;
        for seg in path {
            let Some(e) = self.find_in(block, seg) else { return false };
            let s = self.style(&e);
            if let Style::BlockMap(ci) = s {
                block = Block { start: e.line + 1, end: e.end, indent: ci };
            }
            style = Some(s);
        }
        matches!(style, Some(Style::BlockMap(_)))
    }

    fn find_in(&self, b: Block, key: &str) -> Option<Entry> {
        (b.start..b.end).find_map(|i| {
            let l = &self.lines[i];
            if is_blank(l) || is_comment(l) || indent_of(l) != b.indent {
                return None;
            }
            let (raw, _) = split_key(&l[b.indent..])?;
            (unquote(&raw) == key).then(|| self.entry_at(i, b.indent, b.end))
        })
    }

    fn entry_at(&self, i: usize, indent: usize, block_end: usize) -> Entry {
        let (key_raw, rest) = split_key(&self.lines[i][indent..]).unwrap_or_default();
        let (inline, comment) = split_comment(&rest);
        let mut last = i;
        let mut j = i + 1;
        while j < block_end {
            let l = &self.lines[j];
            if is_blank(l) || is_comment(l) {
                j += 1;
                continue;
            }
            let ind = indent_of(l);
            if ind > indent || (ind == indent && inline.is_empty() && is_seq_item(&l[ind..])) {
                last = j;
                j += 1;
            } else {
                break;
            }
        }
        Entry { line: i, end: last + 1, indent, key_raw, inline, comment }
    }

    fn style(&self, e: &Entry) -> Style {
        if !e.inline.is_empty() {
            return Style::Inline;
        }
        let first = (e.line + 1..e.end).map(|j| &self.lines[j]).find(|l| !is_blank(l) && !is_comment(l));
        match first {
            None => Style::Null,
            Some(l) if is_seq_item(&l[indent_of(l)..]) => Style::BlockSeq,
            Some(l) => Style::BlockMap(indent_of(l)),
        }
    }
}

// ── line helpers ────────────────────────────────────────────────

fn is_blank(l: &str) -> bool {
    l.trim().is_empty()
}

fn is_comment(l: &str) -> bool {
    l.trim_start().starts_with('#')
}

fn indent_of(l: &str) -> usize {
    l.len() - l.trim_start_matches(' ').len()
}

fn is_seq_item(t: &str) -> bool {
    t == "-" || t.starts_with("- ")
}

/// Split `key: rest` (the line with its indent removed) into the raw key and
/// the text after the colon. `None` when the line is not a mapping key.
pub(crate) fn split_key(t: &str) -> Option<(String, String)> {
    if t.starts_with(['#', ' ', '?']) || is_seq_item(t) {
        return None;
    }
    let colon_ok = |rest: &str| rest.is_empty() || rest.starts_with([' ', '\t']);
    if let Some(q) = t.chars().next().filter(|c| *c == '"' || *c == '\'') {
        let mut esc = false;
        for (i, c) in t.char_indices().skip(1) {
            if q == '"' && c == '\\' && !esc {
                esc = true;
                continue;
            }
            if c == q && !esc {
                let after = t[i + 1..].trim_start_matches([' ', '\t']);
                let rest = after.strip_prefix(':')?;
                return colon_ok(rest).then(|| (t[..=i].to_string(), rest.to_string()));
            }
            esc = false;
        }
        return None;
    }
    let mut from = 0;
    while let Some(p) = t[from..].find(':') {
        let at = from + p;
        let rest = &t[at + 1..];
        if colon_ok(rest) {
            let key = t[..at].trim_end();
            if key.is_empty() || key.contains(" #") {
                return None;
            }
            return Some((key.to_string(), rest.to_string()));
        }
        from = at + 1;
    }
    None
}

/// Split the text after a key's colon into its value and its trailing
/// comment (with the whitespace before it).
pub(crate) fn split_comment(rest: &str) -> (String, String) {
    let start = rest.len() - rest.trim_start().len();
    let body = &rest[start..];
    let mut scan_from = 0;
    if let Some(q) = body.chars().next().filter(|c| *c == '"' || *c == '\'') {
        let mut esc = false;
        for (i, c) in body.char_indices().skip(1) {
            if q == '"' && c == '\\' && !esc {
                esc = true;
                continue;
            }
            if c == q && !esc {
                scan_from = i + 1;
                break;
            }
            esc = false;
        }
    }
    let bytes = body.as_bytes();
    let hash = (scan_from..bytes.len()).find(|&i| bytes[i] == b'#' && (i == 0 || bytes[i - 1] == b' ' || bytes[i - 1] == b'\t'));
    match hash {
        Some(h) => {
            let value = body[..h].trim_end();
            (value.to_string(), body[value.len()..].to_string())
        }
        None => (body.trim_end().to_string(), String::new()),
    }
}

pub(crate) fn unquote(raw: &str) -> String {
    if raw.len() >= 2 && raw.starts_with('"') && raw.ends_with('"') {
        serde_yaml::from_str::<String>(raw).unwrap_or_else(|_| raw[1..raw.len() - 1].to_string())
    } else if raw.len() >= 2 && raw.starts_with('\'') && raw.ends_with('\'') {
        raw[1..raw.len() - 1].replace("''", "'")
    } else {
        raw.to_string()
    }
}

fn key_string(k: &Value) -> Result<String, EditError> {
    match k {
        Value::String(s) => Ok(s.clone()),
        Value::Bool(b) => Ok(b.to_string()),
        Value::Number(n) => Ok(n.to_string()),
        _ => Err(EditError::RoundTrip("a mapping key is not a scalar".into())),
    }
}

// ── value helpers ───────────────────────────────────────────────

/// The key of `m` a path segment names: the text key, or else a scalar key
/// written the same way (`123:` is named by `"123"`).
fn key_of(m: &Mapping, seg: &str) -> Option<Value> {
    if m.contains_key(seg) {
        return Some(Value::String(seg.to_string()));
    }
    m.keys().find(|k| !k.is_string() && key_string(k).is_ok_and(|t| t == seg)).cloned()
}

pub fn value_at<'a>(v: &'a Value, path: &[String]) -> Option<&'a Value> {
    let mut cur = v;
    for seg in path {
        let m = cur.as_mapping()?;
        cur = m.get(key_of(m, seg)?)?;
    }
    Some(cur)
}

fn v_set(root: &mut Value, path: &[String], v: Value) {
    let mut cur = root;
    for seg in &path[..path.len() - 1] {
        if !cur.is_mapping() {
            *cur = Value::Mapping(Mapping::new());
        }
        let m = cur.as_mapping_mut().expect("just made a mapping");
        let k = key_of(m, seg).unwrap_or_else(|| Value::String(seg.clone()));
        if !m.get(&k).is_some_and(|x| x.is_mapping()) {
            m.insert(k.clone(), Value::Mapping(Mapping::new()));
        }
        cur = m.get_mut(&k).expect("just inserted");
    }
    if !cur.is_mapping() {
        *cur = Value::Mapping(Mapping::new());
    }
    let m = cur.as_mapping_mut().expect("mapping");
    let k = key_of(m, &path[path.len() - 1]).unwrap_or_else(|| Value::String(path[path.len() - 1].clone()));
    m.insert(k, v);
}

/// Remove `path`, then every ancestor mapping the removal left empty.
fn v_remove(root: &mut Value, path: &[String]) {
    fn go(cur: &mut Value, path: &[String]) -> bool {
        let Some(m) = cur.as_mapping_mut() else { return false };
        let Some(k) = key_of(m, &path[0]) else { return m.is_empty() };
        if path.len() == 1 {
            m.remove(&k);
        } else if let Some(child) = m.get_mut(&k) {
            if go(child, &path[1..]) {
                m.remove(&k);
            }
        }
        m.is_empty()
    }
    if !path.is_empty() {
        go(root, path);
    }
}

// ── rendering ───────────────────────────────────────────────────

/// A scalar, or an empty collection, as one line of YAML.
fn inline_scalar(v: &Value) -> Option<String> {
    match v {
        Value::Null => Some("null".into()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(_) => serde_yaml::to_string(v).ok().map(|s| s.trim().to_string()),
        Value::String(s) => Some(quote(s)),
        Value::Sequence(s) if s.is_empty() => Some("[]".into()),
        Value::Mapping(m) if m.is_empty() => Some("{}".into()),
        Value::Tagged(t) => inline_scalar(&t.value),
        _ => None,
    }
}

/// Write a string bare when YAML reads it back as the same string, quoted
/// otherwise.
pub fn quote(s: &str) -> String {
    let bare = !s.is_empty()
        && s.trim() == s
        && !s.starts_with(['~', '*', '&', '!', '%', '@', '`', '\'', '"', '[', ']', '{', '}', '#', '-', '?', '|', '>', ',', ':'])
        && !s.contains(": ")
        && !s.contains(" #")
        && !s.ends_with(':')
        && !s.chars().any(|c| c.is_control())
        && serde_yaml::from_str::<Value>(s).ok() == Some(Value::String(s.to_string()));
    if bare {
        return s.to_string();
    }
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn render_entry(key: &str, v: &Value, indent: usize, comment: &str) -> Vec<String> {
    let ind = " ".repeat(indent);
    if let Some(s) = inline_scalar(v) {
        return vec![format!("{ind}{key}: {s}{comment}")];
    }
    let mut out = vec![format!("{ind}{key}:{comment}")];
    match v {
        Value::Mapping(m) => {
            for (k, cv) in m {
                out.extend(render_entry(&quote(&key_string(k).unwrap_or_default()), cv, indent + 2, ""));
            }
        }
        Value::Sequence(s) => {
            for item in s {
                out.extend(render_item(item, indent + 2));
            }
        }
        _ => {}
    }
    out
}

fn render_item(v: &Value, indent: usize) -> Vec<String> {
    let ind = " ".repeat(indent);
    if let Some(s) = inline_scalar(v) {
        return vec![format!("{ind}- {s}")];
    }
    match v {
        Value::Mapping(m) => {
            let mut out = Vec::new();
            for (k, cv) in m {
                out.extend(render_entry(&quote(&key_string(k).unwrap_or_default()), cv, indent + 2, ""));
            }
            if let Some(first) = out.first_mut() {
                *first = format!("{ind}- {}", &first[indent + 2..]);
            }
            out
        }
        Value::Sequence(s) => {
            let mut out = vec![format!("{ind}-")];
            for item in s {
                out.extend(render_item(item, indent + 2));
            }
            out
        }
        _ => vec![format!("{ind}- null")],
    }
}

/// Render `path` as nested keys down to `v`.
fn render_path(path: &[String], v: &Value, indent: usize) -> Vec<String> {
    if path.len() == 1 {
        return render_entry(&quote(&path[0]), v, indent, "");
    }
    let mut out = vec![format!("{}{}:", " ".repeat(indent), quote(&path[0]))];
    out.extend(render_path(&path[1..], v, indent + 2));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Vec<String> {
        s.split('.').map(str::to_string).collect()
    }

    fn y(s: &str) -> Value {
        serde_yaml::from_str(s).unwrap()
    }

    fn edit(text: &str, f: impl FnOnce(&mut Doc)) -> String {
        let mut d = Doc::parse(text).unwrap();
        f(&mut d);
        d.verify().unwrap();
        d.text()
    }

    #[test]
    fn a_scalar_set_changes_one_line_and_keeps_its_comment() {
        let src = "# my config\nlanguage: es   # spanish\n\nsemantic_fire_probability: 0.5  # tuned\n# tail\n";
        let out = edit(src, |d| d.set(&p("semantic_fire_probability"), &y("0.4")).unwrap());
        assert_eq!(out, "# my config\nlanguage: es   # spanish\n\nsemantic_fire_probability: 0.4  # tuned\n# tail\n");
    }

    #[test]
    fn a_missing_key_is_appended_and_nested_keys_land_in_their_block() {
        let out = edit("language: es\n", |d| d.set(&p("near_miss_margin"), &y("0.1")).unwrap());
        assert_eq!(out, "language: es\nnear_miss_margin: 0.1\n");
        let src = "refire_presets:\n    normal: 0.2  # mine\n# after\nlanguage: es\n";
        let out = edit(src, |d| d.set(&p("refire_presets.rare"), &y("0.3")).unwrap());
        assert_eq!(out, "refire_presets:\n    normal: 0.2  # mine\n    rare: 0.3\n# after\nlanguage: es\n");
        let out = edit("mode: off\n", |d| d.set(&p("profiles.anthropic.model"), &y("x")).unwrap());
        assert_eq!(out, "mode: off\nprofiles:\n  anthropic:\n    model: x\n");
        let out = edit("profiles:\nmode: off\n", |d| d.set(&p("profiles.anthropic.model"), &y("x")).unwrap());
        assert_eq!(out, "profiles:\n  anthropic:\n    model: x\nmode: off\n");
    }

    #[test]
    fn unset_prunes_empty_parents_and_keeps_comments() {
        let src = "# overlay\nlanguage: en\n\nways:\n  # keep me\n  meta/introspection: false\n\nparent_boost_floor: 0.40\n";
        let out = edit(src, |d| assert!(d.unset(&p("ways.meta/introspection")).unwrap()));
        assert_eq!(out, "# overlay\nlanguage: en\n\n  # keep me\n\nparent_boost_floor: 0.40\n");
        let out = edit("a: 1\n", |d| assert!(!d.unset(&p("b")).unwrap()));
        assert_eq!(out, "a: 1\n");
    }

    #[test]
    fn a_block_value_replaces_a_scalar_and_a_long_form_entry() {
        let src = "ways:\n    itops/incident:\n        enabled: false\n    other: false\n";
        let out = edit(src, |d| d.set(&p("ways.itops/incident"), &y("false")).unwrap());
        assert_eq!(out, "ways:\n    itops/incident: false\n    other: false\n");
        let out = edit("targets: []  # none\nx: 1\n", |d| d.set(&p("targets"), &y("[{path: /a, enabled: true}]")).unwrap());
        assert_eq!(out, "targets:  # none\n  - path: /a\n    enabled: true\nx: 1\n");
    }

    #[test]
    fn a_sequence_at_the_key_indent_is_one_entry() {
        let src = "targets:\n- path: /a\n  enabled: true\n# c\nlanguage: es\n";
        let out = edit(src, |d| d.set(&p("targets"), &y("[]")).unwrap());
        assert_eq!(out, "targets: []\n# c\nlanguage: es\n");
    }

    #[test]
    fn flow_values_are_rewritten_whole_and_crlf_survives() {
        let out = edit("ways: {a: false, b: false}\n", |d| {
            d.unset(&p("ways.a")).unwrap();
        });
        assert_eq!(out, "ways:\n  b: false\n");
        let out = edit("a: 1\r\n# n\r\n", |d| d.set(&p("b"), &y("two words")).unwrap());
        assert_eq!(out, "a: 1\r\n# n\r\nb: two words\r\n");
    }

    #[test]
    fn quoted_keys_match_and_odd_strings_quote() {
        let out = edit("\"a/b\": false\n", |d| d.set(&p("a/b"), &y("true")).unwrap());
        assert_eq!(out, "\"a/b\": true\n");
        for s in ["true", "1.5", "~/.claude", "a: b", "x #y", "", " pad", "null"] {
            let out = edit("", |d| d.set(&p("k"), &Value::String(s.into())).unwrap());
            assert_eq!(parse(&out).unwrap().get("k").unwrap(), &Value::String(s.into()), "{s}");
        }
    }

    #[test]
    fn root_sync_and_parse_errors() {
        let mut d = Doc::parse("# head\nengine: a\nmode: off\n").unwrap();
        d.set(&[], &y("{mode: shadow}")).unwrap();
        d.verify().unwrap();
        assert_eq!(d.text(), "# head\nmode: shadow\n");
        assert!(matches!(Doc::parse("a: [\n"), Err(EditError::Parse { line: Some(_), .. })));
        assert_eq!(Doc::parse("- a\n").unwrap_err(), EditError::NotMapping);
        assert_eq!(Doc::parse("# only\n").unwrap().line_of(&p("a")), None);
        assert_eq!(Doc::parse("a:\n  b: 1\n").unwrap().line_of(&p("a.b")), Some(2));
    }

    #[test]
    fn a_scalar_key_is_named_by_its_text() {
        let out = edit("ways:\n  123: false\n  a/b: false\n", |d| assert!(d.unset(&p("ways.123")).unwrap()));
        assert_eq!(out, "ways:\n  a/b: false\n");
    }
}
