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
    let notes = choice_notes(&reg, prefix, layers.as_deref());
    let many = files.len() > 1;
    // A key with no value emits no fragment; its note still says the choices.
    for (_, note) in notes.iter().filter(|(f, _)| !files.iter().any(|(e, _)| e == f)) {
        println!("# {note}");
    }
    for (i, (file, v)) in files.iter().enumerate() {
        if many {
            if i > 0 {
                println!("---");
            }
            println!("# {}", file_label(file));
        }
        for (_, note) in notes.iter().filter(|(f, _)| f == file) {
            println!("# {note}");
        }
        print!("{}", serde_yaml::to_string(v).unwrap_or_default());
    }
    Ok(())
}

/// A comment line per computed choice under `prefix`, by file kind: the
/// choices in effect, which the fragment's values cannot show. A key with
/// no value is still noted, since naming one of them is how it is set.
fn choice_notes(reg: &Registry, prefix: &str, layers: Option<&[Layer]>) -> Vec<(&'static str, String)> {
    let computed: Vec<_> = reg.concrete(prefix, layers.unwrap_or(&[])).into_iter().filter(|b| matches!(b.spec.kind, Kind::ChoiceOf { .. })).collect();
    if computed.is_empty() {
        return Vec::new();
    }
    // The choices come from the files, read for the canonical fragment too.
    let live;
    let layers = match layers {
        Some(l) => l,
        None => {
            live = live_layers(&project_dir(None));
            &live
        }
    };
    computed.iter().map(|b| (b.spec.file, choice_note(&b.name(), b.spec.kind, layers, &b.bound))).collect()
}

/// The `emit` comment of one computed choice, on one line whatever its
/// source answered: a newline would end the comment and the rest would
/// be read back by `apply` as settings.
pub(super) fn choice_note(name: &str, kind: Kind, layers: &[Layer], bound: &[String]) -> String {
    agent_settings::schema::one_line(&format!("{name}: {}", kind.describe_for(layers, bound)))
}

pub(super) fn file_label(file: &str) -> &'static str {
    match file {
        f if f == ways_agent_core::settings::FILE => "agent.yaml",
        f if f == attend_config::FILE => "attend/config.yaml (or a project's .claude/attend.yaml)",
        _ => "config.yaml (or a project's .claude/ways.yaml)",
    }
}

pub fn lint(file: Option<&Path>, project: Option<&Path>) -> Out {
    agent_settings::load::trace("lint");
    let layers = layers_for(file, project)?;
    let mut n = 0;
    // Findings `fix` can repair; the others name their own repair.
    let mut fixable = 0;
    for l in layers.iter().filter(|l| l.present) {
        for f in &l.findings {
            println!("{f}{}", f.lint_note());
            n += 1;
            fixable += f.lint_note().is_empty() as usize;
        }
    }
    if n > 0 {
        let s = if n == 1 { "" } else { "s" };
        let how = match fixable {
            0 => "each line names its repair",
            _ if fixable == n => "`ways settings fix <section>` repairs what a section's findings point at",
            _ => "`ways settings fix <section>` repairs what a section's findings point at; a line that names its own repair is not one of them",
        };
        return Err(fail(exit::REJECTED, format!("{n} finding{s}; {how}")));
    }
    Ok(())
}

/// The sections `fix <arg>` covers: the section named exactly, else every
/// section under the prefix. `fix gate` is `gate` alone, never `gate.mode`;
/// `fix install` is both `install.*` sections.
fn sections_for(reg: &Registry, arg: &str) -> Vec<(&'static agent_settings::Schema, &'static agent_settings::SectionSpec)> {
    let exact: Vec<_> = reg.sections().filter(|(_, s)| s.name == arg).collect();
    if !exact.is_empty() {
        return exact;
    }
    reg.sections().filter(|(_, s)| agent_settings::registry::under(s.name, arg)).collect()
}

