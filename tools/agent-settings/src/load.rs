//! Per-section checking and layered loading (ADR-503 §3-5).
//!
//! A file is parsed once. Each top-level key belongs to a section, and the
//! section is the fallback unit, except in a per-entry section, where each
//! entry of its mapping or item of its list is. A unit any of whose values
//! fails the schema is dropped from that file's layer, so its keys resolve
//! from the layers beneath, which end at canonical. Every other unit loads as
//! written. A diagnostic names the file, line and section, and is built only
//! when something fails. A top-level key no section owns is reported and
//! changes nothing else.
//!
//! Switches that turn something off fail closed (the ADR-503 addendum): a key
//! with a fail-closed reading takes it when its unit fails, and when its file
//! does not parse the reading is taken from what can be salvaged of the text.

use crate::schema::{KeySpec, LayerScope, Schema, SectionSpec};
use crate::yaml_edit::{self, Doc};
use serde_yaml::{Mapping, Value};
use std::path::{Path, PathBuf};

/// One problem in one file. `fallback` is set when it dropped a unit (a
/// section, or one entry of a per-entry section) from the file's layer, or,
/// with no unit, when the whole file did not parse.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub file: Option<PathBuf>,
    pub line: Option<usize>,
    pub section: Option<String>,
    /// The unit that fell back: the section, `<section>.<entry>`, or
    /// `<section>[<index>]`.
    pub unit: Option<String>,
    pub key: Option<String>,
    pub message: String,
    pub fallback: bool,
    /// The command that repairs the section when `fix` cannot.
    pub repair: Option<String>,
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let file = self.file.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "<input>".into());
        write!(f, "{file}")?;
        if let Some(l) = self.line {
            write!(f, ":{l}")?;
        }
        write!(f, ":")?;
        if let Some(s) = &self.section {
            write!(f, " [{s}]")?;
        }
        if let Some(k) = &self.key {
            write!(f, " {k}")?;
        }
        if self.section.is_some() || self.key.is_some() {
            write!(f, ":")?;
        }
        write!(f, " {}", self.message)
    }
}

impl Finding {
    /// Whether the whole file failed to parse.
    pub fn is_parse_failure(&self) -> bool {
        self.fallback && self.unit.is_none()
    }

    /// The one-line stderr form: the finding, what it dropped, and the repair.
    pub fn diagnostic(&self, tool: &str) -> String {
        let fix = |s: &str| match &self.repair {
            Some(r) => format!("{r} repairs it"),
            None => format!("`ways settings fix {s}` repairs it"),
        };
        match (&self.section, &self.unit, self.fallback) {
            (Some(s), Some(u), true) if u != s => format!(
                "[{tool}] settings: {self}; entry {u} is ignored, so it resolves from the layers beneath \
                 (a switch stays off). `ways settings lint` lists the findings, {}",
                fix(s)
            ),
            (Some(s), _, true) => format!(
                "[{tool}] settings: {self}; section {s} is ignored in this file, so its keys resolve from the \
                 layers beneath, ending at canonical (a switch stays off). `ways settings lint` lists the findings, {}",
                fix(s)
            ),
            _ => format!("[{tool}] settings: {self}"),
        }
    }
}

/// A check failure: the section and unit it dropped, and the key path and
/// message of the value that failed.
#[derive(Debug, Clone)]
struct Failure {
    section: Option<&'static str>,
    repair: Option<&'static str>,
    unit: Option<String>,
    path: Vec<String>,
    message: String,
}

/// The outcome of checking one parsed file.
#[derive(Debug, Clone, Default)]
pub struct Checked {
    /// The top-level keys of every unit that passed, plus the fail-closed
    /// readings of the switches in units that failed.
    pub accepted: Mapping,
    /// Units that fell back: section names, `<section>.<entry>`, or
    /// `<section>[<index>]`.
    pub failed: Vec<String>,
    failures: Vec<Failure>,
}

impl Checked {
    pub fn is_clean(&self) -> bool {
        self.failures.is_empty()
    }

    /// The key paths that failed, with their section's name; for `fix`.
    pub fn failing(&self) -> Vec<(Option<&'static str>, Vec<String>)> {
        self.failures.iter().map(|f| (f.section, f.path.clone())).collect()
    }

    /// The findings, with line numbers looked up in `text`.
    pub fn findings(&self, schema_name: Option<&str>, file: Option<&Path>, text: &str) -> Vec<Finding> {
        let doc = Doc::parse(text).ok();
        self.failures
            .iter()
            .map(|f| {
                let mut message = f.message.clone();
                if f.section.is_none() {
                    if let Some(n) = schema_name {
                        message = format!("{message} ({n})");
                    }
                }
                Finding {
                    file: file.map(Path::to_path_buf),
                    line: doc.as_ref().and_then(|d| d.line_of(&f.path)),
                    section: f.section.map(str::to_string),
                    unit: f.unit.clone(),
                    key: (!f.path.is_empty()).then(|| f.path.join(".")),
                    message,
                    fallback: f.unit.as_ref().is_some_and(|u| self.failed.contains(u)),
                    // A key in a file that may not hold it is removed by fix,
                    // whatever command owns it elsewhere.
                    repair: f.repair.filter(|_| !f.message.ends_with(" file only")).map(str::to_string),
                }
            })
            .collect()
    }

