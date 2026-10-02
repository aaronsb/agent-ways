//! `ways settings` (ADR-503 §9): the ways and agent schemas composed into one
//! tree, read and written through the files.
//!
//! Property mode (`get`, `set`, `unset`, `list`) is terse and reports through
//! exit codes: 0 done, 2 usage or unknown key, 3 rejected by the schema,
//! 4 written but overridden by a higher layer, 5 write failed. A non-zero
//! code prints one line on stderr naming the problem and the next command.
//! Object mode (`apply`) takes a settings object and answers with JSON.

use agent_settings::exit;
use agent_settings::load::{resolve, Layer, Resolved};
use agent_settings::{Bound, Kind, LayerScope, Registry, Scope};
use serde_json::json;
use serde_yaml::Value;
use std::path::{Path, PathBuf};

/// A failure carrying its exit code and the one stderr line.
pub struct Failure {
    pub code: i32,
    pub message: String,
}

type Out = Result<(), Failure>;

fn fail(code: i32, message: impl Into<String>) -> Failure {
    Failure { code, message: message.into() }
}

/// Run a settings verb and exit with its code.
pub fn exit_with(r: Out) -> ! {
    match r {
        Ok(()) => std::process::exit(exit::OK),
        Err(f) => {
            eprintln!("ways settings: {}", f.message);
            std::process::exit(f.code)
        }
    }
}

pub fn registry() -> Registry {
    Registry::new(vec![&ways_core::settings::SCHEMA, &ways_agent_core::settings::SCHEMA])
}

fn project_dir(opt: Option<&Path>) -> PathBuf {
    match opt {
        Some(p) => p.to_path_buf(),
        None => PathBuf::from(
            std::env::var("CLAUDE_PROJECT_DIR")
                .ok()
                .filter(|s| !s.is_empty())
                .or_else(|| std::env::var("PWD").ok())
                .unwrap_or_else(|| ".".into()),
        ),
    }
}

/// The live layers of every file, lowest first.
fn live_layers(project: &Path) -> Vec<Layer> {
    agent_settings::load::trace("tree");
    let mut out = ways_core::settings::layers(project);
    out.extend(ways_agent_core::settings::layers());
    out
}

/// One file read on its own (`--file`): `agent.yaml` is the agent kind, a
/// `ways.yaml` a project overlay, anything else a user-scope config.
fn file_layers(path: &Path) -> Result<Vec<Layer>, Failure> {
    if !path.is_file() {
        return Err(fail(exit::USAGE, format!("{} is not a file", path.display())));
    }
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let layer = if name == "agent.yaml" {
        Layer::read(&ways_agent_core::settings::SCHEMA, "file", ways_agent_core::settings::FILE, LayerScope::User, path)
    } else {
        let scope = if name == "ways.yaml" { LayerScope::Project } else { LayerScope::User };
        Layer::read(&ways_core::settings::SCHEMA, "file", ways_core::settings::FILE, scope, path)
    };
    Ok(vec![layer])
}

fn layers_for(file: Option<&Path>, project: Option<&Path>) -> Result<Vec<Layer>, Failure> {
    match file {
        Some(f) => file_layers(f),
        None => Ok(live_layers(&project_dir(project))),
    }
}

fn lookup(reg: &Registry, key: &str) -> Result<Bound, Failure> {
    reg.lookup(key).ok_or_else(|| fail(exit::USAGE, format!("unknown key {key}; `ways settings list` names the keys")))
}

/// A value as `get` and `list` print it: text bare, anything else as JSON.
fn plain(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(other) => serde_json::to_string(&to_json(other)).unwrap_or_default(),
    }
}

fn to_json(v: &Value) -> serde_json::Value {
    serde_json::to_value(v).unwrap_or(serde_json::Value::Null)
}

