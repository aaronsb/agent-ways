//! Per-section checking and layered loading (ADR-503 §3-5).
//!
//! A file is parsed once. Each top-level key belongs to a section; a section
//! any of whose values fails the schema is dropped from that file's layer, so
//! its keys resolve from the layers beneath, which end at canonical. Every
//! other section loads as written. A diagnostic names the file, line and
//! section, and is built only when something fails.

use crate::schema::{KeySpec, LayerScope, Schema};
use crate::yaml_edit::{self, Doc};
use serde_yaml::{Mapping, Value};
use std::path::{Path, PathBuf};

/// One problem in one file. `fallback` is set when it made a section load as
/// canonical.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub file: Option<PathBuf>,
    pub line: Option<usize>,
    pub section: Option<String>,
    pub key: Option<String>,
    pub message: String,
    pub fallback: bool,
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let file = self.file.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "<input>".into());
        write!(f, "{file}")?;
        if let Some(l) = self.line {
            write!(f, ":{l}")?;
        }
        if let Some(s) = &self.section {
            write!(f, ": [{s}]")?;
        }
        if let Some(k) = &self.key {
            write!(f, " {k}")?;
        }
        write!(f, ": {}", self.message)
    }
}

impl Finding {
    /// The one-line stderr form: the finding, the fallback, and the repair.
    pub fn diagnostic(&self, tool: &str) -> String {
        match (&self.section, self.fallback) {
            (Some(s), true) => format!(
                "[{tool}] settings: {self}; section {s} falls back to canonical. \
                 `ways settings lint` lists the findings, `ways settings fix {s}` rewrites it"
            ),
            _ => format!("[{tool}] settings: {self}"),
        }
    }
}

/// A section check failure: the section, and the key path and message of
/// each value that failed.
#[derive(Debug, Clone)]
struct Failure {
    section: Option<&'static str>,
    path: Vec<String>,
    message: String,
}

/// The outcome of checking one parsed file.
#[derive(Debug, Clone, Default)]
pub struct Checked {
    /// The top-level keys of every section that passed.
    pub accepted: Mapping,
    /// Sections that fell back.
    pub failed: Vec<&'static str>,
    failures: Vec<Failure>,
}

impl Checked {
    pub fn is_clean(&self) -> bool {
        self.failures.is_empty()
    }

    /// The findings, with line numbers looked up in `text`.
    pub fn findings(&self, schema_name: Option<&str>, file: Option<&Path>, text: &str) -> Vec<Finding> {
        let doc = Doc::parse(text).ok();
        self.failures
            .iter()
            .map(|f| Finding {
                file: file.map(Path::to_path_buf),
                line: doc.as_ref().and_then(|d| d.line_of(&f.path)),
                section: f.section.map(str::to_string),
                key: (!f.path.is_empty()).then(|| f.path.join(".")),
                message: f.message.clone(),
                fallback: f.section.is_some_and(|s| self.failed.contains(&s)),
            })
            .map(|mut f| {
                if f.section.is_none() {
                    if let Some(n) = schema_name {
                        f.message = format!("{} ({n})", f.message);
                    }
                }
                f
            })
            .collect()
    }
}

/// Parse a settings file's text. A parse failure is one finding on the line
/// the parser names; every section of the file then falls back.
pub fn parse_text(text: &str, file: Option<&Path>) -> Result<Value, Finding> {
    yaml_edit::parse(text).map_err(|e| {
        let (line, message) = match e {
            yaml_edit::EditError::Parse { line, message } => (line, format!("does not parse: {message}")),
            other => (None, other.to_string()),
        };
        Finding {
            file: file.map(Path::to_path_buf),
            line,
            section: None,
            key: None,
            message: format!("{message}; every section of the file falls back to canonical"),
            fallback: true,
        }
    })
}

/// Check a parsed file of kind `file` at `scope` against `schema`. With
/// `only`, the sections not named are skipped: not checked and not loaded.
pub fn check(schema: &Schema, file: &str, scope: LayerScope, doc: &Value, only: Option<&[&str]>) -> Checked {
    trace_sections(schema, file, only);
    let mut out = Checked::default();
    let Some(root) = doc.as_mapping() else { return out };
    let keys: Vec<&KeySpec> = schema.keys.iter().filter(|k| k.file == file && k.computed.is_none()).collect();
    let retired = schema.file(file).map(|f| f.retired).unwrap_or(&[]);
    for (k, v) in root {
        let Some(top) = k.as_str() else {
            out.failures.push(Failure { section: None, path: vec![], message: format!("a top-level key is not text: {}", crate::schema::show(k)) });
            continue;
        };
        let Some(sec) = schema.section_of_top(file, top) else {
            if only.is_none() {
                let message = match retired.iter().find(|(r, _)| *r == top) {
                    Some((_, m)) => (*m).to_string(),
                    None => "unknown key".to_string(),
                };
                out.failures.push(Failure { section: None, path: vec![top.to_string()], message });
            }
            continue;
        };
        if only.is_some_and(|o| !o.contains(&sec.name)) {
            continue;
        }
        let mut errs = Vec::new();
        check_tree(&keys, scope, &mut vec![top.to_string()], v, &mut errs);
        if !errs.is_empty() && !out.failed.contains(&sec.name) {
            out.failed.push(sec.name);
        }
        out.failures.extend(errs.into_iter().map(|(path, message)| Failure { section: Some(sec.name), path, message }));
    }
    for (k, v) in root {
        let Some(top) = k.as_str() else { continue };
        if let Some(sec) = schema.section_of_top(file, top) {
            if !out.failed.contains(&sec.name) && only.is_none_or(|o| o.contains(&sec.name)) {
                out.accepted.insert(k.clone(), v.clone());
            }
        }
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
                    errs.push((path.clone(), format!("a key is not text: {}", crate::schema::show(kk))));
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

// ── trace ──────────────────────────────────────────────────────

/// `WAYS_SETTINGS_TRACE=1` prints each section load to stderr, so a test can
/// see what a hook path loads (ADR-503 §5). Read once per process.
fn tracing() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("WAYS_SETTINGS_TRACE").is_some_and(|v| !v.is_empty()))
}

fn trace_sections(schema: &Schema, file: &str, only: Option<&[&str]>) {
    if tracing() {
        let names: Vec<&str> = match only {
            Some(o) => o.to_vec(),
            None => schema.sections.iter().filter(|s| s.file == file).map(|s| s.name).collect(),
        };
        eprintln!("settings-trace: load {}:{file} [{}]", schema.component, names.join(","));
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
        let text = std::fs::read_to_string(path).ok();
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
            Err(f) => (Mapping::new(), vec![f]),
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
        SectionSpec { name: "general", file: "cfg", top: &["language"], doc: "" },
        SectionSpec { name: "matching", file: "cfg", top: &["prob", "presets"], doc: "" },
        SectionSpec { name: "toggles", file: "cfg", top: &["ways"], doc: "" },
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
        assert!(l.get(&["ways".into()]).is_none());
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
}
