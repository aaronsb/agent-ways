//! A sensor as an item: one per sensor its files declare (ADR-403), read
//! through attend's own loader ([`attend_config::sensors`]), with what only
//! attend knows (a crate's version, whether a poll failed) from
//! `attend sensor list --json`. The lowest file that defines a sensor is its
//! origin, and each file above it extends it.

use std::time::{Duration, Instant};

use agent_settings::item::{Family, How, Item, ItemKind, Origin, Override, SensorState};
use agent_tui::tree::Card;
use attend_config::sensors::Loaded;
use sensor_spec::Severity;

use super::row;

/// One sensor as `attend sensor list --json` describes it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Described {
    pub name: String,
    /// `crate`, `command`, `mcp` or `ways`; empty for an invalid file.
    pub kind: String,
    /// `builtin`, `user` or `project`: the highest layer with a file.
    pub origin: String,
    /// That layer's file.
    pub path: Option<String>,
    /// A crate's package, a command's program, an mcp sensor's server and
    /// tool.
    pub source: String,
    /// A crate's version.
    pub version: Option<String>,
    /// `active`, `failed`, `off`, `invalid`, `refused`, `not hosted` or
    /// `not compiled`.
    pub state: String,
    /// Why it is in that state.
    pub detail: Option<String>,
    pub enabled: Option<bool>,
    pub lane: Option<String>,
    pub description: String,
    pub interval: Option<u64>,
    pub min_interval: Option<u64>,
    pub threshold: Option<f64>,
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
            enabled: e.get("enabled").and_then(|v| v.as_bool()),
            lane: text("lane"),
            description: text("description").unwrap_or_default(),
            interval: e.get("interval").and_then(|v| v.as_u64()),
            min_interval: e.get("min_interval").and_then(|v| v.as_u64()),
            threshold: e.get("threshold").and_then(|v| v.as_f64()),
        }
    });
    Some(rows.collect())
}

/// A state as attend reports it, in the sensor family's words. attend's
/// `failed`, `not hosted` and `not compiled` read `failing`, `missing` and
/// `not-compiled`.
fn state_of(word: &str) -> Option<SensorState> {
    Some(match word {
        "active" => SensorState::Active,
        "off" => SensorState::Off,
        "not compiled" | "not-compiled" => SensorState::NotCompiled,
        "not hosted" | "missing" => SensorState::Missing,
        "refused" => SensorState::Refused,
        "failed" | "failing" => SensorState::Failing,
        "invalid" => SensorState::Invalid,
        _ => return None,
    })
}

fn kind_of(k: sensor_spec::Kind) -> ItemKind {
    match k {
        sensor_spec::Kind::Crate => ItemKind::Crate,
        sensor_spec::Kind::Command => ItemKind::Command,
        sensor_spec::Kind::Mcp => ItemKind::Mcp,
        sensor_spec::Kind::Ways => ItemKind::Ways,
    }
}

fn origin_of(o: sensor_spec::Origin) -> Origin {
    match o {
        sensor_spec::Origin::Builtin => Origin::Builtin,
        sensor_spec::Origin::User => Origin::User,
        sensor_spec::Origin::Project => Origin::Project,
    }
}

/// The state of `l` as its files say, for an attend that did not describe
/// it, and why. A sensor its files enable has no state until attend says
/// whether it runs, which `unknown` says it has not.
fn local_state(l: &Loaded, unknown: &str) -> (Option<SensorState>, Option<String>) {
    if l.is_refused() {
        return (Some(SensorState::Refused), l.refused.clone());
    }
    let Some(spec) = &l.spec else {
        let why = l.findings.iter().find(|f| f.problem.severity == Severity::Invalid).map(|f| f.problem.to_string());
        return (Some(SensorState::Invalid), why);
    };
    match spec.enabled {
        true => (None, Some(format!("enabled; {unknown}"))),
        false => (Some(SensorState::Off), None),
    }
}

/// The sensor `l`, its files layered, as a card: its kind, the file that
/// defines it and each file that extends it, and its state, source and
/// version as `attend` reports them. With no account from attend, the files
/// alone say what they can: a crate's source is then unknown.
pub fn of_sensor(l: &Loaded, attend: &Attend) -> Card {
    let spec = l.spec.as_ref();
    let d = attend.find(&l.name);
    let kind = spec.map(|s| s.kind).or_else(|| d.and_then(|d| sensor_spec::Kind::parse(&d.kind))).map(kind_of);
    // The files merged into it: every one when nothing else loads, else
    // all but a refused project file.
    let whole = l.is_refused();
    let files: Vec<_> = l.files.iter().filter(|f| whole || !f.refused).collect();
    let base = files.iter().position(|f| !f.switch).unwrap_or(0);
    let (origin, path) = files.get(base).map_or((Origin::Builtin, None), |f| (origin_of(f.origin), f.path.clone()));
    let overrides = files
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != base)
        .filter_map(|(_, f)| Some(Override { origin: origin_of(f.origin), how: How::Extends, path: f.path.clone()? }))
        .collect();
    let unknown = match attend {
        Attend::Absent => "attend absent",
        Attend::Unlisted => "attend did not list its sensors",
        Attend::Listed(_) => "attend does not list it",
    };
    let (state, detail) = match d {
        Some(d) => match state_of(&d.state) {
            Some(s) => (Some(s), d.detail.clone()),
            None => (None, Some(d.detail.clone().map_or(d.state.clone(), |why| format!("{} · {why}", d.state)))),
        },
        None => local_state(l, unknown),
    };
    let (source, version) = match (d, kind) {
        (Some(d), _) if !d.source.is_empty() => (Some(d.source.clone()), d.version.clone()),
        (None, Some(ItemKind::Crate) | None) => (None, None),
        _ => (spec.map(|s| s.source()), None),
    };
    let summary = spec.map(|s| s.description.clone()).filter(|s| !s.is_empty()).or_else(|| d.map(|d| d.description.clone()).filter(|s| !s.is_empty()));
    let mut rows = Vec::new();
    if let Some(s) = spec {
        let c = s.cadence;
        rows.push(row("interval", format!("{}s quiet, {}s busy", c.interval, c.min_interval)));
        rows.push(row("threshold", c.threshold.to_string()));
        rows.push(row("lane", s.lane().as_str()));
    }
    let mut item = Item { kind, path, overrides, source, version, detail, ..Item::new(Family::Sensor, l.name.clone(), origin) };
    if let Some(state) = state {
        item = item.with_state(state);
    }
    Card {
        title: l.name.clone(),
        summary: summary.unwrap_or_else(|| kind.map_or("a sensor file".into(), |k| format!("a {} sensor", k.word()))),
        item,
        fold_state: false,
        rows,
    }
}

