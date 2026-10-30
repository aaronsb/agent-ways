//! emit, lint and fix.

use super::*;

// ── emit, lint, fix ────────────────────────────────────────────

pub fn emit(prefix: Option<&str>, effective: bool, project: Option<&Path>) -> Out {
    let reg = registry();
    let prefix = prefix.unwrap_or("");
    let layers = if effective { Some(live_layers(read_project(project).as_deref())) } else { None };
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
    let canonical = layers.is_none();
    let live;
    let layers = match layers {
        Some(l) => l,
        None => {
            live = live_layers(project_dir(None).as_deref());
            &live
        }
    };
    // The canonical fragment shows each profile as shipped, so an instance's
    // note is read as shipped too: over what the files store it would name
    // the list of a provider the body does not show.
    computed
        .iter()
        .map(|b| {
            let over: &[Layer] = if canonical && !b.bound.is_empty() { &[] } else { layers };
            (b.spec.file, choice_note(&b.name(), b.spec.kind, over, &b.bound))
        })
        .collect()
}

/// The `emit` comment of one computed choice, on one line whatever its
/// source answered: a newline would end the comment and the rest would
/// be read back by `apply` as settings.
pub(super) fn choice_note(name: &str, kind: Kind, layers: &[Layer], bound: &[String]) -> String {
    agent_settings::schema::one_line(&format!("{name}: {}", kind.describe(layers, bound)))
}

pub(super) fn file_label(file: &str) -> &'static str {
    match file {
        f if f == ways_agent_core::settings::FILE => "agent.yaml",
        f if f == attend_config::FILE => "attend/config.yaml (or a project's .claude/attend.yaml)",
        f if f == attend_config::schema::SENSOR_FILE => "attend/sensors/<name>.yaml, one file per sensor (or a project's .claude/attend/sensors/)",
        _ => "config.yaml (or a project's .claude/ways.yaml)",
    }
}

