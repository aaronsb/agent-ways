// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! Settings sections a rename moved, in one table ([`MOVES`]):
//!
//! - `attend_mod`, now `mods.attend`. Nothing configures anything from the
//!   old section.
//! - `mods.attend.keepalive`, now `mods.attend.continuity` with `enabled`
//!   renamed `keepalive`. The old keys are still read during the dual-read
//!   period, so a file holding them loads.
//!
//! `lint` and `ways manage update` name every file that still holds an old
//! section, with the command that moves it; `fix` moves it when the
//! operator runs that command. Nothing else rewrites the files.
//!
//! References:
//! - ADR-512 item 2: `attend_mod` moves under `mods.attend`.
//! - ADR-417 item 1: `mods.attend.keepalive` moves to `mods.attend.continuity`.

use std::path::{Path, PathBuf};

use agent_settings::yaml_edit::{value_at, Doc, EditError};
use serde_yaml::Value;

/// One section a rename moved: where it was, where it is now, and what a
/// file that still holds it means.
#[derive(Debug, PartialEq, Eq)]
pub struct Move {
    /// The section's key path before the rename.
    pub from: &'static [&'static str],
    /// Its key path now.
    pub to: &'static [&'static str],
    /// Keys directly under the section that the rename also renamed, as
    /// `(old, new)`.
    pub renames: &'static [(&'static str, &'static str)],
    /// The argument `ways settings fix` takes to move it.
    pub fix: &'static str,
    /// Whether the old section is still read. A file holding one that is
    /// read loads as written, so `lint` reports it as a warning.
    pub read: bool,
    /// The message `lint` prints, naming `ways settings fix <fix>`, which
    /// each file's own command replaces.
    pub message: &'static str,
}

impl Move {
    fn old_path(&self) -> Vec<String> {
        self.from.iter().map(|s| s.to_string()).collect()
    }

    fn new_path(&self) -> Vec<String> {
        self.to.iter().map(|s| s.to_string()).collect()
    }

    /// The old section's dotted name.
    pub fn old_name(&self) -> String {
        self.from.join(".")
    }

    /// The new section's dotted name.
    pub fn new_name(&self) -> String {
        self.to.join(".")
    }