    fn fail(&mut self, section: &SectionSpec, unit: String, errs: Vec<(Vec<String>, String)>) {
        if !self.failed.contains(&unit) {
            self.failed.push(unit.clone());
        }
        self.failures.extend(errs.into_iter().map(|(path, message)| Failure {
            section: Some(section.name),
            repair: section.repair,
            unit: Some(unit.clone()),
            path,
            message,
        }));
    }
}

/// Parse a settings file's text. A parse failure is one finding on the line
/// the parser names; the file's sections then resolve from the layers
/// beneath, except the switches salvaged by [`closed_from_salvage`].
pub fn parse_text(text: &str, file: Option<&Path>) -> Result<Value, Box<Finding>> {
    yaml_edit::parse(text).map_err(|e| {
        let (line, message) = match e {
            yaml_edit::EditError::Parse { line, message } => (line, format!("does not parse: {message}")),
            other => (None, other.to_string()),
        };
        Box::new(Finding {
            file: file.map(Path::to_path_buf),
            line,
            section: None,
            unit: None,
            key: None,
            message: format!(
                "{message}; its sections resolve from the layers beneath, except that a switch whose key can be \
                 found stays off unless its value reads cleanly as on"
            ),
            fallback: true,
            repair: None,
        })
    })
}

/// The specs a schema checks in one file kind.
fn file_keys<'a>(schema: &'a Schema, file: &str) -> Vec<&'a KeySpec> {
    schema.keys.iter().filter(|k| k.file == file && k.computed.is_none()).collect()
}

/// The fail-closed reading of the raw value at `path`, if its key has one.
fn closed(keys: &[&KeySpec], scope: LayerScope, path: &[String], raw: &Value) -> Option<Value> {
    let k = keys.iter().find(|k| k.match_path(path).is_some())?;
    if !scope.admits(k.scope) {
        return None;
    }
    (k.fail_closed?)(raw)
}

/// Check a parsed file of kind `file` at `scope` against `schema`. With
/// `only`, the sections not named are skipped: not checked and not loaded.
/// A top-level key no section owns is reported either way.
pub fn check(schema: &Schema, file: &str, scope: LayerScope, doc: &Value, only: Option<&[&str]>) -> Checked {
    trace_sections(schema, file, only);
    let mut out = Checked::default();
    let Some(root) = doc.as_mapping() else { return out };
    let keys = file_keys(schema, file);
    let retired = schema.file(file).map(|f| f.retired).unwrap_or(&[]);
    let mut closed_tops: Vec<(Value, Value)> = Vec::new();
    for (k, v) in root {
        let Some(top) = k.as_str() else {
            out.failures.push(Failure {
                section: None,
                repair: None,
                unit: None,
                path: vec![crate::schema::show(k)],
                message: "a top-level key is not text; it is ignored".to_string(),
            });
            continue;
        };
        let Some(sec) = schema.section_of_top(file, top) else {
            let message = match retired.iter().find(|(r, _)| *r == top) {
                Some((_, m)) => (*m).to_string(),
                None => "unknown key; it is ignored".to_string(),
            };
            out.failures.push(Failure { section: None, repair: None, unit: None, path: vec![top.to_string()], message });
            continue;
        };
        if only.is_some_and(|o| !o.contains(&sec.name)) {
            continue;
        }
        let mut accepted = v.clone();
        match (sec.per_entry, v) {
            (true, Value::Mapping(m)) => {
                let kept = accepted.as_mapping_mut().expect("a mapping");
                for (ek, ev) in m {
                    let mut errs = Vec::new();
                    let entry = match ek.as_str() {
                        Some(e) => e.to_string(),
                        None => {
                            let e = crate::schema::show(ek);
                            errs.push((vec![top.to_string(), e.clone()], "a key is not text".to_string()));
                            e
                        }
                    };
                    let path = vec![top.to_string(), entry.clone()];
                    if errs.is_empty() {
                        check_tree(&keys, scope, &mut path.clone(), ev, &mut errs);
                    }
                    if !errs.is_empty() {
                        out.fail(sec, format!("{}.{entry}", sec.name), errs);
                        match (ek.is_string(), closed(&keys, scope, &path, ev)) {
                            (true, Some(c)) => {
                                kept.insert(ek.clone(), c);
                            }
                            _ => {
                                kept.remove(ek);
                            }
                        }
                    }
                }
            }
            (true, Value::Sequence(items)) => {
                let path = vec![top.to_string()];
                let mut kept = Vec::new();
                for (i, item) in items.iter().enumerate() {
                    let mut errs = Vec::new();
                    check_tree(&keys, scope, &mut path.clone(), &Value::Sequence(vec![item.clone()]), &mut errs);
                    if errs.is_empty() {
                        kept.push(item.clone());
                        continue;
                    }
                    out.fail(sec, format!("{}[{i}]", sec.name), errs);
                    if let Some(Value::Sequence(c)) = closed(&keys, scope, &path, &Value::Sequence(vec![item.clone()])) {
                        kept.extend(c);
                    }
                }
                accepted = Value::Sequence(kept);
            }
            _ => {
                let mut errs = Vec::new();
                check_tree(&keys, scope, &mut vec![top.to_string()], v, &mut errs);
                if !errs.is_empty() {
                    out.fail(sec, sec.name.to_string(), errs);
                    if let Some(c) = closed(&keys, scope, &[top.to_string()], v) {
                        closed_tops.push((k.clone(), c));
                    }
                }
            }
        }
        out.accepted.insert(k.clone(), accepted);
    }
    // A whole-section failure drops every top-level key the section owns;
    // a switch among them keeps its fail-closed reading.
    let failed = out.failed.clone();
    out.accepted.retain(|k, _| {
        k.as_str().and_then(|t| schema.section_of_top(file, t)).is_none_or(|sec| !failed.iter().any(|u| u == sec.name))
    });
    for (k, c) in closed_tops {
        out.accepted.insert(k, c);
    }
    out
}

