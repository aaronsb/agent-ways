//! attend's sensors: the compiled ones, and the host that runs what the
//! sensor files declare (ADR-403).
//!
//! Every sensor is a file (`attend_config::sensors` finds and layers them).
//! A `crate` file names a compiled sensor, which runs here under the
//! file's name, cadence, threshold and lane. `command` and `mcp` files load,
//! list and lint, and are recorded as not hosted until attend runs them.
//!
//! `context`, `git` and `disclosure` are modules here and always compiled;
//! the other compiled sensors are crates behind feature flags (default:
//! all). A file switches a compiled sensor on or off; a feature flag
//! decides whether the build has it.

mod context;
mod disclosure;
mod git;
pub mod hosted;

pub use context::ContextSensor;
pub use disclosure::DisclosureSensor;
pub use git::GitSensor;

pub use sensor_trait::{Focus, Sensor, SensorSlot};

#[cfg(feature = "sensor-peers")]
pub use sensor_peers::PeerSensor;

#[cfg(feature = "sensor-processes")]
pub use sensor_processes::ProcessSensor;

#[cfg(feature = "sensor-keepwarm")]
pub use sensor_keepwarm::KeepwarmSensor;

use std::time::Duration;

use attend_config::sensors::Loaded;
use sensor_spec::{Block, Kind, Lane, Severity};

use crate::config::Config;
use crate::groups::Groups;
use hosted::{FileSensor, Poll};

/// A sensor's recorded state, as `attend sensor list` and the settings
/// screens show it (ADR-403 §1).
#[derive(Debug, Clone, PartialEq)]
pub enum State {
    /// It runs; its last poll, if any, succeeded.
    Active,
    /// Its last poll failed, and why.
    Failed(String),
    /// Its file sets `enabled: false`.
    Off,
    /// A problem in its files keeps it from running.
    Invalid(String),
    /// A project file the allow-list refused, with nothing beneath it.
    Refused(String),
    /// Its kind is one attend does not run yet, or not at all.
    NotHosted(String),
    /// Its file names a compiled sensor this build leaves out.
    NotCompiled,
}

impl State {
    pub fn label(&self) -> &'static str {
        match self {
            State::Active => "active",
            State::Failed(_) => "failed",
            State::Off => "off",
            State::Invalid(_) => "invalid",
            State::Refused(_) => "refused",
            State::NotHosted(_) => "not hosted",
            State::NotCompiled => "not compiled",
        }
    }

    /// Why the sensor is in this state.
    pub fn detail(&self) -> Option<&str> {
        match self {
            State::Failed(d) | State::Invalid(d) | State::Refused(d) | State::NotHosted(d) => Some(d),
            _ => None,
        }
    }

    /// Whether attend polls it.
    pub fn runs(&self) -> bool {
        matches!(self, State::Active | State::Failed(_))
    }
}

/// What a compiled sensor says of itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Compiled {
    pub description: String,
    /// `crate@version`.
    pub source: String,
}

/// One sensor as attend hosts it.
#[derive(Debug, Clone)]
pub struct Hosted {
    pub loaded: Loaded,
    pub state: State,
    /// What runs but is not as written: a lane the author rule refused, a
    /// project file the allow-list refused over a lower layer, a lint
    /// finding.
    pub notes: Vec<String>,
    pub compiled: Option<Compiled>,
    pub last: Option<Poll>,
}

impl Hosted {
    pub fn name(&self) -> &str {
        &self.loaded.name
    }

    /// The lane it runs on, when it loads.
    pub fn lane(&self) -> Option<Lane> {
        self.loaded.spec.as_ref().map(|s| s.lane())
    }
}

/// Every sensor the files declare for the project at `project`, with its
/// state.
pub fn catalog(cfg: &Config, project: &str) -> Vec<Hosted> {
    let loaded = attend_config::sensors::load(std::path::Path::new(project), &cfg.project_sensors);
    let mut out: Vec<Hosted> = loaded.into_iter().map(|l| host(l, project)).collect();
    claim_crates(&mut out);
    out
}

