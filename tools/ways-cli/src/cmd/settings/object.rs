//! Object mode: a settings object in, a JSON report out.

use super::*;

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
    // A choice is checked against the files and the object together: a
    // profile the object adds may be the engine it names.
    let mut layers = live_layers(&project_dir(project));
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
        let verdict = match b.spec.kind {
            Kind::ReadOnly => Err("read-only; it is changed by its action command".to_string()),
            Kind::Secret => Err("a secret is never set from a settings object".to_string()),
            _ => b.spec.check_value_in(v, Some(layers)),
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

