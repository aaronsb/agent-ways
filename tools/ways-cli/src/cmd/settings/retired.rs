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
    /// Any other file, read with `--file`: moved by hand.
    Other,
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
            Kind::Other => "renaming attend_mod to mods.attend by hand".to_string(),
        }
    }

    /// The line an update prints for it: where `attend_mod` is, and the
    /// command that moves it.
    pub fn moved_line(&self) -> String {
        let at = self.line.map(|l| format!(":{l}")).unwrap_or_default();
        format!("{}{at} holds attend_mod, now mods.attend: `{}` moves it", self.path.display(), self.fix())
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

/// The one file `path`, when it holds `attend_mod`: a project's
/// `.claude/ways.yaml`, a target's config, the user config, or any other
/// file, each with the fix command for its kind.
pub fn held_by(path: &Path) -> Option<Held> {
    let line = holds(path)?;
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let me = canon(path);
    let kind = if me == canon(&ways_core::paths::user_config()) {
        Kind::User
    } else if let Some(t) = targets().into_iter().find(|t| canon(&t.config_path()) == me) {
        Kind::Target(t.dir())
    } else if path.file_name().is_some_and(|n| n == "ways.yaml") && path.parent().and_then(Path::file_name).is_some_and(|n| n == ".claude") {
        // The root as a person types it, never canonicalize's verbatim form.
        Kind::Project(ways_core::util::strip_verbatim(me.parent().and_then(Path::parent).unwrap_or(Path::new(""))))
    } else {
        Kind::Other
    };
    Some(Held { path: path.to_path_buf(), line, kind })
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

/// One value `fix` left behind when it moved the section, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    /// Its dotted name under `attend_mod`.
    pub name: String,
    pub why: Why,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// `mods.attend` already holds a value at this name, which stays.
    Held(String),
    /// The section is not a mapping: this value, shown.
    NotMapping(String),
    /// A part of the new path holds a value that is not a mapping, which
    /// stays: its dotted name and the value, shown.
    Shape(String, String),
}

impl Dropped {
    /// The line `fix` prints for it, naming the file it worked on.
    pub fn line(&self, file: &Path) -> String {
        let file = file.display();
        match &self.why {
            Why::Held(to) => format!("dropped {}: {to} already holds a value in {file}, which stays", self.name),
            Why::NotMapping(v) => format!("dropped {}: it is `{v}`, not a section of keys, so nothing moved from {file}", self.name),
            Why::Shape(at, v) => format!("dropped {}: {at} is `{v}` in {file}, not a section of keys, and it stays", self.name),
        }
    }
}

/// What moving the section did besides moving it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Carried {
    pub dropped: Vec<Dropped>,
    /// Comments inside the old section were dropped with it, because the
    /// values moved one by one into an existing `mods` section.
    pub comments_dropped: bool,
}

/// The lines of `text` the top-level `attend_mod` key spans: its own line,
/// then each line indented under it, ending before the first line at the
/// left margin and leaving the blank lines that close the block in place.
/// `None` when the key is not written at the left margin as `attend_mod:`.
fn block(text: &str) -> Option<std::ops::Range<usize>> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|l| l.strip_prefix(KEY).is_some_and(|r| r.trim_start().starts_with(':')))?;
    let mut end = start + 1;
    for (i, l) in lines.iter().enumerate().skip(start + 1) {
        if !l.is_empty() && !l.starts_with([' ', '\t']) {
            break;
        }
        if !l.trim().is_empty() {
            end = i + 1;
        }
    }
    Some(start..end)
}

/// `text` with the lines in `range` replaced by `with`.
fn splice(text: &str, range: std::ops::Range<usize>, with: &[String]) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = lines[..range.start].iter().map(|l| l.to_string()).collect();
    out.extend(with.iter().cloned());
    out.extend(lines[range.end..].iter().map(|l| l.to_string()));
    let mut joined = out.join("\n");
    if text.ends_with('\n') {
        joined.push('\n');
    }
    joined
}

