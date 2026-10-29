//! attend's sensor files (ADR-403 §2): where each layer lives, the shipped
//! files embedded in the build, and the load that layers them by name,
//! applies project trust and checks the result. attend runs what this
//! returns; `ways settings lint` reports its findings.
//!
//! The layers, lowest first:
//! - shipped: embedded here, and projected with the app to
//!   `$XDG_DATA_HOME/agent-ways/attend/sensors/`;
//! - user: `$XDG_CONFIG_HOME/attend/sensors/<name>.yaml`;
//! - project: `.claude/attend/sensors/<name>.yaml`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sensor_spec::{Block, Origin, Problem, SensorSpec, Severity};
use serde_yaml::Value;

/// The shipped sensor files, by name.
pub const SHIPPED: &[(&str, &str)] = &[
    ("context", include_str!("../../../attend/sensors/context.yaml")),
    ("disclosure", include_str!("../../../attend/sensors/disclosure.yaml")),
    ("example-command", include_str!("../../../attend/sensors/example-command.yaml")),
    ("example-mcp", include_str!("../../../attend/sensors/example-mcp.yaml")),
    ("git", include_str!("../../../attend/sensors/git.yaml")),
    ("keepwarm", include_str!("../../../attend/sensors/keepwarm.yaml")),
    ("peers", include_str!("../../../attend/sensors/peers.yaml")),
    ("processes", include_str!("../../../attend/sensors/processes.yaml")),
];

/// A compiled sensor a `crate` file may name: its own fields, and the item
/// fields that can stand as its author on the message lane.
#[derive(Debug, Clone, Copy)]
pub struct Crate {
    pub id: &'static str,
    pub fields: &'static [&'static str],
    pub authors: &'static [&'static str],
}

/// The compiled sensors attend has, whether or not this build includes them.
pub const CRATES: &[Crate] = &[
    Crate { id: "context", fields: &[], authors: &[] },
    Crate { id: "disclosure", fields: &[], authors: &[] },
    Crate { id: "git", fields: &[], authors: &[] },
    Crate { id: "keepwarm", fields: &[], authors: &[] },
    Crate { id: "peers", fields: &[], authors: &["sender"] },
    Crate { id: "processes", fields: &["watch"], authors: &[] },
];

/// `$XDG_CONFIG_HOME/attend/sensors/`, beside the user config file.
pub fn user_dir() -> PathBuf {
    crate::user_path().with_file_name("sensors")
}

/// A project's sensor files: `<dir>/.claude/attend/sensors/`.
pub fn project_dir(dir: &Path) -> PathBuf {
    dir.join(".claude").join("attend").join("sensors")
}

/// Where the app projects the shipped files:
/// `$XDG_DATA_HOME/agent-ways/attend/sensors/`.
pub fn shipped_dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| claude_sessions::home_dir().join(".local").join("share"));
    base.join("agent-ways").join("attend").join("sensors")
}

/// One file of a sensor's layers.
#[derive(Debug, Clone, PartialEq)]
pub struct SensorFile {
    pub origin: Origin,
    /// The file. A shipped file's is its projected copy, when there is one.
    pub path: Option<PathBuf>,
    /// A project file the allow-list refused: it is not merged.
    pub refused: bool,
}

/// A problem, and the file it is in when one file holds the field.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub file: Option<PathBuf>,
    pub line: Option<usize>,
    pub problem: Problem,
}

/// One sensor name and what its layers came to.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub name: String,
    /// Its files, lowest layer first.
    pub files: Vec<SensorFile>,
    /// The merged sensor; none when a problem makes it invalid.
    pub spec: Option<SensorSpec>,
    pub findings: Vec<Finding>,
    /// Why a project file was refused.
    pub refused: Option<String>,
}

impl Loaded {
    /// The highest layer that has a file for the sensor.
    pub fn origin(&self) -> Origin {
        self.files.iter().map(|f| f.origin).max().unwrap_or(Origin::Builtin)
    }

    /// The file of the highest layer, if it has a path.
    pub fn path(&self) -> Option<&Path> {
        self.files.iter().rev().find_map(|f| f.path.as_deref())
    }

    /// Whether a problem keeps the sensor from running.
    pub fn is_invalid(&self) -> bool {
        self.spec.is_none() && self.refused.is_none()
    }

    /// Whether every file is refused, so nothing of the sensor loads.
    pub fn is_refused(&self) -> bool {
        self.refused.is_some() && self.files.iter().all(|f| f.refused)
    }
}