    /// The key path `from` and `to` share, and the part of `to` past it.
    /// The old section is always one key past the shared part.
    fn split(&self) -> (usize, &'static [&'static str]) {
        let common = self.from.iter().zip(self.to.iter()).take_while(|(a, b)| a == b).count();
        (common, &self.to[common..])
    }

    /// The section the values move into one by one, when the file already
    /// has it: the first key of the new path the old one does not share.
    pub fn merge_target(&self) -> String {
        let (common, rest) = self.split();
        self.to[..common].iter().chain(rest.first()).copied().collect::<Vec<_>>().join(".")
    }

    /// `key` under the new section: the rename of a key directly under it,
    /// else the key itself.
    fn renamed(&self, key: &str) -> String {
        self.renames.iter().find(|(old, _)| *old == key).map_or(key, |(_, new)| new).to_string()
    }

    /// The value `old` takes under the new section.
    fn moved_value(&self, old: &Value) -> Value {
        match old {
            Value::Mapping(m) => Value::Mapping(
                m.iter()
                    .map(|(k, v)| (k.as_str().map(|s| Value::String(self.renamed(s))).unwrap_or_else(|| k.clone()), v.clone()))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
}

/// The retired top-level key.
pub const KEY: &str = "attend_mod";

/// `attend_mod`, now `mods.attend`.
///
/// References:
/// - ADR-512 item 2: `attend_mod` moves under `mods.attend`.
pub const ATTEND_MOD: Move = Move {
    from: &[KEY],
    to: &["mods", "attend"],
    renames: &[],
    fix: "mods.attend",
    read: false,
    message: ways_core::settings::ATTEND_MOD_RETIRED,
};

/// `mods.attend.keepalive`, now `mods.attend.continuity`.
///
/// References:
/// - ADR-417 item 1: the keepalive section moves to `mods.attend.continuity`.
pub const KEEPALIVE: Move = Move {
    from: &["mods", "attend", "keepalive"],
    to: &["mods", "attend", "continuity"],
    renames: &[("enabled", "keepalive")],
    fix: "mods.attend.continuity",
    read: true,
    message: "`mods.attend.keepalive` was renamed `mods.attend.continuity`, with `enabled` now `keepalive`; it is still read until `ways settings fix mods.attend.continuity` moves it",
};

/// Every move, in the order `fix` applies them: `attend_mod` first, since
/// what it moves can hold the keepalive section.
pub const MOVES: &[&Move] = &[&ATTEND_MOD, &KEEPALIVE];

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

/// One file that holds an old section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    pub path: PathBuf,
    pub line: Option<usize>,
    pub kind: Kind,
    pub moved: &'static Move,
}

impl Held {
    /// The command that moves this file's section.
    pub fn fix(&self) -> String {
        let quote = |p: &Path| crate::cmd::lifecycle::shell_word(&p.display().to_string());
        let fix = self.moved.fix;
        match &self.kind {
            Kind::User => format!("ways settings fix {fix}"),
            Kind::Target(dir) => format!("ways settings fix {fix} --target {}", quote(dir)),
            Kind::Project(dir) => format!("ways settings fix {fix} --project {}", quote(dir)),
            Kind::Other => format!("moving {} to {} by hand", self.moved.old_name(), self.moved.new_name()),
        }
    }

    /// Whether the finding fails `lint`: the old section is not read, so
    /// the file does not configure what it says.
    pub fn fails(&self) -> bool {
        !self.moved.read
    }

    /// The line an update prints for it: where the section is, whether it
    /// is still read, and the command that moves it.
    pub fn moved_line(&self) -> String {
        let at = self.line.map(|l| format!(":{l}")).unwrap_or_default();
        let read = if self.moved.read { "it is still read" } else { "nothing reads it" };
        format!("{}{at} holds {}, now {}; {read}: `{}` moves it", self.path.display(), self.moved.old_name(), self.moved.new_name(), self.fix())
    }

    /// The finding as `lint` prints it: the file, the line, the key, and the
    /// message with this file's fix command, marked a warning when the old
    /// section is still read.
    pub fn line(&self) -> String {
        let at = self.line.map(|l| format!(":{l}")).unwrap_or_default();
        let message = self.moved.message.replace(&format!("`ways settings fix {}`", self.moved.fix), &format!("`{}`", self.fix()));
        let warning = if self.fails() { "" } else { "warning: " };
        format!("{}{at}: {}: {warning}{message}", self.path.display(), self.moved.old_name())
    }
}

/// The line the old section of `m` starts on in `path`, when the file
/// holds it. `None` when it does not, or the file is absent or does not
/// parse: a file that does not parse has its own finding.
fn holds(path: &Path, m: &Move) -> Option<Option<usize>> {
    let text = std::fs::read_to_string(path).ok()?;
    let doc = Doc::parse(text.strip_prefix('\u{feff}').unwrap_or(&text)).ok()?;
    let key = m.old_path();
    doc.get(&key)?;
    Some(doc.line_of(&key))
}

/// Each old section the one file `path` holds: a project's
/// `.claude/ways.yaml`, a target's config, the user config, or any other
/// file, each with the fix command for its kind.
pub fn held_by(path: &Path) -> Vec<Held> {
    let found: Vec<(&'static Move, Option<usize>)> = MOVES.iter().filter_map(|m| holds(path, m).map(|line| (*m, line))).collect();
    if found.is_empty() {
        return Vec::new();
    }
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
    found.into_iter().map(|(moved, line)| Held { path: path.to_path_buf(), line, kind: kind.clone(), moved }).collect()
}

/// Every file that holds an old section: the user config, each target's
/// config, the project `project` names, and the `.claude/ways.yaml` of
/// every project Claude Code knows (`ways projects`), each file once and
/// each section in it once.
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
        for m in MOVES {
            if let Some(line) = holds(&path, m) {
                out.push(Held { path: path.clone(), line, kind: kind.clone(), moved: m });
            }
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

/// One value `fix` left behind when it moved a section, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    /// Its dotted name under the old section.
    pub name: String,
    pub why: Why,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// The new section already holds a value at this name, which stays.
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

/// What moving a section did besides moving it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Carried {
    pub dropped: Vec<Dropped>,
    /// Comments inside the old section were dropped with it, because the
    /// values moved one by one into an existing section.
    pub comments_dropped: bool,
}

/// The number of spaces and tabs `line` opens with.
fn indent(line: &str) -> usize {
    line.len() - line.trim_start_matches([' ', '\t']).len()
}

/// What follows `key:` on `line`, when the line writes `key` as a block key.
fn after_key<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.trim_start_matches([' ', '\t']).strip_prefix(key)?.trim_start().strip_prefix(':')
}

/// The lines of `text` the key on line `start` (0-based) spans: its own
/// line, then each line indented deeper, ending before the first line at
/// its indent or shallower and leaving the blank lines that close the
/// block in place. `None` when that line does not write `key:`.
fn block(text: &str, start: usize, key: &str) -> Option<std::ops::Range<usize>> {
    let lines: Vec<&str> = text.lines().collect();
    let head = lines.get(start)?;
    after_key(head, key)?;
    let at = indent(head);
    let mut end = start + 1;
    for (i, l) in lines.iter().enumerate().skip(start + 1) {
        if l.trim().is_empty() {
            continue;
        }
        if indent(l) <= at {
            break;
        }
        end = i + 1;
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

/// The block at `r` in `text` rewritten under the new name of `m`: the
/// header becomes the keys of the new path past the shared part, the lines
/// under it move right by two columns for each key the new path adds, and
/// each key directly under the section that `m` renames takes its new name.
fn renamed_block(text: &str, r: std::ops::Range<usize>, m: &Move) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let head = lines[r.start];
    let at = &head[..indent(head)];
    let rest = after_key(head, m.from[m.from.len() - 1]).unwrap_or_default();
    let (_, new) = m.split();
    let mut out: Vec<String> = new.iter().enumerate().map(|(i, seg)| format!("{at}{}{seg}:", "  ".repeat(i))).collect();
    if let Some(last) = out.last_mut() {
        last.push_str(rest);
    }
    let shift = "  ".repeat(new.len().saturating_sub(1));
    let body = &lines[r.start + 1..r.end];
    // The indent of the keys directly under the section: its first line
    // that is neither blank nor a comment.
    let child = body.iter().find(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#')).map(|l| indent(l));
    for l in body {
        if l.trim().is_empty() {
            out.push(l.to_string());
            continue;
        }
        let mut line = l.to_string();
        if Some(indent(l)) == child {
            if let Some((old, new)) = m.renames.iter().find(|(old, _)| after_key(l, old).is_some()) {
                line = format!("{}{new}{}", &l[..indent(l)], &l[indent(l) + old.len()..]);
            }
        }
        out.push(format!("{shift}{line}"));
    }
    out
}

/// Move the old section of `m` to its new name in `d`.
///
/// Where the file has nothing at the new section's first new key, the
/// block is renamed in place: its header takes the new name, the lines
/// under it move right as the new path deepens, and the keys `m` renames
/// take their new names, so its comments, inline ones included, stay with
/// their values. Otherwise each value is written under the new name unless
/// the file already holds one there, or a part of the new path holds a
/// value that is not a mapping, and the old block goes with the comments
/// inside it. Returns what was dropped and why.
pub fn carry(d: &mut Doc, m: &Move) -> Result<Carried, EditError> {
    let key = m.old_path();
    let Some(old) = d.get(&key).cloned() else { return Ok(Carried::default()) };
    let mut out = Carried::default();
    if !matches!(old, Value::Mapping(_) | Value::Null) {
        out.dropped.push(Dropped { name: m.old_name(), why: Why::NotMapping(shown(&old)) });
        d.unset(&key)?;
        return Ok(out);
    }
    let to = m.new_path();
    let want = m.moved_value(&old);
    let text = d.text();
    let range = d.line_of(&key).and_then(|l| block(&text, l - 1, m.from[m.from.len() - 1]));
    // Renamed in place, when there is nothing to merge into and the renamed
    // text reads back as the same values.
    let merge_into: Vec<String> = m.merge_target().split('.').map(str::to_string).collect();
    if let (None, Some(r)) = (d.get(&merge_into), range.clone()) {
        if let Ok(renamed) = Doc::parse(&splice(&text, r.clone(), &renamed_block(&text, r, m))) {
            if renamed.get(&key).is_none() && value_at(renamed.value(), &to) == Some(&want) {
                *d = renamed;
                return Ok(out);
            }
        }
    }
    // Value by value into the existing section. The old block goes first,
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
        let name = format!("{}.{}", m.old_name(), rel.join("."));
        let mut new_rel = rel.clone();
        new_rel[0] = m.renamed(&rel[0]);
        let dest: Vec<String> = to.iter().cloned().chain(new_rel).collect();
        // A part of the new path that already holds a value that is not a
        // mapping keeps it. An empty key, such as the parent the old block
        // left behind, takes the new keys.
        let blocked = (1..dest.len()).find_map(|n| match value_at(d.value(), &dest[..n]) {
            Some(Value::Mapping(_) | Value::Null) | None => None,
            Some(other) => Some((dest[..n].join("."), shown(other))),
        });
        if let Some((at, shown)) = blocked {
            out.dropped.push(Dropped { name, why: Why::Shape(at, shown) });
        } else if value_at(d.value(), &dest).is_some() {
            out.dropped.push(Dropped { name, why: Why::Held(dest.join(".")) });
        } else {
            d.set(&dest, &v)?;
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

    fn carried_by(m: &Move, text: &str) -> (String, Carried) {
        let mut d = Doc::parse(text).unwrap();
        let c = carry(&mut d, m).unwrap();
        d.verify().unwrap();
        (d.text(), c)
    }

    fn carried(text: &str) -> (String, Carried) {
        carried_by(&ATTEND_MOD, text)
    }

    fn names(c: &Carried) -> Vec<&str> {
        c.dropped.iter().map(|d| d.name.as_str()).collect()
    }

    #[test]
    fn the_section_moves_under_its_new_name() {
        assert_eq!(carried("attend_mod:\n  listening: active\n"), ("mods:\n  attend:\n    listening: active\n".to_string(), Carried::default()));
        let (text, c) = carried("# mine\nattend_mod:\n  continuity:\n    windows: [\"Sat 10:00-14:00\"]\n    keepalive: false\n");
        assert!(c.dropped.is_empty());
        assert!(text.starts_with("# mine\n"), "{text}");
        let v: Value = serde_yaml::from_str(&text).unwrap();
        assert_eq!(v, serde_yaml::from_str::<Value>("mods: {attend: {continuity: {windows: [\"Sat 10:00-14:00\"], keepalive: false}}}").unwrap());
    }

    /// Review finding 4: renamed in place, every comment stays with its value.
    #[test]
    fn a_section_renamed_in_place_keeps_its_comments() {
        let input = "# top\nattend_mod:\n  # why active\n  listening: active # inline\n  continuity:\n    # weekdays only\n    windows: [\"Mon-Fri 08:00-19:00\"]\nother: 1\n";
        let (text, c) = carried(input);
        assert_eq!(text, "# top\nmods:\n  attend:\n    # why active\n    listening: active # inline\n    continuity:\n      # weekdays only\n      windows: [\"Mon-Fri 08:00-19:00\"]\nother: 1\n");
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
        let (text, c) = carried("mods:\n  attend:\n    continuity: off\nattend_mod:\n  continuity:\n    keepalive: true\n");
        let v: Value = serde_yaml::from_str(&text).unwrap();
        assert_eq!(v, serde_yaml::from_str::<Value>("mods: {attend: {continuity: 'off'}}").unwrap(), "the new value stays");
        assert_eq!(c.dropped[0].why, Why::Shape("mods.attend.continuity".into(), "off".into()));
    }

    #[test]
    fn a_file_without_the_section_is_left_as_it_is() {
        assert_eq!(carried("mods:\n  attend:\n    listening: active\n"), ("mods:\n  attend:\n    listening: active\n".to_string(), Carried::default()));
    }

    /// The keepalive section is renamed in place at its own depth, `enabled`
    /// becomes `keepalive`, and a key of the same name deeper down keeps its
    /// name.
    ///
    /// References:
    /// - ADR-417 item 1: the keepalive section moves to `mods.attend.continuity`.
    #[test]
    fn the_keepalive_section_is_renamed_in_place_with_enabled_renamed() {
        let input = "mods:\n  attend:\n    keepalive:\n      # on here\n      enabled: true # inline\n      nested:\n        enabled: x\n    keepwarm: on\n";
        let (text, c) = carried_by(&KEEPALIVE, input);
        assert_eq!(text, "mods:\n  attend:\n    continuity:\n      # on here\n      keepalive: true # inline\n      nested:\n        enabled: x\n    keepwarm: on\n");
        assert_eq!(c, Carried::default());
    }

    #[test]
    fn into_an_existing_continuity_the_new_value_stays() {
        let (text, c) = carried_by(&KEEPALIVE, "mods:\n  attend:\n    continuity:\n      keepalive: false\n    keepalive:\n      enabled: true\n      wrap: send\n");
        let v: Value = serde_yaml::from_str(&text).unwrap();
        assert_eq!(v, serde_yaml::from_str::<Value>("mods: {attend: {continuity: {keepalive: false, wrap: send}}}").unwrap());
        assert_eq!(names(&c), ["mods.attend.keepalive.enabled"]);
        assert_eq!(c.dropped[0].why, Why::Held("mods.attend.continuity.keepalive".into()));
    }

    /// A flow-style section cannot be renamed line by line, so it moves value
    /// by value.
    #[test]
    fn a_flow_style_keepalive_section_moves_value_by_value() {
        let (text, c) = carried_by(&KEEPALIVE, "mods:\n  attend:\n    keepalive: {enabled: false, max_keepalives: 2}\n");
        let v: Value = serde_yaml::from_str(&text).unwrap();
        assert_eq!(v, serde_yaml::from_str::<Value>("mods: {attend: {continuity: {keepalive: false, max_keepalives: 2}}}").unwrap(), "{text}");
        assert!(c.dropped.is_empty());
    }

    #[test]
    fn each_move_names_where_its_values_merge() {
        assert_eq!(ATTEND_MOD.merge_target(), "mods");
        assert_eq!(KEEPALIVE.merge_target(), "mods.attend.continuity");
    }
}