/// Move `attend_mod` to `mods.attend` in `d`.
///
/// Where the file has no `mods` section the block is renamed in place: its
/// key becomes `mods:` / `  attend:` and every line under it moves two
/// columns right, so its comments, inline ones included, stay with their
/// values. Otherwise each value is written under the new name unless the
/// file already holds one there, or a part of the new path holds a value
/// that is not a mapping, and the old block goes with the comments inside
/// it. Returns what was dropped and why.
pub fn carry(d: &mut Doc) -> Result<Carried, EditError> {
    let key = [KEY.to_string()];
    let Some(old) = d.get(&key).cloned() else { return Ok(Carried::default()) };
    let mut out = Carried::default();
    if let Value::Mapping(_) | Value::Null = old {
    } else {
        out.dropped.push(Dropped { name: KEY.to_string(), why: Why::NotMapping(shown(&old)) });
        d.unset(&key)?;
        return Ok(out);
    }
    let text = d.text();
    let range = block(&text);
    // Renamed in place, when there is no `mods` to merge into and the
    // renamed text reads back as the same values.
    if let (None, Some(r)) = (d.get(&["mods".to_string()]), range.clone()) {
        let lines: Vec<&str> = text.lines().collect();
        let rest = lines[r.start].strip_prefix(KEY).unwrap_or_default().trim_start().strip_prefix(':').unwrap_or_default();
        let mut with = vec!["mods:".to_string(), format!("  attend:{rest}")];
        with.extend(lines[r.start + 1..r.end].iter().map(|l| if l.trim().is_empty() { l.to_string() } else { format!("  {l}") }));
        if let Ok(renamed) = Doc::parse(&splice(&text, r, &with)) {
            let to: Vec<String> = TO.iter().map(|s| s.to_string()).collect();
            if renamed.get(&key).is_none() && value_at(renamed.value(), &to) == Some(&old) {
                *d = renamed;
                return Ok(out);
            }
        }
    }
    // Value by value into the existing `mods`. The old block goes first,
    // comments and all, so none is left orphaned above the next key.
    match range {
        Some(r) => {
            out.comments_dropped = text.lines().skip(r.start + 1).take(r.end - r.start - 1).any(|l| l.trim_start().starts_with('#'))
                || text.lines().nth(r.start).is_some_and(|l| l.contains(" #"));
            *d = Doc::parse(&splice(&text, r, &[]))?;
        }
        None => {
            d.unset(&key)?;
        }
    }
    let mut leaves = Vec::new();
    leaves_of(&old, &mut Vec::new(), &mut leaves);
    for (rel, v) in leaves {
        let name = format!("{KEY}.{}", rel.join("."));
        let to: Vec<String> = TO.iter().map(|s| s.to_string()).chain(rel.iter().cloned()).collect();
        // A part of the path below `mods` that already holds a value that
        // is not a mapping keeps it.
        let blocked = (TO.len()..to.len()).find_map(|n| match value_at(d.value(), &to[..n]) {
            Some(Value::Mapping(_)) | None => None,
            Some(other) => Some((to[..n].join("."), shown(other))),
        });
        if let Some((at, shown)) = blocked {
            out.dropped.push(Dropped { name, why: Why::Shape(at, shown) });
        } else if value_at(d.value(), &to).is_some() {
            out.dropped.push(Dropped { name, why: Why::Held(to.join(".")) });
        } else {
            d.set(&to, &v)?;
        }
    }
    Ok(out)
}

