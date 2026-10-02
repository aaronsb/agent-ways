//! emit, lint and fix.

use super::*;

// ── emit, lint, fix ────────────────────────────────────────────

pub fn emit(prefix: Option<&str>, effective: bool, project: Option<&Path>) -> Out {
    let reg = registry();
    let prefix = prefix.unwrap_or("");
    let layers = if effective { Some(live_layers(&project_dir(project))) } else { None };
    if let Some(l) = &layers {
        report(l);
    }
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

pub(super) fn file_label(file: &str) -> &'static str {
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

/// Whether `fix` must leave a top-level key alone: a read-only key, which
/// its action command owns, or one with no default, which canonical cannot
/// rebuild. Unsetting either would lose what the file holds, such as the
/// targets list.
fn kept_by_fix(schema: &agent_settings::Schema, file: &str, top: &str) -> bool {
    schema.keys.iter().filter(|k| k.file == file && k.path.first() == Some(&top)).any(|k| {
        matches!(k.kind, Kind::ReadOnly | Kind::Secret) || matches!(k.default, agent_settings::DefaultValue::None)
    })
}

/// `fix <section|prefix>`: rewrite each named section of the file from
/// canonical. In a per-entry section only the entries that fail are
/// dropped. A key fix cannot rebuild is left as it is, and said so.
pub fn fix(section: &str, project: Option<&Path>) -> Out {
    let reg = registry();
    let sections: Vec<_> = reg
        .sections()
        .filter(|(_, s)| s.name == section || agent_settings::registry::under(s.name, section))
        .collect();
    if sections.is_empty() {
        return Err(fail(exit::USAGE, format!("no section {section}; `ways settings help` lists them")));
    }
    let mut kept = Vec::new();
    let mut wrote = false;
    let mut by_file: Vec<(PathBuf, Vec<(&'static agent_settings::Schema, &'static agent_settings::SectionSpec)>)> = Vec::new();
    for (schema, sec) in sections {
        let path = if sec.file == ways_agent_core::settings::FILE {
            ways_agent_core::profile::user_layer_path()
        } else if sec.file == ways_core::settings::FILE {
            match project {
                Some(_) => ways_core::settings::project_file(&project_dir(project)),
                None => ways_core::paths::user_config(),
            }
        } else {
            continue;
        };
        match by_file.iter_mut().find(|(p, _)| *p == path) {
            Some((_, list)) => list.push((schema, sec)),
            None => by_file.push((path, vec![(schema, sec)])),
        }
    }
    for (path, list) in &by_file {
        let scope = if path.file_name().is_some_and(|n| n == "ways.yaml") { LayerScope::Project } else { LayerScope::User };
        let ((), changed) = agent_settings::writer::edit_file(path, None, |d| {
            for (schema, sec) in list {
                if sec.per_entry {
                    // Drop the entries that fail; the rest are the operator's.
                    let checked = agent_settings::load::check(schema, sec.file, scope, d.value(), Some(&[sec.name]));
                    for unit in &checked.failed {
                        let Some(entry) = unit.strip_prefix(&format!("{}.", sec.name)) else { continue };
                        for top in sec.top {
                            d.unset(&[top.to_string(), entry.to_string()])?;
                        }
                    }
                    continue;
                }
                let canonical = reg
                    .emit(sec.name, None)
                    .into_iter()
                    .find(|(f, _)| *f == sec.file)
                    .map(|(_, v)| v)
                    .unwrap_or(Value::Mapping(Default::default()));
                for top in sec.top {
                    if kept_by_fix(schema, sec.file, top) {
                        kept.push(format!("{} ({top})", sec.name));
                        continue;
                    }
                    let key = [top.to_string()];
                    match canonical.get(*top) {
                        Some(v) => d.set(&key, v)?,
                        None => {
                            d.unset(&key)?;
                        }
                    }
                }
            }
            Ok(())
        })
        .map_err(write_failed)?;
        wrote |= changed;
    }
    if !kept.is_empty() {
        let msg = format!(
            "fix leaves {}: an action command owns it, or canonical has no value for it; `ways settings help <key>` names the command",
            kept.join(", ")
        );
        if !wrote && by_file.iter().all(|(_, l)| l.iter().all(|(s, sec)| sec.top.iter().all(|t| kept_by_fix(s, sec.file, t)))) {
            return Err(fail(exit::REJECTED, msg));
        }
        eprintln!("ways settings: {msg}");
    }
    Ok(())
}