fn layer_label(r: &Resolved, layers: &[Layer]) -> (String, Option<String>) {
    match r.layer {
        Some(i) => (layers[i].name.clone(), layers[i].path.as_ref().map(|p| p.display().to_string())),
        None if r.value.is_some() && r.default.is_none() => ("computed".into(), None),
        None => ("default".into(), None),
    }
}

fn describe(r: &Resolved, layers: &[Layer]) -> serde_json::Value {
    let (layer, file) = layer_label(r, layers);
    json!({
        "value": r.value.as_ref().map(to_json),
        "default": r.default.as_ref().map(to_json),
        "layer": layer,
        "file": file,
    })
}

// ── property mode ──────────────────────────────────────────────

pub fn get(key: &str, as_json: bool, file: Option<&Path>, project: Option<&Path>) -> Out {
    let reg = registry();
    let b = lookup(&reg, key)?;
    let layers = layers_for(file, project)?;
    let r = resolve(b.spec, &b.bound, &layers);
    if as_json {
        let mut d = describe(&r, &layers);
        d["key"] = json!(r.name);
        println!("{}", serde_json::to_string_pretty(&d).unwrap_or_default());
    } else {
        println!("{}", plain(r.value.as_ref()));
    }
    Ok(())
}

pub fn list(prefix: Option<&str>, as_json: bool, effective: bool, file: Option<&Path>, project: Option<&Path>) -> Out {
    let reg = registry();
    let prefix = prefix.unwrap_or("");
    let layers = layers_for(file, project)?;
    let keys = reg.concrete(prefix, &layers);
    if keys.is_empty() {
        return Err(fail(exit::USAGE, format!("no key under {prefix}; `ways settings list` names the keys")));
    }
    if !as_json {
        for b in &keys {
            let r = resolve(b.spec, &b.bound, &layers);
            if b.spec.kind != Kind::Secret || r.value.is_some() {
                println!("{}={}", r.name, plain(r.value.as_ref()));
            }
        }
        return Ok(());
    }
    // ADR-185 §3: the stored view holds what the files set, one fragment per
    // file, keyed by its path; --effective holds every key resolved, one
    // fragment per file kind. Each fragment has its file's shape.
    let mut fragment = serde_json::Map::new();
    let mut described = serde_json::Map::new();
    for b in &keys {
        let r = resolve(b.spec, &b.bound, &layers);
        let stored = r.layer.is_some();
        if !effective && !stored {
            continue;
        }
        if let (Some(v), true) = (&r.value, b.spec.computed.is_none()) {
            let group = match (effective, layer_label(&r, &layers).1) {
                (false, Some(path)) => path,
                _ => b.spec.file.to_string(),
            };
            let file = fragment.entry(group).or_insert_with(|| json!({}));
            insert_path(file, &r.path, to_json(v));
        }
        described.insert(r.name.clone(), describe(&r, &layers));
    }
    let doc = json!({ "view": if effective { "effective" } else { "stored" }, "fragment": fragment, "keys": described });
    println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_default());
    Ok(())
}

fn insert_path(root: &mut serde_json::Value, path: &[String], v: serde_json::Value) {
    let mut cur = root;
    for seg in &path[..path.len() - 1] {
        if !cur.get(seg).is_some_and(|x| x.is_object()) {
            cur[seg.as_str()] = json!({});
        }
        cur = &mut cur[seg.as_str()];
    }
    cur[path[path.len() - 1].as_str()] = v;
}

/// The file a key is written to, and the layer scope it is written at.
fn target_file(b: &Bound, project: Option<&Path>) -> Result<(PathBuf, LayerScope), Failure> {
    let name = b.name();
    match b.spec.file {
        f if f == ways_agent_core::settings::FILE => {
            if project.is_some() {
                return Err(fail(exit::USAGE, format!("{name} is set in the user file only; drop --project")));
            }
            Ok((ways_agent_core::profile::user_layer_path(), LayerScope::User))
        }
        f if f == ways_core::settings::FILE => match (b.spec.scope, project) {
            (Scope::User, Some(_)) => Err(fail(exit::USAGE, format!("{name} is set in the user file only; drop --project"))),
            (Scope::User, None) | (Scope::Both, None) => Ok((ways_core::paths::user_config(), LayerScope::User)),
            (Scope::Project, p) | (Scope::Both, p @ Some(_)) => {
                Ok((ways_core::settings::project_file(&project_dir(p)), LayerScope::Project))
            }
        },
        _ => Err(fail(exit::REJECTED, format!("{name} is not stored in a settings file; `ways settings help {name}`"))),
    }
}