fn check_tree(keys: &[&KeySpec], scope: LayerScope, path: &mut Vec<String>, v: &Value, errs: &mut Vec<(Vec<String>, String)>) {
    if let Some(k) = keys.iter().find(|k| k.match_path(path).is_some()) {
        if !scope.admits(k.scope) {
            errs.push((path.clone(), format!("{} is set in the {} file only", k.name, k.scope.as_str())));
        } else if let Err(m) = k.check_value(v) {
            errs.push((path.clone(), m));
        }
        return;
    }
    let deeper = keys.iter().any(|k| {
        k.path.len() > path.len() && k.path.iter().zip(path.iter()).all(|(p, s)| *p == "*" || p == s)
    });
    if !deeper {
        errs.push((path.clone(), "unknown key".into()));
        return;
    }
    match v {
        Value::Null => {}
        Value::Mapping(m) => {
            for (kk, vv) in m {
                let Some(s) = kk.as_str() else {
                    let mut at = path.clone();
                    at.push(crate::schema::show(kk));
                    errs.push((at, "a key is not text".to_string()));
                    continue;
                };
                path.push(s.to_string());
                check_tree(keys, scope, path, vv, errs);
                path.pop();
            }
        }
        other => errs.push((path.clone(), format!("expected a mapping, found {}", crate::schema::show(other)))),
    }
}

// ── salvage ────────────────────────────────────────────────────
//
// A heuristic over text that does not parse, used only to keep the switches
// it turns off. It never fails and never panics: lines are measured in ASCII
// spaces, so a cut is always a character boundary. A value that cannot be
// read is kept as its raw text, which every fail-closed reading treats as
// closed: where the text cannot be read, closed wins.

/// What salvage reads from broken text. Duplicate keys are kept, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Salvaged {
    /// A value that parsed, or an unreadable one as its raw text (or null).
    Value(Value),
    Map(Vec<(String, Salvaged)>),
    Seq(Vec<Salvaged>),
}

impl Salvaged {
    /// As a plain value, for a fail-closed reading. A duplicate key keeps
    /// its first occurrence here; [`closed_from_salvage`] merges them.
    pub fn to_value(&self) -> Value {
        match self {
            Salvaged::Value(v) => v.clone(),
            Salvaged::Map(pairs) => {
                let mut m = Mapping::new();
                for (k, v) in pairs {
                    if !m.contains_key(k.as_str()) {
                        m.insert(Value::String(k.clone()), v.to_value());
                    }
                }
                Value::Mapping(m)
            }
            Salvaged::Seq(items) => Value::Sequence(items.iter().map(Salvaged::to_value).collect()),
        }
    }
}

fn spaces(l: &str) -> usize {
    l.len() - l.trim_start_matches(' ').len()
}

fn is_content(l: &str) -> bool {
    !l.trim().is_empty() && !l.trim_start().starts_with('#')
}

/// `l` without up to `n` leading spaces; never cuts into text.
fn dedent(l: &str, n: usize) -> &str {
    &l[spaces(l).min(n)..]
}

