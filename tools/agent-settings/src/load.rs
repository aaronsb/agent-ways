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
//! with a fail-closed reading takes it when its unit fails. A file that does
//! not parse fails closed as a whole: it sets nothing, and every switch in
//! its scope reads off ([`closed_file`]).

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
    /// An entry's name the section refuses: the section failed closed in
    /// this file, every switch it holds reads off, and only a hand edit of
    /// the name repairs it (`fix` refuses).
    pub closed: bool,
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
            (Some(s), _, true) if self.closed => format!(
                "[{tool}] settings: {self}; a name that names nothing closes section {s} in this file: every \
                 switch it holds reads off, and nothing else in it is read, until the name is edited by hand \
                 (`ways settings fix` cannot repair a name)"
            ),
            (Some(s), Some(u), true) if u != s => format!(
                "[{tool}] settings: {self}; entry {u} is ignored, so it resolves from the layers beneath \
                 (a switch whose own value is bad reads off). `ways settings lint` lists the findings, {}",
                fix(s)
            ),
            (Some(s), _, true) => format!(
                "[{tool}] settings: {self}; section {s} is ignored in this file, so its keys resolve from the \
                 layers beneath, ending at canonical (a switch whose own value is bad reads off). \
                 `ways settings lint` lists the findings, {}",
                fix(s)
            ),
            // A value that loads but names nothing in its list.
            (_, _, false) if self.repair.is_some() => format!("[{tool}] settings: {self}{}", self.lint_note()),
            _ => format!("[{tool}] settings: {self}"),
        }
    }

    /// What `lint` adds after the finding: the repair of a value that
    /// loads, which `fix` does not touch. Empty for every other finding.
    pub fn lint_note(&self) -> String {
        match (&self.repair, self.fallback) {
            (Some(r), false) => format!("; it loads as written, and {r} repairs it"),
            _ => String::new(),
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
    /// A refused entry name, which closes its section.
    closed: bool,
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

    /// Whether an entry's name was refused, closing its section: a finding
    /// only a hand edit repairs.
    pub fn has_refused_name(&self) -> bool {
        self.failures.iter().any(|f| f.closed)
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
                    // whatever command owns it elsewhere. A refused name is
                    // repaired by hand only.
                    repair: match f.closed {
                        true => Some("editing the name by hand".to_string()),
                        false => f.repair.filter(|_| !f.message.ends_with(" file only")).map(str::to_string),
                    },
                    closed: f.closed,
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
            closed: false,
        }));
    }
}

/// Parse a settings file's text. A parse failure is one finding on the line
/// the parser names; the whole file then fails closed ([`closed_file`]).
pub fn parse_text(text: &str, file: Option<&Path>) -> Result<Value, Box<Finding>> {
    // One leading byte-order mark is not content. Stripped here, a valid file
    // parses, and a broken one reports its real cause and line rather than
    // the parser's "more than one document".
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
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
                "{message}; the whole file fails closed until its syntax is fixed by hand: it sets nothing, \
                 and every switch in its scope is off"
            ),
            fallback: true,
            repair: None,
            closed: false,
        })
    })
}

/// The specs a schema checks in one file kind.
fn file_keys<'a>(schema: &'a Schema, file: &str) -> Vec<&'a KeySpec> {
    schema.keys.iter().filter(|k| k.file == file && k.computed.is_none()).collect()
}

