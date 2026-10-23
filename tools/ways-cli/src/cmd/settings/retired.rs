//! The retired `attend_mod` section (ADR-512 item 2). Nothing configures
//! anything from it. `lint` and `ways manage update` name every file that
//! still holds it, with the command that moves it; `fix mods.attend` moves
//! it when the operator runs that command. Nothing else rewrites the files.

use std::path::{Path, PathBuf};

use agent_settings::yaml_edit::{value_at, Doc, EditError};
use serde_yaml::Value;

/// The retired top-level key.
pub const KEY: &str = "attend_mod";

/// The entry it moves to, as a key path.
const TO: [&str; 2] = ["mods", "attend"];

/// Which kind of file holds the section, and so which fix command moves it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    User,
    /// A target's own config; the target's directory.
    Target(PathBuf),
    /// A project's `.claude/ways.yaml`; the project's root.
    Project(PathBuf),
}

/// One file that holds `attend_mod`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    pub path: PathBuf,
    pub line: Option<usize>,
    pub kind: Kind,
}

impl Held {
    /// The command that moves this file's section.
    pub fn fix(&self) -> String {
        let quote = |p: &Path| crate::cmd::lifecycle::shell_word(&p.display().to_string());
        match &self.kind {
            Kind::User => "ways settings fix mods.attend".to_string(),
            Kind::Target(dir) => format!("ways settings fix mods.attend --target {}", quote(dir)),
            Kind::Project(dir) => format!("ways settings fix mods.attend --project {}", quote(dir)),
        }
    }

    /// The finding as `lint` prints it: the file, the line, the key, and the
    /// message with this file's fix command.
    pub fn line(&self) -> String {
        let at = self.line.map(|l| format!(":{l}")).unwrap_or_default();
        let message = ways_core::settings::ATTEND_MOD_RETIRED.replace("`ways settings fix mods.attend`", &format!("`{}`", self.fix()));
        format!("{}{at}: {KEY}: {message}", self.path.display())
    }
}

/// The line `attend_mod` starts on in `path`, when the file holds it.
/// `None` when it does not, or the file is absent or does not parse: a file
/// that does not parse has its own finding.
fn holds(path: &Path) -> Option<Option<usize>> {
    let text = std::fs::read_to_string(path).ok()?;
    let doc = Doc::parse(text.strip_prefix('\u{feff}').unwrap_or(&text)).ok()?;
    let key = [KEY.to_string()];
    doc.get(&key)?;
    Some(doc.line_of(&key))
}

/// Every file that holds `attend_mod`: the user config, each target's config,
/// the project `project` names, and the `.claude/ways.yaml` of every project
/// Claude Code knows (`ways projects`), each once.
pub fn files_holding(project: Option<&Path>) -> Vec<Held> {
    let mut candidates: Vec<(PathBuf, Kind)> = vec![(ways_core::paths::user_config(), Kind::User)];
    for t in targets() {
        candidates.push((t.config_path(), Kind::Target(t.dir())));
    }
    let mut projects: Vec<PathBuf> = project.map(Path::to_path_buf).into_iter().collect();
    projects.extend(known_projects(&ways_core::paths::transcripts_root()));
    for p in projects {
        candidates.push((ways_core::settings::project_file(&p), Kind::Project(p)));
    }
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut out = Vec::new();
    for (path, kind) in candidates {
        let c = canon(&path);
        if seen.contains(&c) {
            continue;
        }
        seen.push(c);
        if let Some(line) = holds(&path) {
            out.push(Held { path, line, kind });
        }
    }
    out
}

/// The targets the user config lists, read without reporting its findings:
/// a `lint` reports them once, and an update has its own report.
pub fn targets() -> Vec<ways_core::config::Target> {
    use agent_settings::{Layer, LayerScope};
    let user = Layer::read(&ways_core::settings::SCHEMA, "user", ways_core::settings::FILE, LayerScope::User, &ways_core::paths::user_config());
    user.get(&["targets".into()])
        .and_then(|v| serde_yaml::from_value(v.clone()).ok())
        .unwrap_or_else(|| ways_core::config::Config::default().targets())
}