/// The top-level entries of text that does not parse as a whole.
pub fn salvage(text: &str) -> Vec<(String, Salvaged)> {
    if let Ok(Value::Mapping(m)) = yaml_edit::parse(text) {
        return m.into_iter().filter_map(|(k, v)| Some((k.as_str()?.to_string(), Salvaged::Value(v)))).collect();
    }
    let lines: Vec<&str> = text.lines().collect();
    salvage_map(&lines)
}

/// Entries at the least indent of `lines`. Lines indented less than the
/// first entry, or before it, belong to no entry and are skipped.
fn salvage_map(lines: &[&str]) -> Vec<(String, Salvaged)> {
    let Some(base) = lines.iter().filter(|l| is_content(l)).map(|l| spaces(l)).min() else { return Vec::new() };
    let starts: Vec<usize> = (0..lines.len()).filter(|&i| is_content(lines[i]) && spaces(lines[i]) == base).collect();
    let mut out = Vec::new();
    for (n, &s) in starts.iter().enumerate() {
        let e = starts.get(n + 1).copied().unwrap_or(lines.len());
        let head = dedent(lines[s], base).trim_end();
        if head.starts_with('{') {
            out.extend(salvage_flow(head));
            continue;
        }
        let Some((raw_key, rest)) = yaml_edit::split_key(head) else { continue };
        let key = yaml_edit::unquote(&raw_key);
        let block: Vec<&str> = lines[s..e].iter().map(|l| dedent(l, base)).collect();
        if let Ok(Value::Mapping(m)) = serde_yaml::from_str::<Value>(&block.join("\n")) {
            if let Some((_, v)) = m.into_iter().next() {
                out.push((key, Salvaged::Value(v)));
                continue;
            }
        }
        let (rest, _) = yaml_edit::split_comment(&rest);
        let children = &lines[s + 1..e];
        let v = if !rest.is_empty() {
            match serde_yaml::from_str::<Value>(&rest) {
                Ok(v) if !children.iter().any(|l| is_content(l)) => Salvaged::Value(v),
                _ => Salvaged::Value(Value::String(rest)),
            }
        } else if children.iter().any(|l| is_content(l)) {
            salvage_children(children)
        } else {
            Salvaged::Value(Value::Null)
        };
        out.push((key, v));
    }
    out
}

/// The block under a key: a list, item by item, or a mapping.
fn salvage_children(lines: &[&str]) -> Salvaged {
    let Some(base) = lines.iter().filter(|l| is_content(l)).map(|l| spaces(l)).min() else {
        return Salvaged::Value(Value::Null);
    };
    let first = lines.iter().find(|l| is_content(l) && spaces(l) == base).map(|l| dedent(l, base)).unwrap_or("");
    if !(first == "-" || first.starts_with("- ")) {
        return Salvaged::Map(salvage_map(lines));
    }
    let starts: Vec<usize> = (0..lines.len())
        .filter(|&i| is_content(lines[i]) && spaces(lines[i]) == base && dedent(lines[i], base).starts_with('-'))
        .collect();
    let mut items = Vec::new();
    for (n, &s) in starts.iter().enumerate() {
        let e = starts.get(n + 1).copied().unwrap_or(lines.len());
        let block: Vec<&str> = lines[s..e].iter().map(|l| dedent(l, base)).collect();
        match serde_yaml::from_str::<Value>(&block.join("\n")) {
            Ok(Value::Sequence(mut v)) if v.len() == 1 => items.push(Salvaged::Value(v.remove(0))),
            _ => {
                let raw = block[0].trim_start_matches('-').trim();
                items.push(Salvaged::Value(Value::String(raw.to_string())));
            }
        }
    }
    Salvaged::Seq(items)
}