fn header_for(path: &Path) -> Option<&'static str> {
    (path.file_name().is_some_and(|n| n == "ways.yaml")).then_some("# Project-scope ways overlay — see ADR-115, ADR-131\n")
}

fn write_failed(e: agent_settings::writer::WriteError) -> Failure {
    fail(exit::WRITE_FAILED, format!("{e}; nothing written"))
}

/// After a write to `written`, whether `b` resolves from somewhere else.
fn overridden(b: &Bound, written: &Path, project: Option<&Path>) -> Option<String> {
    let layers = live_layers(&project_dir(project));
    let r = resolve(b.spec, &b.bound, &layers);
    let from = r.layer.and_then(|i| layers[i].path.clone());
    if from.as_deref() == Some(written) {
        return None;
    }
    let written_layer = layers.iter().find(|l| l.path.as_deref() == Some(written));
    if let Some(f) = written_layer.and_then(|l| l.findings.iter().find(|f| f.section.as_deref() == Some(b.spec.section) && f.fallback)) {
        return Some(format!(
            "{} falls back to canonical in {}: {f}; `ways settings lint` lists the findings",
            b.spec.section,
            written.display()
        ));
    }
    match from {
        Some(p) => Some(format!("{} sets {}; `ways settings get {} --json` shows the value in effect", p.display(), r.name, r.name)),
        None => None,
    }
}

pub fn set(key: &str, value: &str, project: Option<&Path>) -> Out {
    let reg = registry();
    let b = lookup(&reg, key)?;
    let v = b
        .spec
        .parse_cli(value)
        .map_err(|m| fail(exit::REJECTED, format!("{key}: {m}; `ways settings help {key}`")))?;
    let (path, _) = target_file(&b, project)?;
    let kp = b.path();
    agent_settings::writer::edit_file(&path, header_for(&path), |d| d.set(&kp, &v)).map_err(write_failed)?;
    if let Some(why) = overridden(&b, &path, project) {
        return Err(fail(exit::OVERRIDDEN, format!("{key} written to {}, but {why}", path.display())));
    }
    Ok(())
}

pub fn unset(key: &str, project: Option<&Path>) -> Out {
    let reg = registry();
    let b = lookup(&reg, key)?;
    if matches!(b.spec.kind, Kind::ReadOnly | Kind::Secret) {
        return Err(fail(exit::REJECTED, format!("{key} is changed by its action command; `ways settings help {key}`")));
    }
    let (path, _) = target_file(&b, project)?;
    let kp = b.path();
    agent_settings::writer::edit_file(&path, None, |d| d.unset(&kp)).map_err(write_failed)?;
    let layers = live_layers(&project_dir(project));
    let r = resolve(b.spec, &b.bound, &layers);
    if let Some(p) = r.layer.and_then(|i| layers[i].path.clone()) {
        if is_higher(&layers, &p, &path) {
            return Err(fail(exit::OVERRIDDEN, format!("{key} removed from {}, but {} still sets it", path.display(), p.display())));
        }
    }
    Ok(())
}

/// Whether the layer at `a` sits above the one at `b`.
fn is_higher(layers: &[Layer], a: &Path, b: &Path) -> bool {
    let pos = |p: &Path| layers.iter().position(|l| l.path.as_deref() == Some(p));
    match (pos(a), pos(b)) {
        (Some(x), Some(y)) => x > y,
        (Some(_), None) => true,
        _ => false,
    }
}