/// The fail-closed reading of the raw value at `path`, if its key has one.
/// A mapping no key names is a unit with keys inside it: it keeps the closed
/// reading of each switch in it, so a bad value beside a switch never
/// switches it back on.
fn closed(keys: &[&KeySpec], scope: LayerScope, path: &[String], raw: &Value) -> Option<Value> {
    if let Some(k) = keys.iter().find(|k| k.match_path(path).is_some()) {
        if !scope.admits(k.scope) {
            return None;
        }
        return (k.fail_closed?)(raw);
    }
    let mut out = Mapping::new();
    for (ck, cv) in raw.as_mapping()? {
        let Some(s) = ck.as_str() else { continue };
        let mut at = path.to_vec();
        at.push(s.to_string());
        if let Some(c) = closed(keys, scope, &at, cv) {
            out.insert(ck.clone(), c);
        }
    }
    (!out.is_empty()).then_some(Value::Mapping(out))
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
                closed: false,
            });
            continue;
        };
        let Some(sec) = schema.section_of_top(file, top) else {
            let message = match retired.iter().find(|(r, _)| *r == top) {
                Some((_, m)) => (*m).to_string(),
                None => "unknown key; it is ignored".to_string(),
            };
            out.failures.push(Failure { section: None, repair: None, unit: None, path: vec![top.to_string()], message, closed: false });
            continue;
        };
        if only.is_some_and(|o| !o.contains(&sec.name)) {
            continue;
        }
        let mut accepted = v.clone();
        match (sec.per_entry, v) {
            (true, Value::Mapping(m)) => {
                let kept = accepted.as_mapping_mut().expect("a mapping");
                // Names the section accepted, and the units with a name it refused.
                let mut names: Vec<String> = Vec::new();
                let mut refused: Vec<String> = Vec::new();
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
                    // A name the section refuses drops its entry whole: it
                    // names nothing, so no switch in it is kept either.
                    let named = match (errs.is_empty(), sec.entry) {
                        (true, Some(c)) => c(&entry).map_err(|m| errs.push((path.clone(), m))).is_ok(),
                        (named, _) => named,
                    };
                    match named {
                        true => names.push(entry.clone()),
                        false if ek.is_string() => refused.push(format!("{}.{entry}", sec.name)),
                        false => {}
                    }
                    if errs.is_empty() {
                        check_tree(&keys, scope, &mut path.clone(), ev, &mut errs);
                    }
                    if !errs.is_empty() {
                        out.fail(sec, format!("{}.{entry}", sec.name), errs);
                        match (named, closed(&keys, scope, &path, ev)) {
                            (true, Some(c)) => {
                                kept.insert(ek.clone(), c);
                            }
                            _ => {
                                kept.remove(ek);
                            }
                        }
                    }
                }
                // A refused name fails the section closed in this file (the
                // operator's decision in #733, under the ADR-503 addendum's
                // whole-file rule): it may be an off-switch the schema cannot
                // read, so every switch the section holds reads off, for each
                // name it declares and each name this file gives, and nothing
                // the file says in it is read.
                if !refused.is_empty() {
                    for f in out.failures.iter_mut().filter(|f| f.unit.as_ref().is_some_and(|u| refused.contains(u))) {
                        f.closed = true;
                    }
                    if !out.failed.iter().any(|u| u == sec.name) {
                        out.failed.push(sec.name.to_string());
                    }
                    closed_tops.push((k.clone(), Value::Mapping(closed_entries(&keys, scope, sec, &names))));
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

/// Set `v` at `path` under `root`, making the mappings between.
fn set_in(root: &mut Mapping, path: &[String], v: Value) {
    let (last, parents) = path.split_last().expect("a key has a path");
    let mut cur = root;
    for seg in parents {
        if !cur.get(seg.as_str()).is_some_and(Value::is_mapping) {
            cur.insert(Value::String(seg.clone()), Value::Mapping(Mapping::new()));
        }
        cur = cur.get_mut(seg.as_str()).and_then(Value::as_mapping_mut).expect("a mapping");
    }
    cur.insert(Value::String(last.clone()), v);
}

/// The bindings a key's closed reading is written for when nothing in the
/// file can be read: none for a fixed name, each declared instance for a
/// pattern with one wildcard. A pattern with no instances names nothing.
fn closed_bindings(k: &KeySpec) -> Vec<Vec<String>> {
    match k.name.matches('*').count() {
        0 => vec![vec![]],
        1 => k.instances.iter().map(|i| vec![i.to_string()]).collect(),
        _ => vec![],
    }
}

/// A per-entry section closed whole: for each entry name, its declared
/// instances and `names`, every fail-closed switch in the entry at its
/// reading of no value. The value under the section's top-level key.
fn closed_entries(keys: &[&KeySpec], scope: LayerScope, sec: &SectionSpec, names: &[String]) -> Mapping {
    let mut out = Mapping::new();
    for k in keys.iter().filter(|k| k.section == sec.name && scope.admits(k.scope) && k.path.len() >= 2 && k.path[1] == "*") {
        let Some(c) = k.fail_closed.and_then(|f| f(&Value::Null)) else { continue };
        let mut all: Vec<String> = k.instances.iter().map(|s| s.to_string()).collect();
        all.extend(names.iter().filter(|n| !k.instances.contains(&n.as_str())).cloned());
        for n in all {
            let path = k.bind(&[n]).1;
            set_in(&mut out, &path[1..], c.clone());
        }
    }
    out
}

// ── a file that does not parse ─────────────────────────────────

/// What a file that does not parse contributes to its layer: nothing it
/// says, and every switch in its scope closed ("whole file fails closed",
/// the operator's decision in #713, recorded in the ADR-503 addendum). Each
/// switch takes its fail-closed reading of no value: `enabled` off, the
/// gate's `mode` off, `secret_path_deny` on, an empty `targets` list where
/// the scope holds one. A switch with a pattern name is closed for each
/// instance its key declares (attend's built-in sensors); a name only a file
/// could give (a per-way toggle, a sensor of your own) cannot be read from a
/// file that does not parse, and `enabled: false` already switches every way
/// in that scope off.
pub fn closed_file(schema: &Schema, file: &str, scope: LayerScope, only: Option<&[&str]>) -> Mapping {
    let mut out = Mapping::new();
    for k in file_keys(schema, file) {
        if !scope.admits(k.scope) || only.is_some_and(|o| !o.contains(&k.section)) {
            continue;
        }
        let Some(c) = k.fail_closed.and_then(|f| f(&Value::Null)) else { continue };
        for b in closed_bindings(k) {
            set_in(&mut out, &k.bind(&b).1, c.clone());
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
    /// `user`, `project`, `target`, `file`.
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
            Err(f) => (closed_file(schema, file, scope, None), vec![*f]),
        };
        Layer { name: name.into(), file, scope, path: path.map(Path::to_path_buf), accepted, findings, present: true }
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

/// Check each stored value of a computed choice against its list over all
/// `layers`, adding a finding to the layer that holds a value the list
/// lacks. A load checks one file on its own, so it cannot; `lint` and the
/// screens read these. The value still loads: the finding reports it and
/// drops nothing.
pub fn check_choices<'a>(keys: impl IntoIterator<Item = &'a KeySpec>, layers: &mut [Layer]) {
    let mut found: Vec<(usize, Finding)> = Vec::new();
    for k in keys.into_iter().filter(|k| matches!(k.kind, crate::schema::Kind::ChoiceOf { .. })) {
        for (i, l) in layers.iter().enumerate().filter(|(_, l)| l.file == k.file && l.scope.admits(k.scope)) {
            for b in bindings(k, std::slice::from_ref(l)) {
                let (name, path) = k.bind(&b);
                let Some(v) = l.get(&path) else { continue };
                let Err(message) = k.check_value_for(v, Some(layers), &b) else { continue };
                let line = l
                    .path
                    .as_ref()
                    .and_then(|p| std::fs::read(p).ok())
                    .and_then(|t| Doc::parse(&String::from_utf8_lossy(&t)).ok())
                    .and_then(|d| d.line_of(&path));
                found.push((
                    i,
                    Finding {
                        file: l.path.clone(),
                        line,
                        section: Some(k.section.to_string()),
                        unit: None,
                        key: Some(path.join(".")),
                        message,
                        fallback: false,
                        // `fix` repairs what a load drops; this value loads,
                        // so setting a listed one is the repair, and naming
                        // it keeps the screens from offering `fix`.
                        repair: Some(format!("`ways settings set {name} <choice>`")),
                        closed: false,
                    },
                ));
            }
        }
    }
    for (i, f) in found {
        layers[i].findings.push(f);
    }
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
        SectionSpec { per_entry: false, entry: None, repair: None, name: "general", file: "cfg", top: &["language"], columns: None, doc: "" },
        SectionSpec { per_entry: false, entry: None, repair: None, name: "matching", file: "cfg", top: &["prob", "presets"], columns: None, doc: "" },
        SectionSpec { per_entry: true, entry: None, repair: None, name: "toggles", file: "cfg", top: &["ways"], columns: None, doc: "" },
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
    fn a_file_that_does_not_parse_contributes_only_closed_switches() {
        const SW: &[KeySpec] = &[
            KeySpec { name: "general.on", path: &["on"], kind: Kind::Bool, default: DefaultValue::Yaml("true"), fail_closed: Some(|v| (v != &Value::Bool(true)).then_some(Value::Bool(false))), ..BASE },
            KeySpec { name: "matching.prob", section: "matching", path: &["prob"], kind: Kind::Float { min: 0.0, max: 1.0 }, ..BASE },
        ];
        static S: Schema = Schema { component: "t", files: &[FileSpec { id: "cfg", retired: &[] }], sections: SECTIONS, keys: SW };
        // Every value the text holds is dropped, readable or not; the switch is off.
        let l = Layer::from_text(&S, "p", "cfg", LayerScope::Project, None, "on: true\nprob: 0.2\nx: [\n");
        assert_eq!(l.accepted, serde_yaml::from_str::<Mapping>("on: false").unwrap());
        assert!(l.findings[0].is_parse_failure() && l.findings[0].message.contains("whole file fails closed"));
        assert_eq!(closed_file(&S, "cfg", LayerScope::Project, Some(&["matching"])), Mapping::new());
    }

    #[test]
    fn loading_never_panics_on_arbitrary_bytes() {
        // The loader's entry point over generated bytes, valid UTF-8 or not
        // (read lossily, as Layer::read does).
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        const BYTES: &[u8] = b"enabled:false true-[]{}\"'#\t \n\r\n,|>&*!?.~\xef\xbb\xbf\xc3\xb1\xff\xfe";
        for _ in 0..5_000 {
            let len = (next() % 64) as usize;
            let bytes: Vec<u8> = (0..len).map(|_| BYTES[(next() % BYTES.len() as u64) as usize]).collect();
            let text = String::from_utf8_lossy(&bytes);
            let l = Layer::from_text(&SCHEMA, "f", "cfg", LayerScope::Project, Some(Path::new("/f.yaml")), &text);
            for f in &l.findings {
                let _ = f.diagnostic("t");
            }
        }
    }

    /// A per-entry section of units with a switch inside each, and a section
    /// whose switch sits under its top-level key: the shape of attend's
    /// `sensors:` and `cleanup:`.
    mod nested {
        use super::*;

        fn closed_off(v: &Value) -> Option<Value> {
            (v != &Value::Bool(true)).then_some(Value::Bool(false))
        }

        fn plain_name(n: &str) -> Result<(), String> {
            match n.chars().next() {
                Some(c) if c.is_ascii_alphanumeric() => Ok(()),
                _ => Err(format!("'{n}' is not a sensor name")),
            }
        }

        const SECTIONS: &[SectionSpec] = &[
            SectionSpec { per_entry: true, entry: Some(plain_name), repair: None, name: "sensors", file: "cfg", top: &["sensors"], columns: None, doc: "" },
            SectionSpec { per_entry: false, entry: None, repair: None, name: "cleanup", file: "cfg", top: &["cleanup"], columns: None, doc: "" },
        ];
        const KEYS: &[KeySpec] = &[
            KeySpec { name: "sensors.*.enabled", section: "sensors", path: &["sensors", "*", "enabled"], kind: Kind::Bool, default: DefaultValue::Yaml("true"), fail_closed: Some(closed_off), instances: &["a", "b"], ..BASE },
            KeySpec { name: "sensors.*.interval", section: "sensors", path: &["sensors", "*", "interval"], kind: Kind::Int { min: 1, max: 60 }, ..BASE },
            KeySpec { name: "cleanup.enabled", section: "cleanup", path: &["cleanup", "enabled"], kind: Kind::Bool, default: DefaultValue::Yaml("true"), fail_closed: Some(closed_off), ..BASE },
            KeySpec { name: "cleanup.interval", section: "cleanup", path: &["cleanup", "interval"], kind: Kind::Int { min: 1, max: 60 }, ..BASE },
        ];
        static S: Schema = Schema { component: "t", files: &[FileSpec { id: "cfg", retired: &[] }], sections: SECTIONS, keys: KEYS };

        fn read(text: &str) -> Layer {
            Layer::from_text(&S, "p", "cfg", LayerScope::Project, Some(Path::new("/p.yaml")), text)
        }

        fn get(l: &Layer, p: &str) -> Option<Value> {
            l.get(&p.split('.').map(str::to_string).collect::<Vec<_>>()).cloned()
        }

        fn yaml(s: &str) -> Option<Value> {
            Some(serde_yaml::from_str(s).unwrap())
        }

        #[test]
        fn a_failed_unit_keeps_the_closed_reading_of_a_switch_inside_it() {
            let l = read("sensors:\n  git: {enabled: maybe, interval: 5}\n  ps: {enabled: false, interval: 99}\n  ok: {interval: 3}\ncleanup:\n  enabled: 2\n  interval: 5\n");
            assert_eq!(get(&l, "sensors.git"), yaml("{enabled: false}"), "a bad switch reads off; its neighbour falls through");
            assert_eq!(get(&l, "sensors.ps"), yaml("{enabled: false}"), "a good off stays off beside a bad value");
            assert_eq!(get(&l, "sensors.ok.interval"), yaml("3"));
            assert_eq!(get(&l, "cleanup"), yaml("{enabled: false}"), "a section's nested switch fails closed too");
            // A switch that reads on has no closed reading: its unit falls through.
            let l = read("sensors:\n  git: {enabled: true, interval: 99}\n");
            assert_eq!(get(&l, "sensors.git"), None);
        }

        #[test]
        fn a_name_the_section_refuses_closes_the_section_in_that_file() {
            // `-ps` may be an off-switch the schema cannot read: the section
            // fails closed. Each declared instance and each name the file
            // gives reads off; nothing the file says in the section is read.
            let l = read("sensors:\n  -ps:\n  +mine: {enabled: true}\n  git: {interval: 4, enabled: true}\n");
            assert_eq!(get(&l, "sensors"), yaml("{a: {enabled: false}, b: {enabled: false}, git: {enabled: false}}"));
            let f: Vec<_> = l.findings.iter().map(|f| (f.unit.clone().unwrap_or_default(), f.line, f.message.clone(), f.closed)).collect();
            assert_eq!(f[0], ("sensors.-ps".into(), Some(2), "'-ps' is not a sensor name".into(), true), "{f:?}");
            assert_eq!((f[1].0.as_str(), f[1].3), ("sensors.+mine", true));
            let d = l.findings[0].diagnostic("t");
            assert!(d.contains("closes section sensors in this file") && d.contains("edited by hand"), "{d}");
            assert!(!d.contains("a switch stays off") && !d.contains("resolves from the layers beneath"), "{d}");
            // A refused name is repaired by hand only: the TUI offers no fix.
            assert!(l.findings[0].repair.is_some());
        }

        #[test]
        fn a_file_that_does_not_parse_closes_each_declared_instance() {
            let l = read("sensors:\n  a:\n    enabled: true\nx: [\n");
            assert_eq!(get(&l, "sensors"), yaml("{a: {enabled: false}, b: {enabled: false}}"));
            assert_eq!(get(&l, "cleanup"), yaml("{enabled: false}"));
        }
    }

    #[test]
    fn a_stored_value_a_computed_choice_lacks_is_a_finding_that_drops_nothing() {
        // The choices: `auto`, then every preset the layers name.
        fn presets(layers: &[Layer], _: &[String]) -> Result<Vec<String>, String> {
            let mut out = vec!["auto".to_string()];
            for l in layers {
                if let Some(Value::Mapping(m)) = l.accepted.get("presets") {
                    out.extend(m.keys().filter_map(Value::as_str).map(str::to_string));
                }
            }
            Ok(out)
        }
        let keys = [KeySpec { kind: Kind::ChoiceOf { options: presets, multi: false }, ..BASE }];
        let mut layers = vec![
            layer("user", LayerScope::User, "language: rare\npresets:\n  rare: 0.3\n"),
            layer("project", LayerScope::Project, "language: gone\n"),
        ];
        check_choices(&keys, &mut layers);
        assert!(layers[0].findings.is_empty(), "a preset another section of the file names");
        let f = &layers[1].findings[0];
        assert_eq!((f.key.as_deref(), f.fallback, f.section.as_deref()), (Some("language"), false, Some("general")));
        assert_eq!(f.message, "expected one of auto, rare, found 'gone'");
        assert_eq!(f.repair.as_deref(), Some("`ways settings set general.language <choice>`"));
        let d = f.diagnostic("t");
        assert!(d.ends_with("found 'gone'; it loads as written, and `ways settings set general.language <choice>` repairs it"), "{d}");
        assert!(!d.contains("resolve from the layers beneath") && !d.contains("ways settings fix"), "{d}");
        assert_eq!(layers[1].get(&["language".into()]), Some(&Value::String("gone".into())), "the value still loads");
    }

    #[test]
    fn a_byte_order_mark_is_not_content() {
        let l = layer("p", LayerScope::Project, "\u{feff}prob: 0.2\n");
        assert!(l.findings.is_empty(), "{:?}", l.findings);
        assert_eq!(l.get(&["prob".into()]), Some(&serde_yaml::from_str::<Value>("0.2").unwrap()));
        // With a real syntax error the file still fails closed, and the finding
        // names the error's line and cause, not "more than one document".
        let l = layer("p", LayerScope::Project, "\u{feff}prob: 0.2\nx: [\n");
        let f = &l.findings[0];
        assert!(f.is_parse_failure());
        assert!(f.line.is_some(), "{f}");
        assert!(!f.message.contains("more than one document"), "{f}");
    }
}