/// The root of every project Claude Code has a transcripts directory for
/// under `root`: the project root its settings resolve to, else the
/// directory itself.
fn known_projects(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    let mut out: Vec<PathBuf> = Vec::new();
    for e in entries.flatten() {
        if !e.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let Some(dir) = claude_sessions::resolve_project_path(root, &name) else { continue };
        let dir = PathBuf::from(dir);
        let project = ways_core::settings::project_root(&dir).unwrap_or(dir);
        if !out.contains(&project) {
            out.push(project);
        }
    }
    out
}

/// Move `attend_mod` to `mods.attend` in `d`: each value is written under
/// the new name unless the file already holds one there, and the old
/// section is removed. Returns the dotted names of the values dropped
/// because `mods.attend` already held them, or the whole section when it
/// was not a mapping.
pub fn carry(d: &mut Doc) -> Result<Vec<String>, EditError> {
    let key = [KEY.to_string()];
    let Some(old) = d.get(&key).cloned() else { return Ok(Vec::new()) };
    let mut dropped = Vec::new();
    match &old {
        Value::Mapping(_) => {
            let mut leaves = Vec::new();
            leaves_of(&old, &mut Vec::new(), &mut leaves);
            for (rel, v) in leaves {
                let to: Vec<String> = TO.iter().map(|s| s.to_string()).chain(rel.iter().cloned()).collect();
                if value_at(d.value(), &to).is_some() {
                    dropped.push(format!("{KEY}.{}", rel.join(".")));
                } else {
                    d.set(&to, &v)?;
                }
            }
        }
        Value::Null => {}
        _ => dropped.push(KEY.to_string()),
    }
    d.unset(&key)?;
    Ok(dropped)
}

/// Each value under `v` that is not a non-empty mapping, with its path.
fn leaves_of(v: &Value, at: &mut Vec<String>, out: &mut Vec<(Vec<String>, Value)>) {
    match v {
        Value::Mapping(m) if !m.is_empty() => {
            for (k, child) in m {
                let name = k.as_str().map(str::to_string).unwrap_or_else(|| agent_settings::schema::show(k));
                at.push(name);
                leaves_of(child, at, out);
                at.pop();
            }
        }
        _ if at.is_empty() => {}
        other => out.push((at.clone(), other.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn carried(text: &str) -> (String, Vec<String>) {
        let mut d = Doc::parse(text).unwrap();
        let dropped = carry(&mut d).unwrap();
        d.verify().unwrap();
        (d.text(), dropped)
    }

    #[test]
    fn the_section_moves_under_its_new_name() {
        assert_eq!(carried("attend_mod:\n  listening: active\n"), ("mods:\n  attend:\n    listening: active\n".to_string(), vec![]));
        let (text, dropped) = carried("# mine\nattend_mod:\n  keepalive:\n    windows: [\"Sat 10:00-14:00\"]\n    enabled: false\n");
        assert!(dropped.is_empty());
        assert!(text.starts_with("# mine\n"), "{text}");
        let v: Value = serde_yaml::from_str(&text).unwrap();
        assert_eq!(v, serde_yaml::from_str::<Value>("mods: {attend: {keepalive: {windows: [\"Sat 10:00-14:00\"], enabled: false}}}").unwrap());
    }

    #[test]
    fn a_value_already_under_the_new_name_is_kept_and_the_old_one_named() {
        let (text, dropped) = carried("mods:\n  attend:\n    listening: dormant\nattend_mod:\n  listening: active\n  keepwarm: off\n");
        let v: Value = serde_yaml::from_str(&text).unwrap();
        assert_eq!(v, serde_yaml::from_str::<Value>("mods: {attend: {listening: dormant, keepwarm: 'off'}}").unwrap());
        assert_eq!(dropped, ["attend_mod.listening"]);
    }

    #[test]
    fn a_file_without_the_section_is_left_as_it_is() {
        assert_eq!(carried("mods:\n  attend:\n    listening: active\n"), ("mods:\n  attend:\n    listening: active\n".to_string(), vec![]));
    }
}