// ── help ───────────────────────────────────────────────────────

/// The long help for a key or section: the text the settings TUI's detail
/// pane shows (ADR-503 §10).
pub fn help(topic: Option<&str>) -> Out {
    let reg = registry();
    let Some(topic) = topic else {
        println!("ways settings: the settings of ways and its agent, read and written through their files.\n");
        println!("  get <key>            the value in effect (--json: with its layer, default and file)");
        println!("  set <key> <value>    write it (--project <dir> for a project's ways.yaml)");
        println!("  unset <key>          remove it, so the layer below applies");
        println!("  list [prefix]        key=value lines (--json: stored, or --effective)");
        println!("  emit [prefix]        the canonical fragment (--effective: the values in effect)");
        println!("  apply                write a settings object from stdin or --file; answers in JSON");
        println!("  lint                 check the files; exit 3 with findings");
        println!("  fix <section>        write a section's canonical fragment");
        println!("  help <key|section>   what a key or section does\n");
        println!("exit codes: 0 done, 2 usage or unknown key, 3 rejected, 4 overridden by a higher layer, 5 write failed\n");
        println!("sections:");
        for (_, s) in reg.sections() {
            println!("  {:<16} {}", s.name, s.doc);
        }
        return Ok(());
    };
    if let Some(b) = reg.lookup(topic).or_else(|| {
        // A pattern key by its own name, `ways.project.*`.
        reg.keys().find(|(_, k)| k.name == topic).map(|(schema, spec)| Bound { schema, spec, bound: vec![] })
    }) {
        let s = b.spec;
        println!("{}", if b.bound.is_empty() { s.name.to_string() } else { b.name() });
        println!("  {}", s.doc);
        println!("  type:    {}", s.kind.describe());
        if let Some(d) = s.default_for(&b.bound) {
            println!("  default: {}", plain(Some(&d)));
        }
        println!("  scope:   {}", s.scope.as_str());
        if s.computed.is_none() {
            println!("  file:    {} key {}", s.file, s.path.join("."));
        }
        if !s.long.is_empty() {
            println!();
            println!("{}", wrap(s.long, 76));
        }
        return Ok(());
    }
    if let Some((schema, sec)) = reg.section(topic) {
        println!("{}: {}", sec.name, sec.doc);
        for k in schema.keys_of_section(sec.name) {
            println!("  {:<40} {}", k.name, k.doc);
        }
        return Ok(());
    }
    Err(fail(exit::USAGE, format!("no key or section {topic}; `ways settings help` lists the sections")))
}

fn wrap(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            out.push_str("  ");
            out.push_str(&line);
            out.push('\n');
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push_str("  ");
        out.push_str(&line);
    }
    out
}

// ── emit, lint, fix ────────────────────────────────────────────

pub fn emit(prefix: Option<&str>, effective: bool, project: Option<&Path>) -> Out {
    let reg = registry();
    let prefix = prefix.unwrap_or("");
    let layers = if effective { Some(live_layers(&project_dir(project))) } else { None };
    if reg.concrete(prefix, layers.as_deref().unwrap_or(&[])).is_empty() {
        return Err(fail(exit::USAGE, format!("no key under {prefix}; `ways settings help` lists the sections")));
    }
    let files = reg.emit(prefix, layers.as_deref());
    let many = files.len() > 1;
    for (i, (file, v)) in files.iter().enumerate() {
        if many {
            if i > 0 {
                println!("---");
            }
            println!("# {}", file_label(file));
        }
        print!("{}", serde_yaml::to_string(v).unwrap_or_default());
    }
    Ok(())
}

fn file_label(file: &str) -> &'static str {
    if file == ways_agent_core::settings::FILE {
        "agent.yaml"
    } else {
        "config.yaml (or a project's .claude/ways.yaml)"
    }
}

