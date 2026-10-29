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

/// attend's account of its sensors, from the `attend` on PATH; `None` when
/// there is none, or it does not answer within [`ATTEND_WAIT`] or in JSON.
pub fn described() -> Option<Vec<Described>> {
    let mut child = std::process::Command::new("attend")
        .args(["sensor", "list", "--json"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < ATTEND_WAIT => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut out = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
    parse(&out)
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

/// The rows every sensor has: its polling and its threshold.
fn timing(sc: &SensorConfig) -> Vec<Row> {
    vec![Row::new("interval", format!("{}s quiet, {}s busy", sc.interval.as_secs(), sc.min_interval.as_secs())), Row::new("threshold", sc.threshold.to_string())]
}

fn enabled(sc: &SensorConfig) -> State {
    if sc.enabled { State::new("enabled", Health::Running) } else { State::new("disabled", Health::Idle) }
}

/// The built-in sensor `name`, configured as `sc`: its crate and version as
/// `attend` (its account, `None` when absent) reports them.
pub fn of_builtin_sensor(name: &str, sc: &SensorConfig, attend: Option<&[Described]>) -> Module {
    let d = attend.and_then(|a| a.iter().find(|d| d.name == name));
    let (summary, source, version, state) = match (attend, d) {
        (None, _) => ("built-in sensor".to_string(), "unknown: attend absent".to_string(), None, enabled(sc)),
        (Some(_), None) => ("built-in sensor".to_string(), "unknown: attend does not list it".to_string(), None, enabled(sc)),
        (Some(_), Some(d)) if d.state == "not compiled" => (d.description.clone(), "not compiled into this attend".to_string(), None, State::new("not compiled", Health::Attention)),
        (Some(_), Some(d)) => {
            let (krate, version) = match d.source.split_once('@') {
                Some((c, v)) => (c.to_string(), Some(v.to_string())),
                None => (d.source.clone(), None),
            };
            (d.description.clone(), krate, version, enabled(sc))
        }
    };
    Module { name: name.into(), summary, kind: Kind::Crate, origin: Origin::Builtin, path: None, source, version, state, rows: timing(sc) }
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
