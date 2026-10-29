//! Object mode: a settings object in, a JSON report out.

use super::*;

// ── object mode ────────────────────────────────────────────────

/// The accepted keys bound for one file, with their values.
type Accepted = Vec<(Bound, Value)>;

/// `apply`: write a settings object. Prints a JSON report of what each key
/// did and the fragment written; exits 3 when any key is rejected, 4 when an
/// accepted key is overridden, 5 when a write fails.
pub fn apply(file: Option<&Path>, dry_run: bool, project: Option<&Path>) -> Out {
    let project = resolve_project(project)?;
    let project = project.as_deref();
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
    // A choice is checked against the files and the object together: a
    // profile the object adds may be the engine it names.
    let mut layers = live_layers(project_dir(project).as_deref());
    for doc in &docs {
        let text = serde_yaml::to_string(doc).unwrap_or_default();
        let schema = &ways_agent_core::settings::SCHEMA;
        layers.push(Layer::from_text(schema, "apply", ways_agent_core::settings::FILE, LayerScope::User, None, &text));
    }
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
            walk(&reg, &layers, sec.file, &mut vec![top], v, &mut accepted, &mut rejected, &mut findings, project);
        }
    }
    // The engine is written through the engine choice, with the judge mod's
    // wanted state (ADR-512 item 4), after the rest.
    let mut engine = accepted.iter().position(|(b, _)| b.name() == "gate.engine").map(|i| accepted.remove(i));
    // The engine in effect, set again (an emit then apply round trip),
    // changes nothing and is no new choice.
    if let Some((_, v)) = &engine {
        use ways_agent_core::profile::{user_layer_path, UserLayer};
        let now = UserLayer::load(&user_layer_path()).ok().and_then(|u| u.engine);
        if now.as_deref() == v.as_str() {
            findings.push(json!({ "key": "gate.engine", "ok": true, "message": "unchanged" }));
            engine = None;
        }
    }
    // Group the accepted keys by the file they are written to.
    let mut by_file: Vec<(PathBuf, LayerScope, Accepted)> = Vec::new();
    for (b, v) in accepted {
        match target_file(&b, project) {
            Ok((p, scope)) => match by_file.iter_mut().find(|(f, _, _)| *f == p) {
                Some((_, _, list)) => list.push((b, v)),
                None => by_file.push((p, scope, vec![(b, v)])),
            },
            Err(e) => {
                rejected.push(json!({ "key": b.name(), "message": e.message }));
            }
        }
    }
    let mut written = serde_json::Map::new();
    let mut accepted_names = Vec::new();
    let mut error = None;
    for (path, scope, list) in &by_file {
        let mut fragment = json!({});
        for (b, v) in list {
            insert_path(&mut fragment, &file_key(b), to_json(v));
            accepted_names.push(b.name());
        }
        if !dry_run {
            let edits: Vec<(Vec<String>, Option<Value>)> = list.iter().map(|(b, v)| (file_key(b), Some(v.clone()))).collect();
            match edit_checked(path, list.iter().any(|(b, _)| is_sensor_key(b)), &edits, None, |_| Ok(())) {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => {
                    error = Some(format!("{e}; nothing written to {}", path.display()));
                    break;
                }
                Err(f) => {
                    error = Some(f.message);
                    break;
                }
            }
        }
        touched(*scope, path, dry_run);
        written.insert(path.display().to_string(), fragment);
    }
    if let (Some((b, v)), None) = (&engine, &error) {
        accepted_names.push(b.name());
        match engine_write(v, dry_run) {
            Ok(files) => {
                for (scope, path, fragment) in files {
                    touched(scope, &path, dry_run);
                    let key = path.display().to_string();
                    match written.get_mut(&key) {
                        Some(f) => merge_json(f, fragment),
                        None => {
                            written.insert(key, fragment);
                        }
                    }
                }
                if !dry_run && written.contains_key(&ways_core::paths::user_config().display().to_string()) {
                    eprintln!("{}", super::engine::REPAIR_HINT);
                }
            }
            Err(f) => error = Some(f.message),
        }
    }
    let mut over = Vec::new();
    if !dry_run && error.is_none() {
        for (path, _, list) in &by_file {
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

/// The engine `v` names (null clears it), written through the engine choice,
/// or under `dry_run` only described. Returns each file with its layer and
/// the fragment written there.
fn engine_write(v: &Value, dry_run: bool) -> Result<Vec<(LayerScope, PathBuf, serde_json::Value)>, Failure> {
    let engine = v.as_str();
    let keyless = engine.is_some_and(super::engine::is_keyless);
    let install = |on: bool| json!({ "mods": { "ways-judge": { "install": on } } });
    let fragment = |scope: LayerScope, path: PathBuf| {
        let user = path == ways_core::paths::user_config();
        let f = if user { install(keyless) } else { json!({ "engine": to_json(v) }) };
        (scope, path, f)
    };
    if !dry_run {
        return Ok(super::write_engine(engine)?.into_iter().map(|(s, p)| fragment(s, p)).collect());
    }
    let agent = (LayerScope::User, ways_agent_core::profile::user_layer_path());
    let user = (LayerScope::User, ways_core::paths::user_config());
    let leaving = ways_core::settings::mod_install_in_user("ways-judge") == Some(true);
    let files = match (keyless, leaving) {
        (true, _) => vec![user, agent],
        (false, true) => vec![agent, user],
        (false, false) => vec![agent],
    };
    Ok(files.into_iter().map(|(s, p)| fragment(s, p)).collect())
}

/// Merge `b` into the object `a`, key by key.
fn merge_json(a: &mut serde_json::Value, b: serde_json::Value) {
    match (a, b) {
        (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
            for (k, v) in b {
                match a.get_mut(&k) {
                    Some(slot) => merge_json(slot, v),
                    None => {
                        a.insert(k, v);
                    }
                }
            }
        }
        (a, b) => *a = b,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn walk(
    reg: &Registry,
    layers: &[Layer],
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
        // The user config's judge install follows the engine. An object
        // holding the value in effect, as `emit` prints it, changes nothing
        // and is passed over; any other value is refused, as `set` refuses it.
        if name == super::engine::JUDGE_INSTALL && project.is_none() {
            let r = agent_settings::load::resolve(b.spec, &b.bound, layers);
            if r.value.or(r.default).as_ref() == Some(v) {
                findings.push(json!({ "key": name, "ok": true, "message": "unchanged; it follows gate.engine" }));
                return;
            }
        }
        let verdict = match b.spec.kind {
            Kind::ReadOnly => Err("read-only; it is changed by its action command".to_string()),
            Kind::Secret => Err("a secret is never set from a settings object".to_string()),
            // A null engine clears it, through the engine choice.
            _ if name == "gate.engine" && v.is_null() => Ok(()),
            _ => b.spec.check_value_in(v, Some(layers), &b.bound),
        }
        .and_then(|_| super::engine::refuse(&name).map_err(|f| f.message))
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
                walk(reg, layers, file, path, cv, accepted, rejected, findings, project);
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