/// `{a: 1, b: [}` on one line: split at the top-level commas.
fn salvage_flow(line: &str) -> Vec<(String, Salvaged)> {
    let body = line.trim_start_matches('{');
    let body = body.strip_suffix('}').unwrap_or(body);
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0i32, 0usize);
    for (i, c) in body.char_indices() {
        match c {
            '[' | '{' => depth += 1,
            ']' | '}' => depth -= 1,
            ',' if depth <= 0 => {
                parts.push(&body[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&body[start..]);
    parts
        .into_iter()
        .filter_map(|p| {
            let (raw_key, rest) = yaml_edit::split_key(p.trim())?;
            let rest = rest.trim().to_string();
            let v = serde_yaml::from_str::<Value>(&rest).unwrap_or(Value::String(rest));
            Some((yaml_edit::unquote(&raw_key), Salvaged::Value(v)))
        })
        .collect()
}

/// Merge a closed reading into `out` at `key`: the first closed reading
/// stays, and lists are joined, so a later open duplicate never reopens it.
fn merge_closed(out: &mut Mapping, key: &str, c: Value) {
    match (out.get_mut(key), c) {
        (Some(Value::Sequence(have)), Value::Sequence(more)) => {
            for v in more {
                if !have.contains(&v) {
                    have.push(v);
                }
            }
        }
        (Some(_), _) => {}
        (None, c) => {
            out.insert(Value::String(key.to_string()), c);
        }
    }
}

/// The fail-closed readings of every switch salvaged from text that does
/// not parse: what such a file still contributes to its layer. A switch
/// whose key can be found stays off unless its value reads cleanly as on.
/// A per-entry list that is named but unreadable contributes an empty list,
/// so it never falls to an implicit default.
pub fn closed_from_salvage(schema: &Schema, file: &str, scope: LayerScope, text: &str, only: Option<&[&str]>) -> Mapping {
    let keys = file_keys(schema, file);
    let mut out = Mapping::new();
    for (top, v) in salvage(text) {
        let Some(sec) = schema.section_of_top(file, &top) else { continue };
        if only.is_some_and(|o| !o.contains(&sec.name)) {
            continue;
        }
        let path = vec![top.clone()];
        if !sec.per_entry {
            if let Some(c) = closed(&keys, scope, &path, &v.to_value()) {
                merge_closed(&mut out, &top, c);
            }
            continue;
        }
        // Per-entry: each entry or item on its own.
        let as_entries = match &v {
            Salvaged::Map(pairs) => Some(pairs.iter().map(|(k, s)| (k.clone(), s.to_value())).collect::<Vec<_>>()),
            Salvaged::Value(Value::Mapping(m)) => {
                Some(m.iter().filter_map(|(k, s)| Some((k.as_str()?.to_string(), s.clone()))).collect())
            }
            _ => None,
        };
        let as_items = match &v {
            Salvaged::Seq(items) => Some(items.iter().map(Salvaged::to_value).collect::<Vec<_>>()),
            Salvaged::Value(Value::Sequence(items)) => Some(items.clone()),
            _ => None,
        };
        if let Some(entries) = as_entries {
            let mut kept = match out.get(top.as_str()) {
                Some(Value::Mapping(m)) => m.clone(),
                _ => Mapping::new(),
            };
            for (e, ev) in entries {
                if let Some(c) = closed(&keys, scope, &[top.clone(), e.clone()], &ev) {
                    merge_closed(&mut kept, &e, c);
                }
            }
            if !kept.is_empty() {
                out.insert(Value::String(top.clone()), Value::Mapping(kept));
            }
        } else {
            let mut kept = Vec::new();
            for item in as_items.unwrap_or_default() {
                if let Some(Value::Sequence(c)) = closed(&keys, scope, &path, &Value::Sequence(vec![item])) {
                    kept.extend(c);
                }
            }
            // A list that is named stays a list, so it never falls to an
            // implicit default, even when nothing in it can be read.
            if keys.iter().any(|k| k.match_path(&path).is_some() && scope.admits(k.scope)) {
                merge_closed(&mut out, &top, Value::Sequence(kept));
            }
        }
    }
    out
}

// ── trace ──────────────────────────────────────────────────────

/// `WAYS_SETTINGS_TRACE=1` prints each load to stderr, so a test can see
/// what a hook path loads (ADR-503 §5). Read once per process.
fn tracing() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("WAYS_SETTINGS_TRACE").is_some_and(|v| !v.is_empty()))
}

/// `load-sections <component>:<file> [names]` for a load that names its
/// sections, `load-all <component>:<file>` for one that takes the whole file.
fn trace_sections(schema: &Schema, file: &str, only: Option<&[&str]>) {
    if tracing() {
        match only {
            Some(o) => eprintln!("settings-trace: load-sections {}:{file} [{}]", schema.component, o.join(",")),
            None => eprintln!("settings-trace: load-all {}:{file}", schema.component),
        }
    }
}

/// Record a full-tree operation (list, lint, emit) in the trace.
pub fn trace(what: &str) {
    if tracing() {
        eprintln!("settings-trace: {what}");
    }
}

// ── layers ─────────────────────────────────────────────────────

/// One source of values: a file at a scope, already checked.
#[derive(Debug, Clone)]
pub struct Layer {
    /// `user`, `project`, `target`, `legacy`, `file`.
    pub name: String,
    pub file: &'static str,
    pub scope: LayerScope,
    pub path: Option<PathBuf>,
    /// Sections that passed, by top-level key.
    pub accepted: Mapping,
    /// Every finding in the file, with lines.
    pub findings: Vec<Finding>,
    /// Whether a file was there to read.
    pub present: bool,
}

impl Layer {
    /// Read and check a file. A missing file is an empty, clean layer.
    pub fn read(schema: &Schema, name: &str, file: &'static str, scope: LayerScope, path: &Path) -> Layer {
        // Read lossily: a stray byte must not hide the switches in the file.
        let text = std::fs::read(path).ok().map(|b| String::from_utf8_lossy(&b).into_owned());
        let present = text.is_some();
        let mut layer = Layer::from_text(schema, name, file, scope, Some(path), text.as_deref().unwrap_or(""));
        layer.present = present;
        layer
    }

    pub fn from_text(schema: &Schema, name: &str, file: &'static str, scope: LayerScope, path: Option<&Path>, text: &str) -> Layer {
        let (accepted, findings) = match parse_text(text, path) {
            Ok(doc) => {
                let c = check(schema, file, scope, &doc, None);
                let f = if c.is_clean() { Vec::new() } else { c.findings(None, path, text) };
                (c.accepted, f)
            }
            Err(f) => (closed_from_salvage(schema, file, scope, text, None), vec![*f]),
        };
        Layer { name: name.into(), file, scope, path: path.map(Path::to_path_buf), accepted, findings, present: true }
    }

    /// A layer from a value built elsewhere (a legacy format), checked the same way.
    pub fn from_value(schema: &Schema, name: &str, file: &'static str, scope: LayerScope, path: Option<&Path>, v: &Value) -> Layer {
        let c = check(schema, file, scope, v, None);
        let findings = if c.is_clean() { Vec::new() } else { c.findings(None, path, "") };
        Layer { name: name.into(), file, scope, path: path.map(Path::to_path_buf), accepted: c.accepted, findings, present: true }
    }

    pub fn get(&self, path: &[String]) -> Option<&Value> {
        let (first, rest) = path.split_first()?;
        let v = self.accepted.get(first.as_str())?;
        yaml_edit::value_at(v, rest)
    }
}

/// A key's resolved value and where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub name: String,
    pub path: Vec<String>,
    pub value: Option<Value>,
    pub default: Option<Value>,
    /// Index into the layers; `None` when the value is the default.
    pub layer: Option<usize>,
}