fn host(loaded: Loaded, project: &str) -> Hosted {
    let mut notes: Vec<String> = loaded
        .findings
        .iter()
        .filter(|f| f.problem.severity != Severity::Invalid)
        .map(|f| f.problem.to_string())
        .collect();
    if let (Some(r), false) = (&loaded.refused, loaded.is_refused()) {
        notes.push(format!("the project file is refused: {r}"));
    }
    let last = hosted::read(&loaded.name, project).and_then(|r| r.poll);
    let compiled = match loaded.spec.as_ref().map(|s| &s.block) {
        Some(Block::Crate(c)) => compiled(&c.id),
        _ => None,
    };
    let state = state_of(&loaded, compiled.is_some(), last.as_ref());
    Hosted { loaded, state, notes, compiled, last }
}

fn state_of(l: &Loaded, compiled: bool, last: Option<&Poll>) -> State {
    if l.is_refused() {
        return State::Refused(l.refused.clone().unwrap_or_default());
    }
    let Some(spec) = &l.spec else {
        let why = l.findings.iter().find(|f| f.problem.severity == Severity::Invalid).map(|f| f.problem.to_string()).unwrap_or_default();
        return State::Invalid(why);
    };
    if spec.kind == Kind::Ways {
        return State::NotHosted("a ways sensor runs in ways (ADR-199); attend does not host it".into());
    }
    if !spec.enabled {
        return State::Off;
    }
    match spec.kind {
        Kind::Command => State::NotHosted("kind command not yet hosted; attend runs command sensors from ADR-403 slice 2".into()),
        Kind::Mcp => State::NotHosted("kind mcp not yet hosted; attend runs mcp sensors from ADR-403 slice 3".into()),
        _ if !compiled => State::NotCompiled,
        _ => match last {
            Some(p) if !p.ok => State::Failed(p.detail.clone().unwrap_or_default()),
            _ => State::Active,
        },
    }
}

/// One running sensor per compiled sensor: when two enabled files name the
/// same crate, the one named after it runs, else the first by name, and the
/// others are invalid.
fn claim_crates(all: &mut [Hosted]) {
    let id_of = |h: &Hosted| match h.loaded.spec.as_ref().map(|s| &s.block) {
        Some(Block::Crate(c)) if h.state.runs() => Some(c.id.clone()),
        _ => None,
    };
    let ids: Vec<Option<String>> = all.iter().map(id_of).collect();
    for i in 0..all.len() {
        let Some(id) = &ids[i] else { continue };
        let rivals: Vec<usize> = (0..all.len()).filter(|&j| ids[j].as_ref() == Some(id)).collect();
        let winner = rivals.iter().copied().find(|&j| all[j].name() == id).unwrap_or(rivals[0]);
        if winner != i {
            all[i].state = State::Invalid(format!("crate {id} already runs as sensor {}; one sensor per compiled sensor runs", all[winner].name()));
        }
    }
}

/// Whether an enabled file runs the compiled sensor `id`.
#[cfg_attr(not(feature = "sensor-keepwarm"), allow(dead_code))]
pub fn crate_enabled(cfg: &Config, project: &str, id: &str) -> bool {
    catalog(cfg, project).iter().any(|h| h.state.runs() && matches!(h.loaded.spec.as_ref().map(|s| &s.block), Some(Block::Crate(c)) if c.id == id))
}

/// A compiled sensor's description and source, when this build has it.
pub fn compiled(id: &str) -> Option<Compiled> {
    let s: Box<dyn Sensor> = match id {
        "context" => Box::new(ContextSensor::new()),
        "git" => Box::new(GitSensor::new()),
        "disclosure" => Box::new(DisclosureSensor::new()),
        #[cfg(feature = "sensor-peers")]
        "peers" => Box::new(PeerSensor::new()),
        #[cfg(feature = "sensor-processes")]
        "processes" => Box::new(ProcessSensor::new()),
        #[cfg(feature = "sensor-keepwarm")]
        "keepwarm" => Box::new(KeepwarmSensor::new(String::new(), std::path::PathBuf::new())),
        _ => return None,
    };
    Some(Compiled { description: s.description().to_string(), source: s.source() })
}