pub fn lint(file: Option<&Path>, project: Option<&Path>) -> Out {
    agent_settings::load::trace("lint");
    let layers = layers_for(file, project)?;
    let mut n = 0;
    for l in layers.iter().filter(|l| l.present) {
        for f in &l.findings {
            println!("{f}");
            n += 1;
        }
    }
    if n > 0 {
        return Err(fail(exit::REJECTED, format!("{n} finding{}; `ways settings fix <section>` rewrites a section from canonical", if n == 1 { "" } else { "s" })));
    }
    Ok(())
}

pub fn fix(section: &str, project: Option<&Path>) -> Out {
    let reg = registry();
    let (_, sec) = reg
        .section(section)
        .ok_or_else(|| fail(exit::USAGE, format!("no section {section}; `ways settings help` lists them")))?;
    let path = if sec.file == ways_agent_core::settings::FILE {
        ways_agent_core::profile::user_layer_path()
    } else if sec.file == ways_core::settings::FILE {
        match project {
            Some(_) => ways_core::settings::project_file(&project_dir(project)),
            None => ways_core::paths::user_config(),
        }
    } else {
        return Err(fail(exit::USAGE, format!("{section} is not stored in a settings file")));
    };
    let canonical = reg
        .emit(section, None)
        .into_iter()
        .find(|(f, _)| *f == sec.file)
        .map(|(_, v)| v)
        .unwrap_or(Value::Mapping(Default::default()));
    agent_settings::writer::edit_file(&path, None, |d| {
        for top in sec.top {
            let key = [top.to_string()];
            match canonical.get(*top) {
                Some(v) => d.set(&key, v)?,
                None => {
                    d.unset(&key)?;
                }
            }
        }
        Ok(())
    })
    .map_err(write_failed)?;
    Ok(())
}

// ── object mode ────────────────────────────────────────────────