/// Check the files. A finding exits 3; a warning is printed and leaves the
/// exit alone, since the file is valid and loads as written.
///
/// Without `--file`, a retired `attend_mod` section is reported in every
/// file that holds it ([`retired::files_holding`]), not only the files
/// layered here, each with its own fix command (ADR-512 item 2).
pub fn lint(file: Option<&Path>, project: Option<&Path>) -> Out {
    agent_settings::load::trace("lint");
    let layers = layers_for(file, project)?;
    let held = match file {
        // One file: its own line, with the fix command for its kind.
        Some(f) => retired::held_by(f).into_iter().collect(),
        None => retired::files_holding(read_project(project).as_deref()),
    };
    let mut n = 0;
    // Findings `fix` can repair; the others name their own repair.
    let mut fixable = 0;
    // A sensor file's findings come with its sensor's, below.
    for l in layers.iter().filter(|l| l.present && l.file != attend_config::schema::SENSOR_FILE) {
        for f in &l.findings {
            // Reported below with the command for its own file.
            if f.section.is_none() && f.key.as_deref() == Some(retired::KEY) && !held.is_empty() {
                continue;
            }
            println!("{f}{}", f.lint_note());
            n += 1;
            fixable += f.lint_note().is_empty() as usize;
        }
        for (is_error, line) in toggle_lines(l, &read_project(project).unwrap_or_else(|| PathBuf::from(crate::util::project_dir()))) {
            // A warning leaves the exit alone, as a key's does.
            n += usize::from(is_error);
            println!("{line}");
        }
    }
    // attend's sensor files (ADR-403), for the project, with one file alone
    // naming none of them.
    if file.is_none() {
        let dir = read_project(project).unwrap_or_else(|| PathBuf::from(crate::util::project_dir()));
        for line in attend_config::sensors::lint(&dir) {
            // A warning leaves the exit alone, as a key's does.
            if !line.contains(": warning: ") {
                n += 1;
            }
            println!("{line}");
        }
    }
    for h in &held {
        println!("{}", h.line());
        n += 1;
        fixable += 1;
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

/// The lines for the per-way toggles (ADR-131) in a project overlay, each as
/// `(is_error, line)`, key by key in file order, then the ties:
/// - an error for a key whose segment mixes text and a wildcard, which is
///   read literally and so switches nothing;
/// - a warning for a key `dir/*`, which now covers only the ways directly
///   under `dir`, naming the toggles for its old meaning;
/// - a warning for a key that names no way the project, the user or the
///   shipped corpus has, so a way renamed since it was written runs again;
/// - a warning for patterns tied at one specificity with opposite values,
///   which resolve to `false`.
///
/// Without the shipped ways the last two are not judged; `ways status`
/// says so.
fn toggle_lines(l: &Layer, fallback_project: &Path) -> Vec<(bool, String)> {
    use crate::cmd::toggle_check;
    let Some(path) = &l.path else { return Vec::new() };
    let keys = toggle_check::overlay_keys(l);
    if keys.is_empty() {
        return Vec::new();
    }
    let toggles = toggle_check::overlay_toggles(l);
    let ids = toggle_check::judged_ids(&toggle_check::project_of(path, fallback_project));
    let judged: Vec<&str> = keys.iter().map(String::as_str).filter(|k| !toggle_check::mixes_wildcard(k)).collect();
    let unmatched = ids.as_ref().map(|ids| toggle_check::unmatched(judged, ids)).unwrap_or_default();
    let text = String::from_utf8_lossy(&std::fs::read(path).unwrap_or_default()).into_owned();
    let at = |key: &str| format!("{}{}: [ways.project] ways.{key}:", path.display(), toggle_check::line_of(&text, key).map(|n| format!(":{n}")).unwrap_or_default());
    let mut out = Vec::new();
    for key in &keys {
        if toggle_check::mixes_wildcard(key) {
            out.push((true, format!("{} {}", at(key), toggle_check::MIXED_WILDCARD)));
        }
        if let Some(advice) = toggle_check::old_star_advice(key, &toggles, &keys) {
            out.push((false, format!("{} warning: {advice}", at(key))));
        }
        if let Some(u) = unmatched.iter().find(|u| u.key == *key) {
            let hint = u.nearest.as_ref().map(|n| format!("; nearest way: {n}")).unwrap_or_default();
            out.push((false, format!("{} warning: names no way in the project, user or shipped ways, so it switches nothing{hint}", at(key))));
        }
    }
    for (tied, id) in ids.map(|ids| toggle_check::ties(&toggles, &ids)).unwrap_or_default() {
        let (first, rest) = tied.split_first().expect("a tie has two keys");
        out.push((false, format!(
            "{} warning: ties with {} on {id} at the same specificity with the opposite value, so the tie resolves to false",
            at(first),
            rest.iter().map(|k| format!("ways.{k}")).collect::<Vec<_>>().join(", ")
        )));
    }
    out
}

/// A section `fix <arg>` covers, and the entry of it `arg` names when it
/// names one entry of a per-entry section (`mods.attend`).
type Covered = (&'static agent_settings::Schema, &'static agent_settings::SectionSpec, Option<String>);

/// The sections `fix <arg>` covers: the section named exactly, else every
/// section under the prefix, else the per-entry section whose entry `arg`
/// names. `fix gate` is `gate` alone, never `gate.mode`; `fix install` is
/// both `install.*` sections; `fix mods.attend` is the `mods` section's
/// `attend` entry.
fn sections_for(reg: &Registry, arg: &str) -> Vec<Covered> {
    let exact: Vec<_> = reg.sections().filter(|(_, s)| s.name == arg).map(|(sc, s)| (sc, s, None)).collect();
    if !exact.is_empty() {
        return exact;
    }
    let under: Vec<_> = reg.sections().filter(|(_, s)| agent_settings::registry::under(s.name, arg)).map(|(sc, s)| (sc, s, None)).collect();
    if !under.is_empty() {
        return under;
    }
    reg.sections()
        .filter(|(_, s)| s.per_entry)
        .filter_map(|(sc, s)| Some((sc, s, Some(arg.strip_prefix(s.name)?.strip_prefix('.')?.to_string()))))
        .filter(|(_, _, e)| e.as_deref().is_some_and(|e| !e.is_empty()))
        .collect()
}

/// Whether `fix <arg>` moves the retired `attend_mod` section: when it
/// covers the `mods` section's `attend` entry.
fn carries(covered: &[Covered]) -> bool {
    covered.iter().any(|(sc, s, e)| {
        std::ptr::eq(*sc, &ways_core::settings::SCHEMA) && s.name == "mods" && e.as_deref().is_none_or(|e| e == "attend")
    })
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
/// `fix mods.attend` (or `fix mods`) first moves a retired `attend_mod`
/// section to `mods.attend` (ADR-512 item 2): each value is written under the
/// new name unless the file already holds one there, which stays; fix names
/// each value it dropped, and removes the old section.
///
/// An entry's name the section refuses is not repaired: it closes its
/// section, and only a hand edit can say what it meant. fix writes nothing
/// to that file and exits 5.
///
/// A top-level key no section owns is not removed: it may be a typo to
/// correct by hand. `fix ""` reports it and exits 3.
///
/// The file is the user's, the project's with `--project`, or a target's
/// own config with `--target`. It is checked again after the edit; exit 3
/// if a finding remains.
pub fn fix(section: &str, project: Option<&Path>, target: Option<&Path>) -> Out {
    let project = resolve_project(project)?;
    let project = project.as_deref();
    let target = target.map(target_config).transpose()?;
    let reg = registry();
    let sections = sections_for(&reg, section);
    if sections.is_empty() {
        return Err(fail(exit::USAGE, format!("no section {section}; `ways settings help` lists them")));
    }
    // `mods.<name>` names a mod the app ships.
    if let Some(m) = section.strip_prefix("mods.").map(|r| r.split('.').next().unwrap_or_default()) {
        if !ways_core::settings::MODS.iter().any(|(n, _)| *n == m) {
            let names: Vec<&str> = ways_core::settings::MODS.iter().map(|(n, _)| *n).collect();
            return Err(fail(exit::USAGE, format!("no mod {m}; the shipped mods are {}", names.join(", "))));
        }
    }
    let carry = carries(&sections);
    let mut by_file: Vec<(PathBuf, LayerScope, Vec<Covered>)> = Vec::new();
    for covered in sections {
        let (_, sec, _) = covered;
        let (path, scope) = match &target {
            // A target's config holds ways' sections alone.
            Some(t) if sec.file == ways_core::settings::FILE => (t.clone(), LayerScope::Target),
            Some(_) => continue,
            None => {
                // A section's file: the user's, or with --project the project's.
                // The agent's sections live in the user file alone, --project or not.
                let (scope, at) = match sec.file == ways_agent_core::settings::FILE {
                    true => (Scope::User, None),
                    false => (Scope::Both, project),
                };
                let Some(Ok((path, _))) = file_of(sec.file, scope, at) else { continue };
                let scope = if is_user_file(&path) { LayerScope::User } else { LayerScope::Project };
                (path, scope)
            }
        };
        match by_file.iter_mut().find(|(p, _, _)| *p == path) {
            Some((_, _, list)) => list.push(covered),
            None => by_file.push((path, scope, vec![covered])),
        }
    }
    if by_file.is_empty() {
        let why = match target {
            Some(_) => format!("{section} is not kept in a target's config; drop --target"),
            // Only the sensor files have no file of their section to fix.
            None => format!("{section} is kept in each sensor's own file, which fix does not write; edit the file, and `attend sensor list` shows its state"),
        };
        return Err(fail(exit::USAGE, why));
    }
    let mut left = Vec::new();
    for (path, scope, list) in &by_file {
        let (scope, user_file) = (*scope, *scope == LayerScope::User);
        let names: Vec<&str> = list.iter().map(|(_, s, _)| s.name).collect();
        let entries: Vec<(&str, Option<&str>)> = list.iter().map(|(_, s, e)| (s.name, e.as_deref())).collect();
        let file = list[0].1.file;
        let schema = list[0].0;
        let carry_here = carry && file == ways_core::settings::FILE;
        let (refused, carried) = agent_settings::writer::edit_file(path, None, |d| {
            // A name the section refuses closes it, and may be an off-switch
            // the schema cannot read: only a hand edit can say what it meant.
            // Checked before anything moves, so a refusal writes nothing.
            let before = agent_settings::load::check(schema, file, scope, d.value(), Some(&names));
            if before.has_refused_name() {
                let text = d.text();
                let refused = before.findings(None, Some(path), &text).into_iter().filter(|f| f.closed).collect::<Vec<_>>();
                return Ok((refused, retired::Carried::default()));
            }
            let carried = if carry_here { retired::carry(d)? } else { retired::Carried::default() };
            let raw = d.value().clone();
            let checked = agent_settings::load::check(schema, file, scope, &raw, Some(&names));
            for (sec, key) in checked.failing() {
                let Some(sec) = sec else {
                    continue; // a top-level key no section owns; not this fix
                };
                if !in_entry(&entries, sec, &key) {
                    continue; // another entry of a per-entry section
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
            Ok((Vec::new(), carried))
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
        touched(scope, path, false);
        for d in &carried.dropped {
            eprintln!("{}", d.line(path));
        }
        if carried.comments_dropped {
            eprintln!(
                "dropped the comments inside attend_mod in {}: the file already has a mods section, so the values moved one by one",
                path.display()
            );
        }
        // Check again: what fix could not repair is reported, with its command.
        let text = std::fs::read_to_string(path).unwrap_or_default();
        if let Ok(doc) = agent_settings::load::parse_text(&text, Some(path)) {
            let after = agent_settings::load::check(schema, file, scope, &doc, Some(&names));
            // `fix ""` covers the whole file, so a key no section owns is
            // reported too; fix leaves it, since it may be a typo to correct.
            left.extend(after.findings(None, Some(path), &text).into_iter().filter(|f| match (&f.section, &f.key) {
                (Some(s), Some(k)) => in_entry(&entries, s, &k.split('.').map(str::to_string).collect::<Vec<_>>()),
                (Some(_), None) => true,
                (None, _) => section.is_empty(),
            }));
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

/// Whether a failing `key` of section `sec` is one `fix` was asked to
/// repair: the section is covered whole, or the key sits in the entry
/// named. A key path's first segment is the section's top-level key, and an
/// entry's name may hold dots (`gate.profiles.my.fast`), so the rest of the
/// path is matched as one dotted name.
fn in_entry(entries: &[(&str, Option<&str>)], sec: &str, key: &[String]) -> bool {
    entries.iter().any(|(name, entry)| {
        *name == sec && entry.is_none_or(|e| key.len() > 1 && agent_settings::registry::under(&key[1..].join("."), e))
    })
}

/// The config of the target at `dir`, as `targets:` in the user file names
/// it. A directory that is no target is exit 2.
fn target_config(dir: &Path) -> Result<PathBuf, Failure> {
    retired::targets()
        .into_iter()
        .find(|t| t.matches_dir(dir))
        .map(|t| t.config_path())
        .ok_or_else(|| fail(exit::USAGE, format!("--target {}: not a target; `ways manage target list` lists them", dir.display())))
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
