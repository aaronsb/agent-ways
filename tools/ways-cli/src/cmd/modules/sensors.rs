//! A sensor as an item: one per sensor its files declare (ADR-403), read
//! through attend's own loader ([`attend_config::sensors`]), with what only
//! attend knows (a crate's version, whether a poll failed, whether it runs
//! the sensor's kind) from `attend sensor list --json`. The lowest file that
//! defines a sensor is its origin, and each other file extends it.

use std::time::{Duration, Instant};

use agent_settings::item::{Entry, ItemKind, SensorState};
use attend_config::sensors::Loaded;
use sensor_spec::Severity;
use serde::Serialize;

/// One sensor as `attend sensor list --json` describes it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Described {
    pub name: String,
    /// `crate`, `command`, `mcp` or `ways`; empty for an invalid file.
    pub kind: String,
    /// `builtin`, `user` or `project`: the lowest layer that defines it.
    pub origin: String,
    /// That layer's file.
    pub path: Option<String>,
    /// A crate's package, a command's program, an mcp sensor's server and
    /// tool.
    pub source: String,
    /// A crate's version.
    pub version: Option<String>,
    /// A word of the sensor family's list.
    pub state: String,
    /// Why it is in that state.
    pub detail: Option<String>,
    pub description: String,
    pub interval: Option<u64>,
    pub min_interval: Option<u64>,
}

/// How long attend's answer is waited on.
const ATTEND_WAIT: Duration = Duration::from_secs(2);

/// What the `attend` on PATH said of its sensors.
#[derive(Debug, Clone, PartialEq)]
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
    let rows = v.as_array()?.iter().map(|e| {
        let text = |k: &str| e.get(k).and_then(|v| v.as_str()).map(str::to_string);
        Described {
            name: text("name").unwrap_or_default(),
            kind: text("kind").unwrap_or_default(),
            origin: text("origin").unwrap_or_default(),
            path: text("path"),
            source: text("source").unwrap_or_default(),
            version: text("version"),
            state: text("state").unwrap_or_default(),
            detail: text("detail"),
            description: text("description").unwrap_or_default(),
            interval: e.get("interval").and_then(|v| v.as_u64()),
            min_interval: e.get("min_interval").and_then(|v| v.as_u64()),
        }
    });
    Some(rows.collect())
}

/// A state as attend reports it, in the sensor family's words. An attend
/// older than those words says `failed`, `not hosted` and `not compiled`,
/// which read `failing`, `degraded` and `not-compiled`.
fn state_of(word: &str) -> Option<SensorState> {
    Some(match word {
        "active" => SensorState::Active,
        "off" => SensorState::Off,
        "not-compiled" | "not compiled" => SensorState::NotCompiled,
        "missing" => SensorState::Missing,
        "refused" => SensorState::Refused,
        "failing" | "failed" => SensorState::Failing,
        "invalid" => SensorState::Invalid,
        "degraded" | "not hosted" => SensorState::Degraded,
        "unknown" => SensorState::Unknown,
        _ => return None,
    })
}

/// The state of `l` as its files say, for an attend that did not describe
/// it, and why. A sensor its files enable is `unknown` until attend says
/// whether it runs, and `why` says why attend did not.
fn local_state(l: &Loaded, why: &str) -> (SensorState, Option<String>) {
    if l.is_refused() {
        return (SensorState::Refused, l.refused.clone());
    }
    let Some(spec) = &l.spec else {
        let finding = l.findings.iter().find(|f| f.problem.severity == Severity::Invalid).map(|f| f.problem.to_string());
        return (SensorState::Invalid, finding);
    };
    match spec.enabled {
        true => (SensorState::Unknown, Some(format!("enabled; {why}"))),
        false => (SensorState::Off, None),
    }
}

/// A sensor's own fields, after its descriptor (ADR-516 §2). The screens
/// also draw its threshold and lane, which the CLI leaves to
/// `attend sensor list`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SensorFields {
    pub description: Option<String>,
    /// Seconds between polls while quiet.
    pub interval: Option<u64>,
    /// Seconds between polls while things change.
    pub min_interval: Option<u64>,
    #[serde(skip)]
    pub threshold: Option<f64>,
    #[serde(skip)]
    pub lane: Option<&'static str>,
}

/// The sensor `l`, its files layered: its kind, the file that defines it
/// and each file that extends it, and its state, source and version as
/// `attend` reports them. With no account from attend, the files alone say
/// what they can: a crate's source is then unknown.
pub fn item(l: &Loaded, attend: &Attend) -> Entry<SensorFields> {
    let spec = l.spec.as_ref();
    let d = attend.find(&l.name);
    let mut item = l.item();
    if item.kind.is_none() {
        item.kind = d.and_then(|d| sensor_spec::Kind::parse(&d.kind)).map(attend_config::sensors::item_kind);
    }
    let why = match attend {
        Attend::Absent => "attend absent",
        Attend::Unlisted => "attend did not list its sensors",
        Attend::Listed(_) => "attend does not list it",
    };
    let (state, detail) = match d {
        Some(d) => match state_of(&d.state) {
            Some(s) => (s, d.detail.clone()),
            None => (SensorState::Unknown, Some(d.detail.clone().map_or(d.state.clone(), |why| format!("{} · {why}", d.state)))),
        },
        None => local_state(l, why),
    };
    let (source, version) = match (d, item.kind) {
        (Some(d), _) if !d.source.is_empty() => (Some(d.source.clone()), d.version.clone()),
        (None, Some(ItemKind::Crate) | None) => (None, None),
        _ => (spec.map(|s| s.source()), None),
    };
    item.source = source;
    item.version = version;
    item.detail = detail;
    let description = spec.map(|s| s.description.clone()).filter(|s| !s.is_empty()).or_else(|| d.map(|d| d.description.clone()).filter(|s| !s.is_empty()));
    let fields = SensorFields {
        description,
        interval: spec.map(|s| s.cadence.interval),
        min_interval: spec.map(|s| s.cadence.min_interval),
        threshold: spec.map(|s| s.cadence.threshold),
        lane: spec.map(|s| s.lane().as_str()),
    };
    Entry { item: item.with_state(state), fields }
}
