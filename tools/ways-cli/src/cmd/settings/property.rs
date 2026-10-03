//! Property mode: one key at a time, reporting through exit codes.

use super::*;

// ── property mode ──────────────────────────────────────────────

pub fn get(key: &str, as_json: bool, file: Option<&Path>, project: Option<&Path>) -> Out {
    let reg = registry();
    let b = lookup(&reg, key)?;
    let layers = layers_for(file, project)?;
    report(&layers);
    let r = resolve(b.spec, &b.bound, &layers);
    if as_json {
        let mut d = describe(&r, b.spec, &layers, &b.bound);
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
    report(&layers);
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
        described.insert(r.name.clone(), describe(&r, b.spec, &layers, &b.bound));
    }
    let doc = json!({ "view": if effective { "effective" } else { "stored" }, "fragment": fragment, "keys": described });
    println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_default());
    Ok(())
}

pub fn set(key: &str, value: &str, project: Option<&Path>) -> Out {
    let reg = registry();
    let b = lookup(&reg, key)?;
    let layers = live_layers(&project_dir(project));
    let v = b
        .spec
        .parse_cli(value, &layers, &b.bound)
        .map_err(|m| fail(exit::REJECTED, format!("{key}: {m}; `ways settings help {key}`")))?;
    let (path, _) = target_file(&b, project)?;
    write_file(&path, &[(b.path(), v)])?;
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

/// Bare `ways settings`: the settings screens on a terminal, `list` in a
/// pipe (ADR-503 §9).
pub fn bare() -> Out {
    use std::io::IsTerminal;
    if std::io::stdout().is_terminal() && std::io::stdin().is_terminal() {
        return super::tui::open(&super::tui::Open::default());
    }
    list(None, false, false, None, None)
}