/// Resolve one bound key through `layers`, lowest first.
pub fn resolve(spec: &KeySpec, bound: &[String], layers: &[Layer]) -> Resolved {
    let (name, path) = spec.bind(bound);
    let default = spec.default_for(bound);
    if let Some(c) = spec.computed {
        return Resolved { name, path, value: Some(c(bound)), default, layer: None };
    }
    let mut value = default.clone();
    let mut from = None;
    for (i, l) in layers.iter().enumerate() {
        if l.file != spec.file || !l.scope.admits(spec.scope) {
            continue;
        }
        if let Some(v) = l.get(&path) {
            value = Some(v.clone());
            from = Some(i);
        }
    }
    Resolved { name, path, value, default, layer: from }
}

/// Every binding of a key: its declared instances plus any a layer sets.
pub fn bindings(spec: &KeySpec, layers: &[Layer]) -> Vec<Vec<String>> {
    if !spec.is_pattern() {
        return vec![vec![]];
    }
    let mut out: Vec<Vec<String>> = spec.instances.iter().map(|i| vec![i.to_string()]).collect();
    for l in layers.iter().filter(|l| l.file == spec.file && l.scope.admits(spec.scope)) {
        let root = Value::Mapping(l.accepted.clone());
        collect_bindings(&root, spec.path, &mut Vec::new(), &mut out);
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|b| seen.insert(b.clone()));
    out
}