/// A layer's file read for one name.
struct Read {
    origin: Origin,
    path: Option<PathBuf>,
    text: String,
}

/// Every sensor of the shipped, user and project layers for the project at
/// `project`, by name. `trusted` is the user's `attend.project_sensors`.
pub fn load(project: &Path, trusted: &[String]) -> Vec<Loaded> {
    let mut by_name: BTreeMap<String, Vec<Read>> = BTreeMap::new();
    let projected = shipped_dir();
    for (name, text) in SHIPPED {
        let p = projected.join(format!("{name}.yaml"));
        by_name.entry(name.to_string()).or_default().push(Read { origin: Origin::Builtin, path: p.is_file().then_some(p), text: text.to_string() });
    }
    for (origin, dir) in [(Origin::User, user_dir()), (Origin::Project, project_dir(project))] {
        for (name, path) in yaml_files(&dir) {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            by_name.entry(name).or_default().push(Read { origin, path: Some(path), text });
        }
    }
    let trust = is_trusted(project, trusted);
    by_name.into_iter().map(|(name, reads)| layer(name, reads, trust, project)).collect()
}

/// The `.yaml` files in `dir`, by stem, sorted.
fn yaml_files(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<(String, PathBuf)> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "yaml") && p.is_file())
        .filter_map(|p| Some((p.file_stem()?.to_str()?.to_string(), p)))
        .collect();
    out.sort();
    out
}

/// Whether `project` is in the allow-list. Entries expand `~`, `$HOME` and
/// `$XDG_*`, and compare as canonical paths.
pub fn is_trusted(project: &Path, trusted: &[String]) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let here = canon(project);
    trusted.iter().any(|t| canon(Path::new(&crate::expand_path(t))) == here)
}

fn layer(name: String, reads: Vec<Read>, trust: bool, project: &Path) -> Loaded {
    let mut findings = Vec::new();
    let mut docs: Vec<(Origin, Option<PathBuf>, Value)> = Vec::new();
    for r in reads {
        match serde_yaml::from_str::<Value>(&r.text) {
            Ok(Value::Null) => docs.push((r.origin, r.path, Value::Mapping(Default::default()))),
            Ok(mut v) => {
                if r.origin != Origin::Builtin {
                    if let Some(m) = v.as_mapping_mut().filter(|m| m.contains_key("lane_exception")) {
                        m.remove("lane_exception");
                        findings.push(Finding {
                            file: r.path.clone(),
                            line: line_of(&r.text, "lane_exception"),
                            problem: Problem { at: "lane_exception".into(), message: "only a shipped file sets lane_exception; it is ignored".into(), severity: Severity::Error },
                        });
                    }
                }
                docs.push((r.origin, r.path, v));
            }
            Err(e) => findings.push(Finding {
                file: r.path.clone(),
                line: e.location().map(|l| l.line()),
                problem: Problem { at: String::new(), message: format!("does not parse: {e}"), severity: Severity::Invalid },
            }),
        }
    }
    let files_of = |refused: bool, docs: &[(Origin, Option<PathBuf>, Value)]| {
        docs.iter().map(|(o, p, _)| SensorFile { origin: *o, path: p.clone(), refused: refused && *o == Origin::Project }).collect::<Vec<_>>()
    };
    if findings.iter().any(|f| f.problem.severity == Severity::Invalid) {
        return Loaded { files: files_of(false, &docs), name, spec: None, findings, refused: None };
    }
    let merged = |docs: &[(Origin, Option<PathBuf>, Value)]| {
        let mut out = Value::Mapping(Default::default());
        for (_, _, v) in docs {
            sensor_spec::merge(&mut out, v);
        }
        out
    };
    let doc = merged(&docs);
    let kind = doc.get("kind").and_then(Value::as_str).unwrap_or_default();
    let has_project = docs.iter().any(|(o, _, _)| *o == Origin::Project);
    if has_project && !trust && matches!(kind, "command" | "mcp") {
        let refused = Some(format!(
            "a project {kind} sensor runs code the repository chose; it loads once {} is in attend.project_sensors in {}",
            project.display(),
            crate::user_path().display()
        ));
        let lower: Vec<_> = docs.iter().filter(|(o, _, _)| *o != Origin::Project).cloned().collect();
        let files = files_of(true, &docs);
        if lower.is_empty() {
            return Loaded { name, files, spec: None, findings, refused };
        }
        let mut l = check(name, files, &lower, &merged(&lower), findings);
        l.refused = refused;
        return l;
    }
    check(name, files_of(false, &docs), &docs, &doc, findings)
}

