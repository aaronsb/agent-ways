//! A sensor as a [`Module`]. A built-in is a crate of attend's, described
//! by attend itself (`attend sensor list --json`).

use std::time::{Duration, Instant};

use agent_settings::load::Layer;

use super::{Health, Kind, Module, Origin, State};

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
    let mut cmd = std::process::Command::new("attend");
    cmd.args(["sensor", "list", "--json"]);
    described_by(cmd)
}

/// What `cmd` answers as attend's account of its sensors.
pub fn described_by(mut cmd: std::process::Command) -> Attend {
    let spawned = cmd
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

/// Whether the key `key` of the sensor `name` is written to the project's
/// file. Sensor keys are written to sensor files (ADR-403), which no layer
/// here reads, so none is.
pub fn writes_project(_layers: &[Layer], _name: &str, _key: &str) -> bool {
    false
}

/// The built-in sensor `name`: its crate and version as `attend` reports
/// them.
pub fn of_builtin_sensor(name: &str, attend: &Attend) -> Module {
    let builtin = "built-in sensor".to_string();
    let listed = State::new("listed", Health::Running);
    let (summary, source, version, state) = match (attend, attend.find(name)) {
        (Attend::Absent, _) => (builtin, "unknown: attend absent".to_string(), None, listed),
        (Attend::Unlisted, _) => (builtin, "unknown: attend did not list its sensors".to_string(), None, listed),
        (Attend::Listed(_), None) => (builtin, "unknown: attend does not list it".to_string(), None, listed),
        (_, Some(d)) => {
            let (krate, version) = match d.source.split_once('@') {
                Some((c, v)) => (c.to_string(), Some(v.to_string())),
                None => (d.source.clone(), None),
            };
            (d.description.clone(), krate, version, State::new(d.state.clone(), Health::Running))
        }
    };
    Module { name: name.into(), summary, kind: Kind::Crate, origin: Origin::Builtin, path: None, source, version, state, rows: Vec::new() }
}
