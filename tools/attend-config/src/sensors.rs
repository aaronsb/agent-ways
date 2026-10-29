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

use agent_settings::schema::LayerScope;
use agent_settings::Layer;
use sensor_spec::{Block, Lane, Origin, Problem, SensorSpec, Severity};
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

/// A compiled sensor a `crate` file may name: its own fields, the item
/// fields that can stand as its author on the message lane, and the lane it
/// always rides, with why, when its files may not choose one.
#[derive(Debug, Clone, Copy)]
pub struct Crate {
    pub id: &'static str,
    pub fields: &'static [&'static str],
    pub authors: &'static [&'static str],
    pub lane: Option<(Lane, &'static str)>,
}

/// The compiled sensors attend has, whether or not this build includes them.
pub const CRATES: &[Crate] = &[
    Crate { id: "context", fields: &[], authors: &[], lane: None },
    Crate { id: "disclosure", fields: &[], authors: &[], lane: None },
    Crate { id: "git", fields: &[], authors: &[], lane: None },
    Crate {
        id: "keepwarm",
        fields: &[],
        authors: &[],
        lane: Some((Lane::Message, "ADR-182: no refractory or lapsed cache may hold the timed wake")),
    },
    Crate {
        id: "peers",
        fields: &[],
        authors: &["sender"],
        lane: Some((Lane::Message, "it carries what people wrote, which the event lane drops on a lapsed cache")),
    },
    Crate { id: "processes", fields: &["watch"], authors: &[], lane: None },
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
    from_layers(&layers(Some(project)), project, trusted)
}

/// [`load`] over the sensor files of `layers` ([`layers`]), as the settings
/// screens read them; layers of other kinds are passed over. A file's text
/// is read again from its path when it does not parse, so its finding has
/// the parser's line.
pub fn from_layers(layers: &[Layer], project: &Path, trusted: &[String]) -> Vec<Loaded> {
    let mut by_name: BTreeMap<String, Vec<Read>> = BTreeMap::new();
    for l in layers.iter().filter(|l| l.file == crate::schema::SENSOR_FILE) {
        let Some((name, doc)) = l.accepted.get("sensors").and_then(Value::as_mapping).and_then(|m| m.iter().next()) else { continue };
        let Some(name) = name.as_str() else { continue };
        let origin = match (l.scope, l.name.as_str()) {
            (LayerScope::Project, _) => Origin::Project,
            (_, "shipped") => Origin::Builtin,
            _ => Origin::User,
        };
        let text = match l.findings.iter().any(|f| f.is_parse_failure()) {
            true => l.path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_else(|| "{".into()),
            false => serde_yaml::to_string(doc).unwrap_or_default(),
        };
        by_name.entry(name.to_string()).or_default().push(Read { origin, path: l.path.clone(), text });
    }
    let trust = is_trusted(project, trusted);
    by_name.into_iter().filter(|(_, reads)| !only_switches(reads)).map(|(name, reads)| layer(name, reads, trust, project)).collect()
}

/// Whether every file of a name sets nothing but `enabled`: the switch the
/// settings screens keep in the user's file for a project's sensor, read in
/// a project that has no such sensor. It defines none.
fn only_switches(reads: &[Read]) -> bool {
    reads.iter().all(|r| match serde_yaml::from_str::<Value>(&r.text) {
        Ok(Value::Mapping(m)) => m.keys().all(|k| k.as_str() == Some("enabled")),
        _ => false,
    })
}