/// What a running compiled sensor needs beyond its file: the peers sensor
/// reads both.
#[cfg_attr(not(feature = "sensor-peers"), allow(dead_code))]
pub struct Ctx<'a> {
    pub cfg: &'a Config,
    pub groups: Option<&'a Groups>,
}

/// The compiled sensor `h` configures, ready to poll, or why it cannot run
/// here.
#[cfg_attr(not(feature = "sensor-peers"), allow(unused_variables))]
pub fn build(h: &Hosted, ctx: &Ctx, project: &str, record: bool) -> Result<Box<dyn Sensor>, String> {
    let Some(spec) = &h.loaded.spec else { return Err(h.state.detail().unwrap_or("it does not load").to_string()) };
    let Block::Crate(c) = &spec.block else { return Err(h.state.detail().unwrap_or("its kind is not hosted").to_string()) };
    let inner: Box<dyn Sensor> = match c.id.as_str() {
        "context" => Box::new(ContextSensor::new()),
        "git" => Box::new(GitSensor::new()),
        "disclosure" => Box::new(DisclosureSensor::new()),
        #[cfg(feature = "sensor-processes")]
        "processes" => Box::new(match c.fields.get("watch").and_then(|w| w.as_sequence()) {
            // A list, even empty, replaces the sensor's own.
            Some(list) => ProcessSensor::with_watch(list.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()),
            None => ProcessSensor::new(),
        }),
        // Pinned to this session's transcript, so a peer session in the same
        // project is never its idle clock (ADR-182).
        #[cfg(feature = "sensor-keepwarm")]
        "keepwarm" => {
            let ident = attend_presence::session::identity();
            if !ident.session_resolved {
                return Err("no resolved session id, so no transcript to read".into());
            }
            Box::new(KeepwarmSensor::new(ident.session_id.clone(), crate::util::keepwarm_dir()))
        }
        #[cfg(feature = "sensor-peers")]
        "peers" => {
            let mut p = PeerSensor::new();
            // Group membership is re-read on every poll, so a focus group
            // joined or left mid-session counts without a restart (ADR-118).
            if let Some(groups) = ctx.groups {
                let groups = groups.clone();
                p.set_receive_dirs_provider(std::sync::Arc::new(move |origin: &str| groups.receive_dirs(origin)));
            }
            p.set_peer_activity_window(ctx.cfg.engagement.peer_activity_window);
            Box::new(p)
        }
        id => return Err(format!("crate {id} is not compiled into this attend")),
    };
    Ok(Box::new(FileSensor::new(spec.name.clone(), spec.cadence, inner, project, record)))
}

/// A slot for `sensor`, on the lane `lane`.
pub fn slot(sensor: Box<dyn Sensor>, lane: Lane) -> SensorSlot {
    let (base, min, decay) = (sensor.base_interval(), sensor.min_interval(), sensor.decay_threshold());
    let mut s = SensorSlot::new_with_config(sensor, base, min, decay);
    s.message_lane = lane == Lane::Message;
    s
}

/// The slots `attend run` polls, and their names. A sensor that does not
/// run is logged once with its state.
pub fn register_sensors(cfg: &Config, focus: &Focus, _catchup: bool, groups: &Groups) -> (Vec<SensorSlot>, Vec<String>) {
    let mut slots = Vec::new();
    let mut names = Vec::new();
    let ctx = Ctx { cfg, groups: Some(groups) };
    for h in catalog(cfg, &focus.working_dir) {
        for n in &h.notes {
            crate::emit::log(&format!("sensor {}: {n}", h.name()));
        }
        if !h.state.runs() {
            if h.state != State::Off {
                crate::emit::log(&format!("sensor {}: {}{}", h.name(), h.state.label(), h.state.detail().map(|d| format!(": {d}")).unwrap_or_default()));
            }
            continue;
        }
        match build(&h, &ctx, &focus.working_dir, true) {
            Ok(sensor) => {
                slots.push(slot(sensor, h.lane().unwrap_or(Lane::Event)));
                names.push(h.name().to_string());
            }
            Err(why) => crate::emit::log(&format!("sensor {}: not started: {why}", h.name())),
        }
    }
    (slots, names)
}