/// A value as a message shows it: a string bare, anything else as YAML.
fn shown(v: &Value) -> String {
    v.as_str().map(str::to_string).unwrap_or_else(|| agent_settings::schema::show(v))
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

    fn carried(text: &str) -> (String, Carried) {
        let mut d = Doc::parse(text).unwrap();
        let c = carry(&mut d).unwrap();
        d.verify().unwrap();
        (d.text(), c)
    }

    fn names(c: &Carried) -> Vec<&str> {
        c.dropped.iter().map(|d| d.name.as_str()).collect()
    }

    #[test]
    fn the_section_moves_under_its_new_name() {
        assert_eq!(carried("attend_mod:\n  listening: active\n"), ("mods:\n  attend:\n    listening: active\n".to_string(), Carried::default()));
        let (text, c) = carried("# mine\nattend_mod:\n  keepalive:\n    windows: [\"Sat 10:00-14:00\"]\n    enabled: false\n");
        assert!(c.dropped.is_empty());
        assert!(text.starts_with("# mine\n"), "{text}");
        let v: Value = serde_yaml::from_str(&text).unwrap();
        assert_eq!(v, serde_yaml::from_str::<Value>("mods: {attend: {keepalive: {windows: [\"Sat 10:00-14:00\"], enabled: false}}}").unwrap());
    }

    /// Review finding 4: renamed in place, every comment stays with its value.
    #[test]
    fn a_section_renamed_in_place_keeps_its_comments() {
        let input = "# top\nattend_mod:\n  # why active\n  listening: active # inline\n  keepalive:\n    # weekdays only\n    windows: [\"Mon-Fri 08:00-19:00\"]\nother: 1\n";
        let (text, c) = carried(input);
        assert_eq!(text, "# top\nmods:\n  attend:\n    # why active\n    listening: active # inline\n    keepalive:\n      # weekdays only\n      windows: [\"Mon-Fri 08:00-19:00\"]\nother: 1\n");
        assert_eq!(c, Carried::default());
    }

    /// Review finding 4: merged into an existing `mods`, the old block goes
    /// whole, its comments with it, and the caller is told.
    #[test]
    fn merged_into_mods_the_comments_go_with_the_block() {
        let (text, c) = carried("mods:\n  ways-judge:\n    install: false\nattend_mod:\n  # why active\n  listening: active\n# about other\nother: 1\n");
        assert!(!text.contains("# why active") && text.contains("# about other\nother: 1"), "{text}");
        assert!(c.comments_dropped);
        let v: Value = serde_yaml::from_str(&text).unwrap();
        assert_eq!(v, serde_yaml::from_str::<Value>("{mods: {ways-judge: {install: false}, attend: {listening: active}}, other: 1}").unwrap());
    }

    #[test]
    fn a_value_already_under_the_new_name_is_kept_and_the_old_one_named() {
        let (text, c) = carried("mods:\n  attend:\n    listening: dormant\nattend_mod:\n  listening: active\n  keepwarm: off\n");
        let v: Value = serde_yaml::from_str(&text).unwrap();
        assert_eq!(v, serde_yaml::from_str::<Value>("mods: {attend: {listening: dormant, keepwarm: 'off'}}").unwrap());
        assert_eq!(names(&c), ["attend_mod.listening"]);
        assert_eq!(c.dropped[0].why, Why::Held("mods.attend.listening".into()));
    }

    /// Review findings 6 and 7: each dropped value says why.
    #[test]
    fn each_dropped_value_has_its_reason() {
        let (text, c) = carried("attend_mod: true\n");
        assert_eq!(text.trim(), "");
        assert_eq!(c.dropped, [Dropped { name: "attend_mod".into(), why: Why::NotMapping("true".into()) }]);
        let (text, c) = carried("mods:\n  attend:\n    keepalive: off\nattend_mod:\n  keepalive:\n    enabled: true\n");
        let v: Value = serde_yaml::from_str(&text).unwrap();
        assert_eq!(v, serde_yaml::from_str::<Value>("mods: {attend: {keepalive: 'off'}}").unwrap(), "the new value stays");
        assert_eq!(c.dropped[0].why, Why::Shape("mods.attend.keepalive".into(), "off".into()));
    }

    #[test]
    fn a_file_without_the_section_is_left_as_it_is() {
        assert_eq!(carried("mods:\n  attend:\n    listening: active\n"), ("mods:\n  attend:\n    listening: active\n".to_string(), Carried::default()));
    }
}