/// `fix <section|prefix>`: repair what the named sections' findings point
/// at, and nothing else.
///
/// Each failing key path is repaired on its own:
/// - a switch takes its fail-closed reading, so a switch stays off;
/// - a read-only key is left for its action command, which is named;
/// - in a project or target file the bad key is removed, so the layers
///   beneath apply;
/// - in the user file it takes its canonical value, or is removed when it has
///   none;
/// - a key this file may not hold (a project-only toggle in the user file,
///   `targets` in a project file) is removed;
/// - an unknown key, a non-text key, or a value of the wrong shape is
///   removed.
///
/// An entry's name the section refuses is not repaired: it closes its
/// section, and only a hand edit can say what it meant. fix writes nothing
/// to that file and exits 5.
///
/// A top-level key no section owns is not removed: it may be a typo to
/// correct by hand. `fix ""` reports it and exits 3.
///
/// The file is checked again after the edit; exit 3 if a finding remains.
pub fn fix(section: &str, project: Option<&Path>) -> Out {
    let reg = registry();
    let sections = sections_for(&reg, section);
    if sections.is_empty() {
        return Err(fail(exit::USAGE, format!("no section {section}; `ways settings help` lists them")));
    }
    let mut by_file: Vec<(PathBuf, Vec<(&'static agent_settings::Schema, &'static agent_settings::SectionSpec)>)> = Vec::new();
    for (schema, sec) in sections {
        // A section's file: the user's, or with --project the project's. The
        // agent's sections live in the user file alone, --project or not.
        let (scope, at) = match sec.file == ways_agent_core::settings::FILE {
            true => (Scope::User, None),
            false => (Scope::Both, project),
        };
        let Some(Ok((path, _))) = file_of(sec.file, scope, at) else { continue };
        match by_file.iter_mut().find(|(p, _)| *p == path) {
            Some((_, list)) => list.push((schema, sec)),
            None => by_file.push((path, vec![(schema, sec)])),
        }
    }
    let mut left = Vec::new();
    for (path, list) in &by_file {
        let user_file = is_user_file(path);
        let scope = if user_file { LayerScope::User } else { LayerScope::Project };
        let names: Vec<&str> = list.iter().map(|(_, s)| s.name).collect();
        let file = list[0].1.file;
        let schema = list[0].0;
        let refused = agent_settings::writer::edit_file(path, None, |d| {
            let raw = d.value().clone();
            let checked = agent_settings::load::check(schema, file, scope, &raw, Some(&names));
            // A name the section refuses closes it, and may be an off-switch
            // the schema cannot read: only a hand edit can say what it meant.
            if checked.has_refused_name() {
                let text = d.text();
                return Ok(checked.findings(None, Some(path), &text).into_iter().filter(|f| f.closed).collect::<Vec<_>>());
            }
            for (sec, key) in checked.failing() {
                if sec.is_none() {
                    continue; // a top-level key no section owns; not this fix
                }
                let at = agent_settings::yaml_edit::value_at(&raw, &key).cloned();
                let spec = reg.lookup_path(file, &key);
                match (spec, at) {
                    // A key this file may not hold is ignored here anyway:
                    // removing it is the repair, whatever its kind.
                    (Some(b), _) if !scope.admits(b.spec.scope) => {
                        d.unset(&key)?;
                    }
                    (Some(b), _) if matches!(b.spec.kind, Kind::ReadOnly) => {}
                    (Some(b), Some(v)) if b.spec.fail_closed.is_some() && path_is_text(&raw, &key) => {
                        match (b.spec.fail_closed.expect("checked"))(&v) {
                            Some(c) => d.set(&key, &c)?,
                            None => {
                                d.unset(&key)?;
                            }
                        }
                    }
                    (Some(b), _) if user_file && path_is_text(&raw, &key) => match b.spec.default_for(&b.bound) {
                        Some(c) => d.set(&key, &c)?,
                        None => {
                            d.unset(&key)?;
                        }
                    },
                    _ => {
                        d.unset(&key)?;
                    }
                }
            }
            Ok(Vec::new())
        })
        .map_err(write_failed)?
        .0;
        if let Some(f) = refused.first() {
            let hint = f.message.split_once("; ").map_or("rename it, or delete it", |(_, h)| h);
            return Err(fail(
                exit::WRITE_FAILED,
                format!("{f}; fix cannot repair an entry's name, so nothing was written. Edit the file by hand: {hint}. Until then the section fails closed in that file"),
            ));
        }
        // Check again: what fix could not repair is reported, with its command.
        let text = std::fs::read_to_string(path).unwrap_or_default();
        if let Ok(doc) = agent_settings::load::parse_text(&text, Some(path)) {
            let after = agent_settings::load::check(schema, file, scope, &doc, Some(&names));
            // `fix ""` covers the whole file, so a key no section owns is
            // reported too; fix leaves it, since it may be a typo to correct.
            left.extend(after.findings(None, Some(path), &text).into_iter().filter(|f| f.section.is_some() || section.is_empty()));
        }
    }
    if !left.is_empty() {
        for f in &left {
            eprintln!("{}", f.diagnostic("ways"));
        }
        return Err(fail(exit::REJECTED, format!("{} finding(s) fix cannot repair; the line above names the command that can", left.len())));
    }
    Ok(())
}

/// Whether every segment of `key` names a text key in `raw`. A key written
/// as `123:` is not one; fix removes it rather than write under it.
fn path_is_text(raw: &Value, key: &[String]) -> bool {
    let mut cur = raw;
    for seg in key {
        let Some(m) = cur.as_mapping() else { return true };
        match m.get(seg.as_str()) {
            Some(c) => cur = c,
            None => return !m.keys().any(|k| !k.is_string() && agent_settings::schema::show(k) == *seg),
        }
    }
    true
}
