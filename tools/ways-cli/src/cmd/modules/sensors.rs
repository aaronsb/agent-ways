//! A sensor as a [`Module`]. A built-in is a crate of attend's, described
//! by attend itself (`attend sensor list --json`); a sensor of the user's
//! is one whose `script` a settings layer sets, read from those layers
//! through attend's own loader ([`user_sensors`]).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use agent_settings::load::{resolve, Layer};
use agent_settings::LayerScope;
use attend_config::SensorConfig;
use attend_config::schema::BUILTINS;

use super::{Health, Kind, Module, Origin, Row, State};

/// One sensor as `attend sensor list --json` describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Described {
    pub name: String,
    /// `active`, `off`, `not compiled` or `missing`.
    pub state: String,
    pub description: String,
    /// A built-in's `crate@version`, a script sensor's path.
    pub source: String,
}

/// How long attend's answer is waited on.
const ATTEND_WAIT: Duration = Duration::from_secs(2);

/// What the `attend` on PATH said of its sensors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attend {
    /// Its own account.
    Listed(Vec<Described>),
    /// No `attend` on PATH.
    Absent,
    /// An attend that did not list its sensors: one older than `--json`,
    /// one that failed or did not answer within [`ATTEND_WAIT`], or one
    /// that printed no JSON.
    Unlisted,
}

impl Attend {
    fn find(&self, name: &str) -> Option<&Described> {
        match self {
            Attend::Listed(d) => d.iter().find(|d| d.name == name),
            _ => None,
        }
    }
}

/// attend's account of its sensors, from the `attend` on PATH.
pub fn described() -> Attend {
    described_by(std::ffi::OsStr::new("attend"), &["sensor", "list", "--json"])
}

/// What `program args` answers as attend's account of its sensors.
pub fn described_by(program: &std::ffi::OsStr, args: &[&str]) -> Attend {
    let spawned = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Attend::Absent,
        Err(_) => return Attend::Unlisted,
    };
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < ATTEND_WAIT => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Attend::Unlisted;
            }
        }
    };
    let mut out = String::new();
    let read = child.stdout.take().is_some_and(|mut o| std::io::Read::read_to_string(&mut o, &mut out).is_ok());
    match (status.success() && read).then(|| parse(&out)).flatten() {
        Some(d) => Attend::Listed(d),
        None => Attend::Unlisted,
    }
}

/// The JSON `attend sensor list --json` prints, as [`Described`] rows.
pub fn parse(json: &str) -> Option<Vec<Described>> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let text = |e: &serde_json::Value, k: &str| e.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_string();
    Some(v.as_array()?.iter().map(|e| Described { name: text(e, "name"), state: text(e, "state"), description: text(e, "description"), source: text(e, "source") }).collect())
}

/// A sensor the user added: one whose `script` a layer sets.
#[derive(Debug, Clone, PartialEq)]
pub struct UserSensor {
    pub name: String,
    /// The script, with `~`, `$HOME` and `$XDG_*` expanded and a relative
    /// path taken under the project, as attend runs it.
    pub script: PathBuf,
    /// The file that sets the script, and whether it is a project's.
    pub file: Option<PathBuf>,
    pub project: bool,
}

/// The sensors the user added, by name, as `layers` set them.
pub fn user_sensors(layers: &[Layer], project: &Path) -> Vec<UserSensor> {
    let cfg = attend_config::Config::from_layers(layers);
    let spec = attend_config::schema::SCHEMA.keys.iter().find(|k| k.name == "attend.sensors.*.script");
    cfg.sensors
        .iter()
        .filter(|(name, _)| !BUILTINS.contains(&name.as_str()))
        .filter_map(|(name, sc)| {
            let script = Path::new(sc.script.as_deref()?);
            let layer = spec.and_then(|s| resolve(s, std::slice::from_ref(name), layers).layer).and_then(|i| layers.get(i));
            Some(UserSensor {
                name: name.clone(),
                script: if script.is_absolute() { script.to_path_buf() } else { project.join(script) },
                file: layer.and_then(|l| l.path.clone()),
                project: layer.is_some_and(|l| l.scope == LayerScope::Project),
            })
        })
        .collect()
}

/// Whether the sensor `name`'s script is set in a project's file, which its
/// keys are then written to.
pub fn defined_in_project(layers: &[Layer], name: &str) -> bool {
    let Some(spec) = attend_config::schema::SCHEMA.keys.iter().find(|k| k.name == "attend.sensors.*.script") else { return false };
    resolve(spec, &[name.to_string()], layers).layer.and_then(|i| layers.get(i)).is_some_and(|l| l.scope == LayerScope::Project)
}

/// The rows every sensor has: its polling and its threshold.
fn timing(sc: &SensorConfig) -> Vec<Row> {
    vec![Row::new("interval", format!("{}s quiet, {}s busy", sc.interval.as_secs(), sc.min_interval.as_secs())), Row::new("threshold", sc.threshold.to_string())]
}

fn enabled(sc: &SensorConfig) -> State {
    if sc.enabled { State::new("enabled", Health::Running) } else { State::new("disabled", Health::Idle) }
}

/// The built-in sensor `name`, configured as `sc`: its crate and version as
/// `attend` reports them. A script set under the built-in's name runs as a
/// sensor of its own beside it, which its state says.
pub fn of_builtin_sensor(name: &str, sc: &SensorConfig, attend: &Attend) -> Module {
    let builtin = "built-in sensor".to_string();
    let (summary, source, version, mut state) = match (attend, attend.find(name)) {
        (Attend::Absent, _) => (builtin, "unknown: attend absent".to_string(), None, enabled(sc)),
        (Attend::Unlisted, _) => (builtin, "unknown: attend did not list its sensors".to_string(), None, enabled(sc)),
        (Attend::Listed(_), None) => (builtin, "unknown: attend does not list it".to_string(), None, enabled(sc)),
        (_, Some(d)) if d.state == "not compiled" => (d.description.clone(), "not compiled into this attend".to_string(), None, State::new("not compiled", Health::Attention)),
        (_, Some(d)) => {
            let (krate, version) = match d.source.split_once('@') {
                Some((c, v)) => (c.to_string(), Some(v.to_string())),
                None => (d.source.clone(), None),
            };
            (d.description.clone(), krate, version, enabled(sc))
        }
    };
    let mut rows = timing(sc);
    if let Some(script) = &sc.script {
        if sc.enabled {
            state = State::new(format!("{} · a script named {name} also runs", state.word), Health::Attention);
        }
        rows.push(Row::new("script", script.clone()));
    }
    Module { name: name.into(), summary, kind: Kind::Crate, origin: Origin::Builtin, path: None, source, version, state, rows }
}

/// The user's sensor `u`, configured as `sc`.
pub fn of_script_sensor(u: &UserSensor, sc: &SensorConfig) -> Module {
    let on = enabled(sc);
    let state = match (u.script.is_file(), sc.enabled) {
        (true, _) => on,
        (false, true) => State::new(format!("{} · script not found", on.word), Health::Broken),
        (false, false) => State::new(format!("{} · script not found", on.word), Health::Attention),
    };
    Module {
        name: u.name.clone(),
        summary: "a sensor of your own".into(),
        kind: Kind::Script,
        origin: if u.project { Origin::Project } else { Origin::User },
        path: u.file.clone(),
        source: u.script.display().to_string(),
        version: None,
        state,
        rows: timing(sc),
    }
}