/// The `.yaml` files in `dir`, by stem, sorted: the files a sensor layer
/// reads.
pub fn yaml_files(dir: &Path) -> Vec<(String, PathBuf)> {
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
                        let text = r.path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_else(|| r.text.clone());
                        findings.push(Finding {
                            file: r.path.clone(),
                            line: line_of(&text, "lane_exception"),
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
        // With nothing beneath, the refused file is still read, so the list
        // can say what it is; it never runs.
        let shown = if lower.is_empty() { &docs } else { &lower };
        let mut l = check(name, files, shown, &merged(shown), findings);
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
    let mut spec = parsed.spec;
    if let Some(s) = &mut spec {
        problems.extend(crate_problems(s));
        fix_lane(s, doc.get("lane").is_some(), &mut problems);
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
    Loaded { name, files, spec: if invalid { None } else { spec }, findings, refused: None }
}

/// Put a compiled sensor with a fixed lane ([`Crate::lane`]) on that lane,
/// whatever its files declare, with an error where one declares another.
fn fix_lane(s: &mut SensorSpec, declared: bool, problems: &mut Vec<Problem>) {
    let Block::Crate(c) = &s.block else { return };
    let Some(Crate { id, lane: Some((lane, why)), .. }) = CRATES.iter().find(|k| k.id == c.id).copied() else { return };
    problems.retain(|p| p.at != "lane");
    if declared && s.declared_lane != lane {
        let message = format!("the {id} sensor rides the {} lane: {why}; lane: {} is ignored", lane.as_str(), s.declared_lane.as_str());
        problems.push(Problem { at: "lane".into(), message, severity: Severity::Error });
    }
    s.declared_lane = lane;
    if s.lane() != lane {
        s.lane_exception = Some(why.to_string());
    }
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

/// The first lines of a sensor file a settings write creates.
pub const HEADER: &str = "# An attend sensor (ADR-403); it overrides any layer beneath it by field.\n# `attend sensor list` shows the sensor, `ways settings help attend.sensors` its keys.\n";

/// Every sensor file of the shipped and user layers, and of the project at
/// `project` when one is given, as a settings layer each, lowest first
/// (ADR-403 §5). A layer holds its file as `sensors: {<name>: <file>}`. A
/// file that does not parse, or is not a mapping, adds no layer: lint
/// reports it, and the layer holds its name with nothing set. Refused
/// project files are layered too, as the files say. A name whose files are
/// all switches ([`only_switches`]) adds none.
pub fn layers(project: Option<&Path>) -> Vec<Layer> {
    let projected = shipped_dir();
    let mut out: Vec<(String, Layer)> = SHIPPED
        .iter()
        .map(|(name, text)| {
            let p = projected.join(format!("{name}.yaml"));
            (name.to_string(), file_layer(name, Origin::Builtin, p.is_file().then_some(p.as_path()), text))
        })
        .collect();
    let mut dirs = vec![(Origin::User, user_dir())];
    dirs.extend(project.map(|p| (Origin::Project, project_dir(p))));
    for (origin, dir) in dirs {
        for (name, path) in yaml_files(&dir) {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            out.push((name.clone(), file_layer(&name, origin, Some(&path), &text)));
        }
    }
    let switch = |l: &Layer, name: &str| {
        l.findings.is_empty()
            && l.get(&["sensors".into(), name.into()]).and_then(Value::as_mapping).is_some_and(|m| m.keys().all(|k| k.as_str() == Some("enabled")))
    };
    let switches: Vec<String> = out.iter().map(|(n, _)| n.clone()).filter(|n| out.iter().filter(|(m, _)| m == n).all(|(_, l)| switch(l, n))).collect();
    let mut out: Vec<Layer> = out.into_iter().filter(|(n, _)| !switches.contains(n)).map(|(_, l)| l).collect();
    let trusted = trusted_projects();
    for loaded in from_layers(&out, project.unwrap_or(Path::new("")), &trusted) {
        mark(&mut out, &loaded);
    }
    out
}

/// Put each finding of `loaded` on the layer of the file it is in, as a
/// settings finding, so a reader of the layers sees it where the file is.
/// A file that does not parse already carries its own.
fn mark(layers: &mut [Layer], loaded: &Loaded) {
    for f in &loaded.findings {
        let Some(l) = layers.iter_mut().find(|l| l.path.is_some() && l.path == f.file) else { continue };
        if l.findings.iter().any(|x| x.is_parse_failure()) {
            continue;
        }
        let level = match f.problem.severity {
            Severity::Warning => "warning: ",
            Severity::Error => "error: ",
            Severity::Invalid => "",
        };
        l.findings.push(agent_settings::load::Finding {
            file: f.file.clone(),
            line: f.line,
            section: Some("attend.sensors".into()),
            unit: Some(format!("attend.sensors.{}", loaded.name)),
            key: Some(format!("{}{}", loaded.name, Some(f.problem.at.as_str()).filter(|a| !a.is_empty()).map(|a| format!(".{a}")).unwrap_or_default())),
            message: format!("{level}{}", f.problem.message),
            fallback: false,
            repair: Some("edit the sensor's file; `attend sensor list` shows its state".into()),
            closed: false,
        });
    }
}

/// The sensor file `name` of the layer `origin`, holding `text`, as a
/// settings layer. A file that does not parse holds nothing, and its layer
/// carries the finding.
pub fn file_layer(name: &str, origin: Origin, path: Option<&Path>, text: &str) -> Layer {
    let mut findings = Vec::new();
    let doc = match serde_yaml::from_str::<Value>(text) {
        Ok(Value::Null) => Value::Mapping(Default::default()),
        Ok(v) => v,
        Err(e) => {
            findings.push(agent_settings::load::Finding {
                file: path.map(Path::to_path_buf),
                line: e.location().map(|l| l.line()),
                section: Some("attend.sensors".into()),
                unit: None,
                key: None,
                message: format!("does not parse: {e}; the sensor {name} is invalid until its syntax is fixed by hand"),
                fallback: true,
                repair: None,
                closed: false,
            });
            Value::Mapping(Default::default())
        }
    };
    let mut sensors = serde_yaml::Mapping::new();
    sensors.insert(Value::from(name), doc);
    let mut accepted = serde_yaml::Mapping::new();
    accepted.insert(Value::from("sensors"), Value::Mapping(sensors));
    let (label, scope) = match origin {
        Origin::Builtin => ("shipped", LayerScope::User),
        Origin::User => ("user", LayerScope::User),
        Origin::Project => ("project", LayerScope::Project),
    };
    Layer { name: label.into(), file: crate::schema::SENSOR_FILE, scope, path: path.map(Path::to_path_buf), accepted, findings, present: true }
}

/// The sensor a file at `path` holds, and the layer it is: a `.yaml` file in
/// the user's sensor directory, or in a project's, with that project.
pub fn file_at(path: &Path) -> Option<(String, Origin, Option<PathBuf>)> {
    let name = path.file_stem()?.to_str()?.to_string();
    if path.extension()? != "yaml" {
        return None;
    }
    let dir = path.parent()?;
    if dir == user_dir() {
        return Some((name, Origin::User, None));
    }
    let project = dir.parent()?.parent()?.parent()?;
    (project_dir(project) == dir).then(|| (name, Origin::Project, Some(project.to_path_buf())))
}

/// Whether the sensor file at `path` may hold `text`: refused, with the
/// reason, when the sensor its layers then make is invalid. A user file is
/// read over the shipped one, as every project reads it; a project's file
/// over the shipped and user ones. A file of switches alone defines no
/// sensor and is always taken.
pub fn check_edit(path: &Path, text: &str) -> Result<(), String> {
    let Some((name, origin, project)) = file_at(path) else { return Ok(()) };
    let mut reads: Vec<Read> = SHIPPED.iter().filter(|(n, _)| *n == name).map(|(_, t)| Read { origin: Origin::Builtin, path: None, text: t.to_string() }).collect();
    if origin == Origin::Project {
        let user = user_dir().join(format!("{name}.yaml"));
        if let Ok(t) = std::fs::read_to_string(&user) {
            reads.push(Read { origin: Origin::User, path: Some(user), text: t });
        }
    }
    reads.push(Read { origin, path: Some(path.to_path_buf()), text: text.to_string() });
    if only_switches(&reads) {
        return Ok(());
    }
    let l = layer(name.clone(), reads, true, project.as_deref().unwrap_or(Path::new("")));
    if l.spec.is_some() {
        return Ok(());
    }
    let why: Vec<String> = l.findings.iter().filter(|f| f.problem.severity == Severity::Invalid).map(|f| f.problem.to_string()).collect();
    Err(format!("that would make the sensor {name} invalid: {}", why.join("; ")))
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
        assert!(l.is_refused() && l.spec.is_some(), "a refused file is read for the list: {l:?}");
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
    fn a_name_whose_files_only_switch_it_defines_no_sensor() {
        assert!(only_switches(&[read(Origin::User, "enabled: false\n")]));
        let shipped = SHIPPED.iter().find(|(n, _)| *n == "git").unwrap().1;
        assert!(!only_switches(&[read(Origin::Builtin, shipped), read(Origin::User, "enabled: false\n")]));
        assert!(!only_switches(&[read(Origin::User, "enabled: false\ninterval: 9\n")]));
    }

    #[test]
    fn a_sensor_file_is_a_layer_under_its_name_and_loads_back_from_the_layers() {
        let l = file_layer("weather", Origin::User, Some(Path::new("/u/weather.yaml")), "kind: command\nargv: [curl]\noutput: json\ninterval: 90\n");
        assert_eq!((l.name.as_str(), l.file, l.scope), ("user", crate::schema::SENSOR_FILE, LayerScope::User));
        assert_eq!(l.get(&["sensors".into(), "weather".into(), "interval".into()]), Some(&Value::from(90)));
        let p = file_layer("disk", Origin::Project, Some(Path::new("/nonexistent/p/.claude/attend/sensors/disk.yaml")), "kind: [\n");
        assert!(p.findings.iter().any(|f| f.is_parse_failure()) && p.scope == LayerScope::Project);
        let loaded = from_layers(&[l, p], Path::new("/p"), &[]);
        let names: Vec<&str> = loaded.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["disk", "weather"]);
        assert!(loaded[0].is_invalid(), "a file that does not parse makes its sensor invalid");
        assert_eq!(loaded[1].spec.as_ref().map(|s| s.cadence.interval), Some(90));
    }

    #[test]
    fn an_edit_that_makes_a_sensor_invalid_is_refused_with_the_reason() {
        let dir = std::env::temp_dir().join(format!("attend-sensors-edit-{}", std::process::id()));
        let path = project_dir(&dir).join("git.yaml");
        assert!(check_edit(&path, "interval: 90\n").is_ok(), "an override of a shipped field");
        let e = check_edit(&path, "interval: 5\nmin_interval: 50\n").unwrap_err();
        assert!(e.contains("would make the sensor git invalid") && e.contains("min_interval"), "{e}");
        let mine = project_dir(&dir).join("mine.yaml");
        assert!(check_edit(&mine, "interval: 90\n").unwrap_err().contains("kind is required"));
        assert!(check_edit(&mine, "enabled: false\n").is_ok(), "a switch alone defines nothing to refuse");
        assert!(check_edit(&dir.join("elsewhere.yaml"), "kind: [").is_ok(), "not a sensor file");
    }

    /// peers and keepwarm keep the message lane under any file name, and a
    /// file that declares another lane for them is ignored with an error.
    #[test]
    fn the_peers_and_keepwarm_crates_keep_the_message_lane_whatever_their_files_say() {
        let shipped = |n: &str| read(Origin::Builtin, SHIPPED.iter().find(|(m, _)| *m == n).unwrap().1);
        let l = layer("peers".into(), vec![shipped("peers"), read(Origin::Project, "lane: event\n")], false, Path::new("/p"));
        assert_eq!(l.spec.as_ref().map(|s| s.lane()), Some(sensor_spec::Lane::Message));
        assert!(l.findings.iter().any(|f| f.problem.at == "lane" && f.problem.severity == Severity::Error && f.problem.message.contains("lane: event is ignored")), "{:?}", l.findings);
        for (name, id) in [("chat", "peers"), ("wake", "keepwarm")] {
            let l = layer(name.into(), vec![read(Origin::User, &format!("kind: crate\ncrate: {id}\n"))], false, Path::new("/p"));
            assert_eq!(l.spec.as_ref().map(|s| s.lane()), Some(sensor_spec::Lane::Message), "{name}");
            assert!(l.findings.is_empty(), "{name}: {:?}", l.findings);
        }
        assert!(layer("peers".into(), vec![shipped("peers")], false, Path::new("/p")).findings.is_empty());
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