/// Parse and check the merged `doc`, and place each problem in the highest
/// file that sets its field.
fn check(name: String, files: Vec<SensorFile>, docs: &[(Origin, Option<PathBuf>, Value)], doc: &Value, mut findings: Vec<Finding>) -> Loaded {
    let parsed = sensor_spec::parse(&name, doc);
    let mut problems = parsed.problems;
    if let Some(s) = &parsed.spec {
        problems.extend(crate_problems(s));
    }
    let invalid = problems.iter().any(|p| p.severity == Severity::Invalid);
    for problem in problems {
        let top = problem.at.split(['.', '[']).next().unwrap_or_default().to_string();
        let holder = docs.iter().rev().find(|(_, _, v)| !top.is_empty() && v.get(top.as_str()).is_some()).or(docs.last());
        let (file, line) = match holder {
            Some((_, Some(p), _)) => {
                let text = std::fs::read_to_string(p).unwrap_or_default();
                (Some(p.clone()), line_of(&text, &problem.at))
            }
            _ => (None, None),
        };
        findings.push(Finding { file, line, problem });
    }
    Loaded { name, files, spec: if invalid { None } else { parsed.spec }, findings, refused: None }
}

/// A crate file's checks against the compiled sensor it names.
fn crate_problems(s: &SensorSpec) -> Vec<Problem> {
    let Block::Crate(c) = &s.block else { return Vec::new() };
    let bad = |at: &str, m: String| Problem { at: at.to_string(), message: m, severity: Severity::Invalid };
    let Some(known) = CRATES.iter().find(|k| k.id == c.id) else {
        let ids: Vec<&str> = CRATES.iter().map(|k| k.id).collect();
        return vec![bad("crate", format!("names no compiled sensor; attend has {}", ids.join(", ")))];
    };
    let mut out = Vec::new();
    for (k, v) in &c.fields {
        let k = k.as_str().unwrap_or_default();
        if !known.fields.contains(&k) {
            out.push(bad(k, format!("unknown field for a crate sensor, and the {} sensor takes none of that name", c.id)));
        } else if k == "watch" && !v.as_sequence().is_some_and(|s| s.iter().all(Value::is_string)) {
            out.push(bad(k, "watch is a list of process names".into()));
        }
    }
    if let Some(a) = &s.author {
        if !known.authors.contains(&a.as_str()) {
            out.push(bad("author", format!("the {} sensor has no item field {a}", c.id)));
        }
    }
    out
}

/// The line of the field at the dotted `at` in `text`, or of its nearest
/// parent that has one.
fn line_of(text: &str, at: &str) -> Option<usize> {
    let doc = agent_settings::yaml_edit::Doc::parse(text).ok()?;
    let mut path: Vec<String> = at.split('.').filter(|s| !s.is_empty()).map(str::to_string).collect();
    while !path.is_empty() {
        if let Some(l) = doc.line_of(&path) {
            return Some(l);
        }
        path.pop();
    }
    None
}

/// The user's `attend.project_sensors`.
pub fn trusted_projects() -> Vec<String> {
    crate::Config::from_layers(&[crate::user_layer()]).project_sensors
}

/// Every finding in the sensor files for `project`, one line each:
/// `<file>:<line>: [attend.sensors.<name>] <field>: <message>`.
pub fn lint(project: &Path) -> Vec<String> {
    load(project, &trusted_projects()).iter().flat_map(lint_lines).collect()
}