/// `apply`: write a settings object. Prints a JSON report of what each key
/// did and the fragment written; exits 3 when any key is rejected, 4 when an
/// accepted key is overridden, 5 when a write fails.
pub fn apply(file: Option<&Path>, dry_run: bool, project: Option<&Path>) -> Out {
    let text = match file {
        Some(p) => std::fs::read_to_string(p).map_err(|e| fail(exit::USAGE, format!("reading {}: {e}", p.display())))?,
        None => {
            let mut s = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut s).map_err(|e| fail(exit::USAGE, format!("reading stdin: {e}")))?;
            s
        }
    };
    let mut docs = Vec::new();
    for d in serde_yaml::Deserializer::from_str(&text) {
        let v = <Value as serde::Deserialize>::deserialize(d).map_err(|e| fail(exit::USAGE, format!("the settings object does not parse: {e}")))?;
        match v {
            Value::Null => {}
            Value::Mapping(_) => docs.push(v),
            _ => return Err(fail(exit::USAGE, "a settings object is a mapping shaped like the file, as `ways settings emit` prints")),
        }
    }
    let reg = registry();
    let mut findings = Vec::new();
    let mut accepted: Vec<(Bound, Value)> = Vec::new();
    let mut rejected = Vec::new();
    for doc in &docs {
        let m = doc.as_mapping().expect("mappings only");
        for (k, v) in m {
            let top = k.as_str().unwrap_or_default().to_string();
            let Some((_, sec)) = reg.owner_of_top(&top) else {
                rejected.push(json!({ "key": top, "message": "unknown key" }));
                findings.push(json!({ "key": top, "ok": false, "message": "unknown key" }));
                continue;
            };
            walk(&reg, sec.file, &mut vec![top], v, &mut accepted, &mut rejected, &mut findings, project);
        }
    }
    // Group the accepted keys by the file they are written to.
    let mut by_file: Vec<(PathBuf, Vec<(Bound, Value)>)> = Vec::new();
    for (b, v) in accepted {
        match target_file(&b, project) {
            Ok((p, _)) => match by_file.iter_mut().find(|(f, _)| *f == p) {
                Some((_, list)) => list.push((b, v)),
                None => by_file.push((p, vec![(b, v)])),
            },
            Err(e) => {
                rejected.push(json!({ "key": b.name(), "message": e.message }));
            }
        }
    }
    let mut written = serde_json::Map::new();
    let mut accepted_names = Vec::new();
    let mut error = None;
    for (path, list) in &by_file {
        let mut fragment = json!({});
        for (b, v) in list {
            insert_path(&mut fragment, &b.path(), to_json(v));
            accepted_names.push(b.name());
        }
        if !dry_run {
            let r = agent_settings::writer::edit_file(path, header_for(path), |d| {
                for (b, v) in list {
                    d.set(&b.path(), v)?;
                }
                Ok(())
            });
            if let Err(e) = r {
                error = Some(format!("{e}; nothing written to {}", path.display()));
                break;
            }
        }
        written.insert(path.display().to_string(), fragment);
    }
    let mut over = Vec::new();
    if !dry_run && error.is_none() {
        for (path, list) in &by_file {
            for (b, _) in list {
                if let Some(why) = overridden(b, path, project) {
                    over.push(json!({ "key": b.name(), "why": why }));
                }
            }
        }
    }
    let report = json!({
        "dry_run": dry_run,
        "accepted": accepted_names,
        "rejected": rejected,
        "findings": findings,
        "written": written,
        "overridden": over,
        "error": error,
    });
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
    if let Some(e) = error {
        return Err(fail(exit::WRITE_FAILED, e));
    }
    if !report["rejected"].as_array().is_some_and(|a| a.is_empty()) {
        return Err(fail(exit::REJECTED, "some keys were rejected; see \"rejected\" in the report"));
    }
    if !over.is_empty() {
        return Err(fail(exit::OVERRIDDEN, "some keys are overridden by a higher layer; see \"overridden\" in the report"));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn walk(
    reg: &Registry,
    file: &str,
    path: &mut Vec<String>,
    v: &Value,
    accepted: &mut Vec<(Bound, Value)>,
    rejected: &mut Vec<serde_json::Value>,
    findings: &mut Vec<serde_json::Value>,
    project: Option<&Path>,
) {
    if let Some(b) = reg.lookup_path(file, path) {
        let name = b.name();
        let verdict = match b.spec.kind {
            Kind::ReadOnly => Err("read-only; it is changed by its action command".to_string()),
            Kind::Secret => Err("a secret is never set from a settings object".to_string()),
            _ => b.spec.check_value(v),
        }
        .and_then(|_| target_file(&b, project).map(|_| ()).map_err(|f| f.message));
        match verdict {
            Ok(()) => {
                findings.push(json!({ "key": name, "ok": true }));
                accepted.push((b, v.clone()));
            }
            Err(m) => {
                findings.push(json!({ "key": name, "ok": false, "message": m }));
                rejected.push(json!({ "key": name, "message": m }));
            }
        }
        return;
    }
    let deeper = reg.keys().any(|(_, k)| {
        k.file == file && k.path.len() > path.len() && k.path.iter().zip(path.iter()).all(|(p, s)| *p == "*" || p == s)
    });
    match (v, deeper) {
        (Value::Mapping(m), true) => {
            for (k, cv) in m {
                path.push(k.as_str().unwrap_or_default().to_string());
                walk(reg, file, path, cv, accepted, rejected, findings, project);
                path.pop();
            }
        }
        _ => {
            let name = path.join(".");
            let message = if deeper { "expected a mapping" } else { "unknown key" };
            findings.push(json!({ "key": name, "ok": false, "message": message }));
            rejected.push(json!({ "key": name, "message": message }));
        }
    }
}

/// Bare `ways settings`: the TUI is a later increment (ADR-504), so a
/// terminal and a pipe both get `list`.
pub fn bare() -> Out {
    list(None, false, false, None, None)
}