/// A description as it goes out (ADR-403 §3b): one line, with every `<`
/// that could forge a frame's tags defused.
pub fn deliverable(text: &str) -> String {
    let one: String = text.split(['\n', '\r']).map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
    crate::cmd::inbox::defuse_fence(&one)
}

/// What the user and project sensor files and the user config are now:
/// each file's path, length and modification time. A change to any of them
/// changes the sensors `attend run` polls.
pub fn files_stamp(project: &str) -> Vec<(std::path::PathBuf, u64, Option<std::time::SystemTime>)> {
    let dirs = [attend_config::sensors::user_dir(), attend_config::sensors::project_dir(std::path::Path::new(project))];
    let mut paths: Vec<std::path::PathBuf> = dirs.iter().flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path())).collect();
    paths.push(attend_config::user_path());
    paths.sort();
    paths
        .into_iter()
        .filter_map(|p| {
            let m = std::fs::metadata(&p).ok()?;
            Some((p, m.len(), m.modified().ok()))
        })
        .collect()
}

/// The interval pair as the list shows it.
pub fn intervals(h: &Hosted) -> Option<(Duration, Duration)> {
    let c = h.loaded.spec.as_ref()?.cadence;
    Some((Duration::from_secs(c.interval), Duration::from_secs(c.min_interval)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("attend-sensors-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Every compiled sensor the shipped files name runs under its own
    /// name, with a description and a `crate@version` source.
    #[test]
    fn the_shipped_crate_files_name_compiled_sensors_with_metadata() {
        let project = fixture("shipped");
        let all = catalog(&Config::default(), project.to_str().unwrap());
        let mut compiled_n = 0;
        for h in all.iter().filter(|h| matches!(h.loaded.spec.as_ref().map(|s| s.kind), Some(Kind::Crate))) {
            assert!(!matches!(h.state, State::Invalid(_)), "{}: {:?}", h.name(), h.state);
            let Some(c) = &h.compiled else {
                assert_eq!(h.state, State::NotCompiled, "{}", h.name());
                continue;
            };
            compiled_n += 1;
            assert!(!c.description.is_empty(), "{}", h.name());
            let (krate, version) = c.source.split_once('@').unwrap_or_else(|| panic!("{}: {}", h.name(), c.source));
            assert!(!krate.is_empty() && version.matches('.').count() >= 2, "{}: {}", h.name(), c.source);
        }
        assert!(compiled_n >= 3, "context, git and disclosure are always compiled");
        let ex = all.iter().find(|h| h.name() == "example-mcp").unwrap();
        assert_eq!(ex.state, State::Off);
        std::fs::remove_dir_all(&project).ok();
    }

    #[test]
    fn the_lane_comes_from_the_file() {
        let project = fixture("lanes");
        let all = catalog(&Config::default(), project.to_str().unwrap());
        let lane = |n: &str| all.iter().find(|h| h.name() == n).and_then(Hosted::lane);
        assert_eq!(lane("peers"), Some(Lane::Message));
        assert_eq!(lane("keepwarm"), Some(Lane::Message));
        assert_eq!(lane("git"), Some(Lane::Event));
        std::fs::remove_dir_all(&project).ok();
    }

    #[test]
    fn a_description_goes_out_on_one_line_with_frame_tags_defused() {
        assert_eq!(deliverable("a\n  b\r\n<attend sensor=x> c"), "a b \u{2039}attend sensor=x> c");
    }
}