fn collect_bindings(v: &Value, pat: &[&str], bound: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    let Some((first, rest)) = pat.split_first() else {
        out.push(bound.clone());
        return;
    };
    let Some(m) = v.as_mapping() else { return };
    if *first == "*" {
        for (k, child) in m {
            if let Some(k) = k.as_str() {
                bound.push(k.to_string());
                collect_bindings(child, rest, bound, out);
                bound.pop();
            }
        }
    } else if let Some(child) = m.get(*first) {
        collect_bindings(child, rest, bound, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::*;

    const SECTIONS: &[SectionSpec] = &[
        SectionSpec { per_entry: false, repair: None, name: "general", file: "cfg", top: &["language"], doc: "" },
        SectionSpec { per_entry: false, repair: None, name: "matching", file: "cfg", top: &["prob", "presets"], doc: "" },
        SectionSpec { per_entry: true, repair: None, name: "toggles", file: "cfg", top: &["ways"], doc: "" },
    ];
    const BASE: KeySpec = KeySpec {
        name: "general.language",
        section: "general",
        file: "cfg",
        path: &["language"],
        kind: Kind::Text,
        default: DefaultValue::Yaml("auto"),
        instances: &[],
        scope: Scope::Both,
        doc: "",
        long: "",
        check: None,
        computed: None,
        fail_closed: None,
    };
    const KEYS: &[KeySpec] = &[
        BASE,
        KeySpec { name: "matching.prob", section: "matching", path: &["prob"], kind: Kind::Float { min: 0.0, max: 1.0 }, default: DefaultValue::Yaml("0.5"), ..BASE },
        KeySpec { name: "matching.presets.*", section: "matching", path: &["presets", "*"], kind: Kind::Float { min: 0.0, max: 1.0 }, instances: &["normal"], default: DefaultValue::Yaml("0.15"), ..BASE },
        KeySpec { name: "ways.project.*", section: "toggles", path: &["ways", "*"], kind: Kind::Toggle, scope: Scope::Project, default: DefaultValue::Yaml("true"), ..BASE },
    ];
    const SCHEMA: Schema = Schema { component: "t", files: &[FileSpec { id: "cfg", retired: &[("old_key", "old_key was retired; use prob")] }], sections: SECTIONS, keys: KEYS };

    fn layer(name: &str, scope: LayerScope, text: &str) -> Layer {
        Layer::from_text(&SCHEMA, name, "cfg", scope, Some(Path::new(&format!("/{name}.yaml"))), text)
    }

    #[test]
    fn a_failing_section_falls_back_and_the_rest_load() {
        let l = layer("user", LayerScope::User, "language: es\nprob: 1.5\npresets:\n  normal: 0.2\n");
        assert_eq!(l.get(&["language".into()]), Some(&Value::String("es".into())));
        assert_eq!(l.get(&["prob".into()]), None);
        assert_eq!(l.get(&["presets".into(), "normal".into()]), None, "the whole section falls back");
        let f = &l.findings[0];
        assert_eq!((f.line, f.section.as_deref(), f.key.as_deref(), f.fallback), (Some(2), Some("matching"), Some("prob"), true));
        assert!(f.to_string().starts_with("/user.yaml:2: [matching] prob: 1.5 is outside"), "{f}");
    }

    #[test]
    fn unknown_keys_fail_their_section_and_retired_ones_only_report() {
        let l = layer("user", LayerScope::User, "presets:\n  normal: 0.2\n  typo: x\nold_key: 1\nmystery: 2\n");
        assert!(l.get(&["presets".into()]).is_none());
        let msgs: Vec<String> = l.findings.iter().map(|f| format!("{:?} {}", f.key, f.message)).collect();
        assert!(msgs.iter().any(|m| m.contains("old_key was retired")), "{msgs:?}");
        assert!(msgs.iter().any(|m| m.contains("mystery") && m.contains("unknown key")), "{msgs:?}");
        let typo = l.findings.iter().find(|f| f.key.as_deref() == Some("presets.typo")).unwrap();
        assert_eq!(typo.line, Some(3));
        assert!(!l.findings.iter().find(|f| f.key.as_deref() == Some("mystery")).unwrap().fallback);
    }

    #[test]
    fn a_project_only_key_in_the_user_file_is_a_finding() {
        let l = layer("user", LayerScope::User, "ways:\n  a/b: false\n");
        assert!(l.get(&["ways".into(), "a/b".into()]).is_none());
        let p = layer("project", LayerScope::Project, "ways:\n  a/b: {enabled: false}\n");
        assert!(p.findings.is_empty());
    }

    #[test]
    fn provenance_names_the_winning_layer_and_falls_through_a_bad_one() {
        let layers = vec![
            layer("user", LayerScope::User, "prob: 0.4\nlanguage: es\n"),
            layer("project", LayerScope::Project, "prob: 7\nlanguage: ja\n"),
        ];
        let r = resolve(&KEYS[1], &[], &layers);
        assert_eq!((r.value, r.layer), (Some(serde_yaml::from_str("0.4").unwrap()), Some(0)));
        let r = resolve(&KEYS[0], &[], &layers);
        assert_eq!((r.value, r.layer), (Some(Value::String("ja".into())), Some(1)));
        let r = resolve(&KEYS[0], &[], &[]);
        assert_eq!((r.value, r.layer), (Some(Value::String("auto".into())), None));
        let b = bindings(&KEYS[2], &[layer("user", LayerScope::User, "presets:\n  rare: 0.3\n")]);
        assert_eq!(b, vec![vec!["normal".to_string()], vec!["rare".to_string()]]);
    }

    #[test]
    fn a_parse_error_falls_back_every_section_and_names_the_line() {
        let l = layer("user", LayerScope::User, "language: es\nprob: [\n");
        assert!(l.accepted.is_empty());
        assert_eq!(l.findings.len(), 1);
        assert!(l.findings[0].line.is_some());
        assert!(l.findings[0].message.contains("does not parse"));
    }

    #[test]
    fn only_skips_the_sections_not_named() {
        let doc: Value = serde_yaml::from_str("language: 3\nprob: 0.2\n").unwrap();
        let c = check(&SCHEMA, "cfg", LayerScope::User, &doc, Some(&["matching"]));
        assert!(c.is_clean(), "the broken general section is not read");
        assert!(c.accepted.get("language").is_none());
        assert!(c.accepted.get("prob").is_some());
    }

    #[test]
    fn a_per_entry_section_drops_only_the_bad_entry() {
        let p = layer("project", LayerScope::Project, "ways:\n  a/b: false\n  c/d: maybe\n  e/f: {enabled: false}\n");
        assert_eq!(p.get(&["ways".into(), "a/b".into()]), Some(&Value::Bool(false)));
        assert_eq!(p.get(&["ways".into(), "e/f".into(), "enabled".into()]), Some(&Value::Bool(false)));
        assert_eq!(p.get(&["ways".into(), "c/d".into()]), None);
        let f = &p.findings[0];
        assert_eq!((f.unit.as_deref(), f.fallback, f.line), (Some("toggles.c/d"), true, Some(3)));
        assert!(f.diagnostic("t").contains("entry toggles.c/d is ignored"), "{}", f.diagnostic("t"));
    }

    #[test]
    fn an_unknown_top_level_key_is_reported_on_a_sectioned_load() {
        let doc: Value = serde_yaml::from_str("mdoe: off\nprob: 0.2\n").unwrap();
        let c = check(&SCHEMA, "cfg", LayerScope::User, &doc, Some(&["matching"]));
        let f = c.findings(None, None, "mdoe: off\nprob: 0.2\n");
        assert_eq!(f.len(), 1);
        assert_eq!((f[0].key.as_deref(), f[0].fallback, f[0].line), (Some("mdoe"), false, Some(1)));
        assert!(c.accepted.get("prob").is_some());
    }

    #[test]
    fn the_fallback_diagnostic_says_the_keys_fall_through() {
        let l = layer("user", LayerScope::User, "prob: 3\n");
        let d = l.findings[0].diagnostic("t");
        assert!(d.contains("resolve from the layers beneath, ending at canonical"), "{d}");
        assert!(!d.contains("falls back to canonical"), "{d}");
    }

    #[test]
    fn salvage_reads_the_entries_that_parse() {
        let m = salvage("enabled: false\nlanguage: [\nways:\n  a/b: false\n  c/d: [\n  e/f: false\n");
        assert_eq!(m[0], ("enabled".into(), Salvaged::Value(Value::Bool(false))));
        assert_eq!(m[1], ("language".into(), Salvaged::Value(Value::String("[".into()))));
        let Salvaged::Map(ways) = &m[2].1 else { panic!("{m:?}") };
        assert_eq!(ways[0], ("a/b".into(), Salvaged::Value(Value::Bool(false))));
        assert_eq!(ways[1], ("c/d".into(), Salvaged::Value(Value::String("[".into()))));
        assert_eq!(ways[2], ("e/f".into(), Salvaged::Value(Value::Bool(false))));
    }

    #[test]
    fn salvage_never_cuts_into_text_or_keys() {
        // N1: the first line indented, a multibyte comment at column 0.
        let m = salvage("  language: en\n#ña\nenabled: false\n");
        assert!(m.contains(&("enabled".into(), Salvaged::Value(Value::Bool(false)))), "{m:?}");
        let m = salvage("  language: en\nenabled: false\n");
        assert!(m.contains(&("enabled".into(), Salvaged::Value(Value::Bool(false)))), "{m:?}");
    }

    #[test]
    fn salvage_and_checks_never_panic_on_generated_text() {
        // A deterministic fuzz over pieces that break YAML: indents, quotes,
        // brackets, list marks, multibyte text, tabs, duplicate keys.
        const PIECES: &[&str] = &[
            "enabled", "ways", "targets", "mode", ":", ": ", " ", "  ", "   ", "\t", "\n", "\n", "\n", "- ", "-", "\"",
            "'", "[", "]", "{", "}", ",", "#", "ñ", "é", "日本", "\u{1F600}", "false", "true", "a/b", "path", "~/.claude",
            "|", ">", "&a", "*a", "!!str", "---", "...", "\r\n", "\u{feff}", "0", "-1", "1.5e3", "null", "?",
        ];
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..20_000 {
            let len = (next() % 40) as usize;
            let text: String = (0..len).map(|_| PIECES[(next() % PIECES.len() as u64) as usize]).collect();
            let _ = salvage(&text);
            let _ = closed_from_salvage(&SCHEMA, "cfg", LayerScope::Project, &text, None);
            let l = Layer::from_text(&SCHEMA, "f", "cfg", LayerScope::Project, Some(Path::new("/f.yaml")), &text);
            for f in &l.findings {
                let _ = f.diagnostic("t");
            }
            if let Ok(mut d) = crate::yaml_edit::Doc::parse(&text) {
                let _ = d.line_of(&["ways".into(), "a/b".into()]);
                let _ = d.set(&["ways".into(), "a/b".into()], &Value::Bool(false));
            }
        }
    }
}
