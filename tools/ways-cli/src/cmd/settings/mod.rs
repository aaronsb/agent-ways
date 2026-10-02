//! `ways settings` (ADR-503 §9): the ways and agent schemas composed into one
//! tree, read and written through the files.
//!
//! Property mode (`get`, `set`, `unset`, `list`) is terse and reports through
//! exit codes: 0 done, 2 usage or unknown key, 3 rejected by the schema,
//! 4 written but overridden by a higher layer, 5 write failed. A non-zero
//! code prints one line on stderr naming the problem and the next command.
//! Object mode (`apply`) takes a settings object and answers with JSON.
//!
//! `property` holds get, set, unset and list; `object` apply; `maintain`
//! emit, lint and fix; `help` the schema's text; `tui` the settings screens
//! on `agent-tui` (ADR-504 §8). This module holds what they share: the
//! registry, the layers, where a key is written, and the one write path.

mod help;
mod maintain;
mod object;
mod property;
pub mod tui;

pub use help::{help, help_text};
pub use maintain::{emit, fix, lint};
pub use object::apply;
pub use property::{bare, get, list, set, unset};

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

pub(super) type Out = Result<(), Failure>;

pub(super) fn fail(code: i32, message: impl Into<String>) -> Failure {
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

pub(super) fn project_dir(opt: Option<&Path>) -> PathBuf {
    match opt {
        Some(p) => p.to_path_buf(),
        None => PathBuf::from(crate::util::project_dir()),
    }
}

/// The live layers of every file, lowest first.
pub(super) fn live_layers(project: &Path) -> Vec<Layer> {
    agent_settings::load::trace("tree");
    let mut out = ways_core::settings::layers(project);
    out.extend(ways_agent_core::settings::layers());
    out
}

/// One file read on its own (`--file`): `agent.yaml` is the agent kind, a
/// `ways.yaml` a project overlay, anything else a user-scope config.
pub(super) fn file_layers(path: &Path) -> Result<Vec<Layer>, Failure> {
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

pub(super) fn layers_for(file: Option<&Path>, project: Option<&Path>) -> Result<Vec<Layer>, Failure> {
    match file {
        Some(f) => file_layers(f),
        None => Ok(live_layers(&project_dir(project))),
    }
}

pub(super) fn lookup(reg: &Registry, key: &str) -> Result<Bound, Failure> {
    reg.lookup(key).ok_or_else(|| fail(exit::USAGE, format!("unknown key {key}; `ways settings list` names the keys")))
}

/// A value as `get` and `list` print it: text bare, anything else as JSON.
pub(super) fn plain(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(other) => serde_json::to_string(&to_json(other)).unwrap_or_default(),
    }
}

pub(super) fn to_json(v: &Value) -> serde_json::Value {
    serde_json::to_value(v).unwrap_or(serde_json::Value::Null)
}

pub(super) fn layer_label(r: &Resolved, layers: &[Layer]) -> (String, Option<String>) {
    match r.layer {
        Some(i) => (layers[i].name.clone(), layers[i].path.as_ref().map(|p| p.display().to_string())),
        None if r.value.is_some() && r.default.is_none() => ("computed".into(), None),
        None => ("default".into(), None),
    }
}

pub(super) fn describe(r: &Resolved, layers: &[Layer]) -> serde_json::Value {
    let (layer, file) = layer_label(r, layers);
    json!({
        "value": r.value.as_ref().map(to_json),
        "default": r.default.as_ref().map(to_json),
        "layer": layer,
        "file": file,
    })
}

/// The file a key is written to, and the layer scope it is written at.
pub(super) fn target_file(b: &Bound, project: Option<&Path>) -> Result<(PathBuf, LayerScope), Failure> {
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

/// Write `values`, each a key path and a typed value, into one settings file
/// in one locked edit. `set` writes through here, and so does the settings
/// screens' apply, so both leave the same bytes. Returns whether the file
/// changed.
pub(super) fn write_file(path: &Path, values: &[(Vec<String>, Value)]) -> Result<bool, Failure> {
    agent_settings::writer::edit_file(path, header_for(path), |d| {
        for (k, v) in values {
            d.set(k, v)?;
        }
        Ok(())
    })
    .map(|((), changed)| changed)
    .map_err(write_failed)
}

pub(super) fn header_for(path: &Path) -> Option<&'static str> {
    (path.file_name().is_some_and(|n| n == "ways.yaml")).then_some("# Project-scope ways overlay — see ADR-115, ADR-131\n")
}

pub(super) fn write_failed(e: agent_settings::writer::WriteError) -> Failure {
    use agent_settings::writer::WriteError;
    use agent_settings::yaml_edit::EditError;
    match &e {
        // The writer edits text it can parse; a file that does not parse is
        // repaired by hand, and until then it fails closed.
        WriteError::Edit(_, EditError::Parse { .. } | EditError::NotMapping) => fail(
            exit::WRITE_FAILED,
            format!("{e}; nothing written. Fix the file's syntax by hand; until then it fails closed"),
        ),
        _ => fail(exit::WRITE_FAILED, format!("{e}; nothing written")),
    }
}

/// After a write to `written`, whether `b` resolves from somewhere else.
pub(super) fn overridden(b: &Bound, written: &Path, project: Option<&Path>) -> Option<String> {
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

/// Whether the layer at `a` sits above the one at `b`.
pub(super) fn is_higher(layers: &[Layer], a: &Path, b: &Path) -> bool {
    let pos = |p: &Path| layers.iter().position(|l| l.path.as_deref() == Some(p));
    match (pos(a), pos(b)) {
        (Some(x), Some(y)) => x > y,
        (Some(_), None) => true,
        _ => false,
    }
}

pub(super) fn insert_path(root: &mut serde_json::Value, path: &[String], v: serde_json::Value) {
    let mut cur = root;
    for seg in &path[..path.len() - 1] {
        if !cur.get(seg).is_some_and(|x| x.is_object()) {
            cur[seg.as_str()] = json!({});
        }
        cur = &mut cur[seg.as_str()];
    }
    cur[path[path.len() - 1].as_str()] = v;
}


/// Print each loaded file's findings on stderr, one line each, as a load
/// on any other command does (ADR-503 §4). stdout stays the value.
pub(super) fn report(layers: &[Layer]) {
    for l in layers.iter().filter(|l| l.present) {
        for f in &l.findings {
            eprintln!("{}", f.diagnostic("ways"));
        }
    }
}