/// [`lint`]'s lines for one sensor.
pub fn lint_lines(l: &Loaded) -> Vec<String> {
    let mut out: Vec<String> = l
        .findings
        .iter()
        .map(|f| {
            let at = match (&f.file, f.line) {
                (Some(p), Some(n)) => format!("{}:{n}: ", p.display()),
                (Some(p), None) => format!("{}: ", p.display()),
                (None, _) => format!("<shipped {}.yaml>: ", l.name),
            };
            format!("{at}[attend.sensors.{}] {}", l.name, f.problem)
        })
        .collect();
    if let (Some(r), Some(p)) = (&l.refused, l.files.iter().find(|f| f.refused).and_then(|f| f.path.as_ref())) {
        out.push(format!("{}: [attend.sensors.{}] refused: {r}", p.display(), l.name));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shipped_file_loads_and_lints_clean() {
        let empty = std::env::temp_dir().join(format!("attend-sensors-none-{}", std::process::id()));
        for (name, text) in SHIPPED {
            let l = layer(name.to_string(), vec![Read { origin: Origin::Builtin, path: None, text: text.to_string() }], false, &empty);
            assert!(l.findings.is_empty(), "{name}: {:?}", l.findings);
            let s = l.spec.unwrap_or_else(|| panic!("{name} loads"));
            assert_eq!(s.name, *name);
        }
    }

    #[test]
    fn the_examples_are_off_and_of_their_kind() {
        for (name, kind) in [("example-command", sensor_spec::Kind::Command), ("example-mcp", sensor_spec::Kind::Mcp)] {
            let text = SHIPPED.iter().find(|(n, _)| *n == name).unwrap().1;
            let s = sensor_spec::parse(name, &serde_yaml::from_str(text).unwrap()).spec.unwrap();
            assert_eq!((s.kind, s.enabled), (kind, false), "{name}");
            assert!(!s.triggers.is_empty() && !s.state.is_empty(), "{name} shows triggers and state");
        }
        let mcp = SHIPPED.iter().find(|(n, _)| *n == "example-mcp").unwrap().1;
        assert_eq!(sensor_spec::parse("example-mcp", &serde_yaml::from_str(mcp).unwrap()).spec.unwrap().lane(), sensor_spec::Lane::Message);
    }

    fn read(origin: Origin, text: &str) -> Read {
        Read { origin, path: None, text: text.to_string() }
    }

    #[test]
    fn a_user_file_overrides_a_field_and_a_lane_exception_is_the_shipped_file_s_alone() {
        let shipped = SHIPPED.iter().find(|(n, _)| *n == "git").unwrap().1;
        let l = layer("git".into(), vec![read(Origin::Builtin, shipped), read(Origin::User, "interval: 90\nlane: message\nlane_exception: mine\n")], false, Path::new("/p"));
        let s = l.spec.unwrap();
        assert_eq!((s.cadence.interval, s.cadence.min_interval), (90, 10));
        assert_eq!(s.lane(), sensor_spec::Lane::Event);
        assert!(l.findings.iter().any(|f| f.problem.at == "lane_exception"), "{:?}", l.findings);
        assert!(l.findings.iter().any(|f| f.problem.at == "lane" && f.problem.severity == Severity::Error), "{:?}", l.findings);
    }

    #[test]
    fn a_project_command_file_is_refused_until_the_project_is_trusted() {
        let cmd = "kind: command\nargv: [/bin/true]\noutput: lines\n";
        let l = layer("chk".into(), vec![read(Origin::Project, cmd)], false, Path::new("/p"));
        assert!(l.is_refused() && l.spec.is_none(), "{l:?}");
        let l = layer("chk".into(), vec![read(Origin::Project, cmd)], true, Path::new("/p"));
        assert!(l.refused.is_none() && l.spec.is_some());
        // A crate override from a project needs no trust.
        let shipped = SHIPPED.iter().find(|(n, _)| *n == "git").unwrap().1;
        let l = layer("git".into(), vec![read(Origin::Builtin, shipped), read(Origin::Project, "enabled: false\n")], false, Path::new("/p"));
        assert!(l.refused.is_none() && !l.spec.unwrap().enabled);
        // A project file that turns a user command sensor's argv is refused, and the user's file runs.
        let l = layer("chk".into(), vec![read(Origin::User, cmd), read(Origin::Project, "argv: [rm, -rf, x]\n")], false, Path::new("/p"));
        let Block::Command(c) = l.spec.as_ref().unwrap().block.clone() else { panic!() };
        assert_eq!(c.argv, vec!["/bin/true".to_string()]);
        assert!(l.refused.is_some() && !l.is_refused());
    }

    #[test]
    fn a_crate_file_is_checked_against_its_compiled_sensor() {
        let l = layer("p2".into(), vec![read(Origin::User, "kind: crate\ncrate: processes\nwatch: cargo\n")], false, Path::new("/p"));
        assert!(l.findings.iter().any(|f| f.problem.at == "watch"), "{:?}", l.findings);
        let l = layer("x".into(), vec![read(Origin::User, "kind: crate\ncrate: nope\n")], false, Path::new("/p"));
        assert!(l.is_invalid());
        let l = layer("g".into(), vec![read(Origin::User, "kind: crate\ncrate: git\ncolour: red\n")], false, Path::new("/p"));
        assert!(l.findings.iter().any(|f| f.problem.at == "colour"), "{:?}", l.findings);
        let l = layer("bad".into(), vec![read(Origin::User, "kind: [\n")], false, Path::new("/p"));
        assert!(l.is_invalid() && l.findings[0].problem.message.starts_with("does not parse"));
    }
}
