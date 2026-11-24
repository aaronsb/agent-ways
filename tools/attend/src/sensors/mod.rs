// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! attend's sensors: the compiled ones, and the host that runs what the
//! sensor files declare (ADR-403).
//!
//! Every sensor is a file (`attend_config::sensors` finds and layers them).
//! A `crate` file names a compiled sensor, which runs here under the
//! file's name, cadence, threshold and lane. A `command` file runs its argv
//! each poll, and its triggers and declared state evaluate the output. An
//! `mcp` file calls one read-only tool on a stdio MCP server each poll, and
//! its triggers and declared state evaluate the result; it runs once
//! `attend sensor probe` has recorded that the tool can be read twice
//! without a change, and the operator confirmed it.
//!
//! `context`, `git` and `disclosure` are modules here and always compiled;
//! the other compiled sensors are crates behind feature flags (default:
//! all). A file switches a compiled sensor on or off; a feature flag
//! decides whether the build has it.

mod command;
mod context;
pub mod eval;
mod git;
pub mod hosted;
mod mcp;
pub mod state;
pub mod writer;

pub use context::ContextSensor;
pub use git::GitSensor;

pub use sensor_trait::{Focus, Sensor, SensorSlot};

#[cfg(feature = "sensor-peers")]
pub use sensor_peers::PeerSensor;

#[cfg(feature = "sensor-processes")]
pub use sensor_processes::ProcessSensor;

use std::time::Duration;

use attend_config::sensors::Loaded;
use sensor_spec::{Block, Kind, Lane, Severity};

use crate::config::Config;
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
    /// An mcp sensor with no probe record that holds for it, and why.
    Unprobed(String),
    /// Its file names a compiled sensor this build leaves out.
    NotCompiled,
}

impl State {
    /// The sensor family's word for it (ADR-516 §2): a kind attend does
    /// not run yet reads `degraded`.
    pub fn word(&self) -> agent_settings::item::SensorState {
        use agent_settings::item::SensorState as W;
        match self {
            State::Active => W::Active,
            State::Failed(_) => W::Failing,
            State::Off => W::Off,
            State::Invalid(_) => W::Invalid,
            State::Refused(_) => W::Refused,
            State::NotHosted(_) | State::Unprobed(_) => W::Degraded,
            State::NotCompiled => W::NotCompiled,
        }
    }

    pub fn label(&self) -> &'static str {
        agent_settings::item::State::from(self.word()).word()
    }

    /// Why the sensor is in this state.
    pub fn detail(&self) -> Option<&str> {
        match self {
            State::Failed(d) | State::Invalid(d) | State::Refused(d) | State::NotHosted(d) | State::Unprobed(d) => Some(d),
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
    /// The project the sensor was loaded for.
    pub project: String,
}

impl Hosted {
    pub fn name(&self) -> &str {
        &self.loaded.name
    }

    /// The lane it runs on, when it loads.
    pub fn lane(&self) -> Option<Lane> {
        self.loaded.spec.as_ref().map(|s| s.lane())
    }

    /// Why an mcp sensor may not call its tool, whether or not it is
    /// enabled: it has no probe record that holds for it here. None for
    /// any other sensor.
    pub fn probe_gap(&self) -> Option<String> {
        match self.loaded.spec.as_ref().map(|s| &s.block) {
            Some(Block::Mcp(m)) => unprobed(&self.loaded, m, &self.project),
            _ => None,
        }
    }

    /// Whether a file of the project's own layer defines part of it.
    pub fn in_project(&self) -> bool {
        in_project(&self.loaded)
    }

    /// The probe record that applies to an mcp sensor here: for a sensor a
    /// project file defines, the one kept for this project; else its
    /// `probe`.
    pub fn probe_record(&self) -> Option<&sensor_spec::ProbeRecord> {
        match self.loaded.spec.as_ref().map(|s| &s.block) {
            Some(Block::Mcp(m)) => record_for(&self.loaded, m, &self.project),
            _ => None,
        }
    }

    /// The compiled sensor its file names, when it is a crate sensor.
    pub fn crate_id(&self) -> Option<&str> {
        match self.loaded.spec.as_ref().map(|s| &s.block) {
            Some(Block::Crate(c)) => Some(&c.id),
            _ => None,
        }
    }
}

/// Every sensor the files declare for the project at `project`, with its
/// state. The last poll is this session's own, or with no session id the
/// newest of any session in the project.
pub fn catalog(cfg: &Config, project: &str) -> Vec<Hosted> {
    let ident = attend_presence::session::identity();
    catalog_for(cfg, project, ident.session_resolved.then_some(ident.session_id.as_str()))
}

/// [`catalog`], with the last polls of `session`'s stores.
pub fn catalog_for(cfg: &Config, project: &str, session: Option<&str>) -> Vec<Hosted> {
    let loaded = attend_config::sensors::load(std::path::Path::new(project), &cfg.project_sensors);
    let polls = hosted::last_polls(project, session);
    catalog_of(loaded, project, |name| polls.get(name).cloned())
}

/// The sensors `loaded` declares for the project at `project`, each with
/// its last poll from `last`.
pub(crate) fn catalog_of(loaded: Vec<Loaded>, project: &str, last: impl Fn(&str) -> Option<Poll>) -> Vec<Hosted> {
    let mut out: Vec<Hosted> = loaded
        .into_iter()
        .map(|l| {
            let poll = last(&l.name);
            host(l, project, poll)
        })
        .collect();
    claim_crates(&mut out);
    out
}

fn host(loaded: Loaded, project: &str, last: Option<Poll>) -> Hosted {
    let mut notes: Vec<String> = loaded
        .findings
        .iter()
        .filter(|f| f.problem.severity != Severity::Invalid)
        .map(|f| f.problem.to_string())
        .collect();
    if let (Some(r), false) = (&loaded.refused, loaded.is_refused()) {
        notes.push(format!("the project file is refused: {r}"));
    }
    if let Some(Block::Mcp(m)) = loaded.spec.as_ref().map(|s| &s.block) {
        if m.allow_unannotated {
            notes.push(format!("overridden: allow_unannotated: true calls {} even though it declares no readOnlyHint", m.tool));
        }
    }
    let compiled = match loaded.spec.as_ref().map(|s| &s.block) {
        Some(Block::Crate(c)) => compiled(&c.id),
        _ => None,
    };
    let state = state_of(&loaded, project, compiled.is_some(), last.as_ref());
    Hosted { loaded, state, notes, compiled, last, project: project.to_string() }
}

fn state_of(l: &Loaded, project: &str, compiled: bool, last: Option<&Poll>) -> State {
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
    if let Block::Mcp(m) = &spec.block {
        if let Some(why) = unprobed(l, m, project) {
            return State::Unprobed(why);
        }
    }
    match spec.kind {
        Kind::Crate if !compiled => State::NotCompiled,
        _ => match last {
            Some(p) if !p.ok => State::Failed(p.detail.clone().unwrap_or_default()),
            _ => State::Active,
        },
    }
}

/// Why the mcp sensor `l` does not run for want of a probe in the project
/// at `project`, or none when its record holds: taken with its server,
/// tool and args, bound to its server's definition, for this project when
/// a project file defines part of the sensor, with the calls matching and
/// the operator's confirmation. A named server's binding is checked again
/// at each call, since only resolving it reads its definition.
///
/// References:
/// - ADR-403 §4: a sensor with no record, a failed comparison or no
///   confirmation stays disabled, and a change voids the record.
fn unprobed(l: &Loaded, m: &sensor_spec::McpBlock, project: &str) -> Option<String> {
    let again = format!("run `attend sensor probe {}` at a terminal", l.name);
    let Some(p) = record_for(l, m, project) else {
        return Some(match in_project(l) {
            true => format!("no probe record for this project; {again}"),
            false => format!("no probe record; {again}"),
        });
    };
    let stale = !m.holds(&sensor_spec::ProbeRecord { calls_match: true, confirmed: true, ..p.clone() }) || mcp::inline_binding(&m.server).is_some_and(|b| p.binding.as_deref() != Some(b.as_str()));
    Some(match () {
        _ if stale => format!("the probe record was taken with another server, tool or args; {again}"),
        _ if !p.calls_match => format!("the probe's second call did not return what the first did, so reading may change the tool's result; {again} once that is fixed"),
        _ if !p.confirmed => format!("the probe record has no confirmation that the items were left untouched; check in another client, then {again}"),
        _ => return None,
    })
}

/// Whether a file of the project's own layer defines part of `l`.
fn in_project(l: &Loaded) -> bool {
    l.files.iter().any(|f| f.origin == sensor_spec::Origin::Project && !f.refused)
}

/// The path a project is known by in a probe record: canonical, so a
/// project reached through a symlink keeps its record.
pub fn project_key(project: &str) -> String {
    std::fs::canonicalize(project).map(|p| p.display().to_string()).unwrap_or_else(|_| project.to_string())
}

/// The record of `m` that applies in the project at `project`: for a
/// sensor a project file defines, the one its `probes` keep for the
/// project, compared as canonical paths; else its `probe`.
fn record_for<'a>(l: &Loaded, m: &'a sensor_spec::McpBlock, project: &str) -> Option<&'a sensor_spec::ProbeRecord> {
    match in_project(l) {
        true => {
            let here = project_key(project);
            m.probes.iter().find(|(k, _)| project_key(k) == here).map(|(_, r)| r)
        }
        false => m.probe.as_ref(),
    }
}

/// One running sensor per compiled sensor: when two enabled files name the
/// same crate, the one named after it runs, else the first by name, and the
/// others are invalid.
fn claim_crates(all: &mut [Hosted]) {
    let ids: Vec<Option<String>> = all.iter().map(|h| h.crate_id().filter(|_| h.state.runs()).map(str::to_string)).collect();
    for i in 0..all.len() {
        let Some(id) = &ids[i] else { continue };
        let rivals: Vec<usize> = (0..all.len()).filter(|&j| ids[j].as_ref() == Some(id)).collect();
        let winner = rivals.iter().copied().find(|&j| all[j].name() == id).unwrap_or(rivals[0]);
        if winner != i {
            all[i].state = State::Invalid(format!("crate {id} already runs as sensor {}; one sensor per compiled sensor runs", all[winner].name()));
        }
    }
}

/// A compiled sensor's description and source, when this build has it.
pub fn compiled(id: &str) -> Option<Compiled> {
    let s: Box<dyn Sensor> = match id {
        "context" => Box::new(ContextSensor::new()),
        "git" => Box::new(GitSensor::new()),
        #[cfg(feature = "sensor-peers")]
        "peers" => Box::new(PeerSensor::new()),
        #[cfg(feature = "sensor-processes")]
        "processes" => Box::new(ProcessSensor::new()),
        _ => return None,
    };
    Some(Compiled { description: s.description().to_string(), source: s.source() })
}

/// What a running sensor needs beyond its file: an mcp sensor reads the
/// config, and a recorded sensor keys its store by the session.
pub struct Ctx<'a> {
    pub cfg: &'a Config,
    /// The session its reports go to; none when the caller has no id.
    pub session: Option<&'a str>,
    /// The session's Claude Code pid. The processes sensor reports only the
    /// builds that run under it; none reports every build the user runs.
    /// Only the sensor host sets it: a one-shot `attend sensor poll` keeps
    /// no process state between runs, so a root would change nothing.
    #[cfg_attr(not(feature = "sensor-processes"), allow(dead_code))]
    pub root: Option<u32>,
}

/// The sensor `h` configures, ready to poll, or why it cannot run here.
pub fn build(h: &Hosted, ctx: &Ctx, project: &str, record: bool) -> Result<Box<dyn Sensor>, String> {
    file_sensor(h, ctx, project, record).map(|s| Box::new(s) as Box<dyn Sensor>)
}

/// The sensor `h` configures as a [`FileSensor`], so a caller can read its
/// trigger verdicts. With a session id in `ctx` it reads its store, and
/// with `record` as well it writes it.
#[cfg_attr(not(feature = "sensor-peers"), allow(unused_variables))]
pub fn file_sensor(h: &Hosted, ctx: &Ctx, project: &str, record: bool) -> Result<FileSensor, String> {
    let Some(spec) = &h.loaded.spec else { return Err(h.state.detail().unwrap_or("it does not load").to_string()) };
    let c = match &spec.block {
        Block::Crate(c) => c,
        Block::Command(cmd) => {
            let run = command::Command::new(cmd.clone(), project, command::Inherit::of(h.loaded.command_from));
            return Ok(FileSensor::declared(spec, Box::new(run), project, ctx.session, record));
        }
        Block::Mcp(_) => return Ok(FileSensor::declared(spec, Box::new(mcp_source(h, ctx.cfg, project)?), project, ctx.session, record)),
        _ => return Err(h.state.detail().unwrap_or("its kind is not hosted").to_string()),
    };
    let inner: Box<dyn Sensor> = match c.id.as_str() {
        "context" => Box::new(ContextSensor::new()),
        "git" => Box::new(GitSensor::new()),
        #[cfg(feature = "sensor-processes")]
        "processes" => Box::new(match c.fields.get("watch").and_then(|w| w.as_sequence()) {
            // A list, even empty, replaces the sensor's own.
            Some(list) => ProcessSensor::with_watch(list.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()),
            None => ProcessSensor::new(),
        }
        .with_root(ctx.root)),
        #[cfg(feature = "sensor-peers")]
        "peers" => Box::new(PeerSensor::new()),
        id => return Err(format!("crate {id} is not compiled into this attend")),
    };
    Ok(FileSensor::new(spec.name.clone(), spec.cadence, inner, project, ctx.session, record))
}

/// The tool call the mcp sensor `h` polls through, for the project at
/// `project`. Nothing starts until its first call.
pub fn mcp_source(h: &Hosted, cfg: &Config, project: &str) -> Result<mcp::Mcp, String> {
    let Some(Block::Mcp(m)) = h.loaded.spec.as_ref().map(|s| &s.block) else { return Err(format!("{} is not an mcp sensor", h.name())) };
    let lookup = mcp::Lookup::new(attend_config::sensors::is_trusted(std::path::Path::new(project), &cfg.project_sensors));
    Ok(mcp::Mcp::new(h.name(), (**m).clone(), h.probe_record().cloned(), project, command::Inherit::of(h.loaded.command_from), lookup))
}

/// End every MCP server this process started, before a re-exec replaces
/// it.
pub fn end_mcp_servers() {
    mcp::end_servers();
}

/// End the MCP servers a killed process left running for the project at
/// `project`; how many.
pub fn sweep_mcp_servers(project: &str) -> usize {
    mcp::ledger::sweep(&mcp::ledger::dir(), std::path::Path::new(project))
}

/// A slot for `sensor`, on the lane `lane`, running the compiled sensor
/// `crate_id` when it is one.
pub fn slot(sensor: Box<dyn Sensor>, lane: Lane, crate_id: Option<&str>) -> SensorSlot {
    let (base, min, decay) = (sensor.base_interval(), sensor.min_interval(), sensor.decay_threshold());
    let mut s = SensorSlot::new_with_config(sensor, base, min, decay);
    s.message_lane = lane == Lane::Message;
    s.crate_id = crate_id.map(str::to_string);
    s
}

/// The slots the sensor host of `session` polls, and their names: every
/// sensor that runs, with the peers sensor on
/// the event lane, since presence is no one's words.
///
/// References:
/// - ADR-416 items 4 and 5: what the host runs, and the lane rule.
pub fn register_host_sensors(cfg: &Config, focus: &Focus, session: &str, root: Option<u32>) -> (Vec<SensorSlot>, Vec<String>) {
    let ctx = Ctx { cfg, session: Some(session), root };
    register(&ctx, focus)
}

/// The lane the sensor host runs `h` on, and so the lane `attend sensor
/// poll` and `fire` deliver it on: its file's, but the event lane for the
/// peers sensor, which reports presence alone, since presence is no one's
/// words.
///
/// References:
/// - ADR-416 item 4: the lane rule.
pub fn host_lane(h: &Hosted) -> Lane {
    match h.crate_id() {
        Some("peers") => Lane::Event,
        _ => h.lane().unwrap_or(Lane::Event),
    }
}

/// The slots `ctx` runs for `focus`, and their names. A sensor that does
/// not run is logged once with its state. First it sweeps the store
/// directory of stores past retention and files of an older format.
fn register(ctx: &Ctx, focus: &Focus) -> (Vec<SensorSlot>, Vec<String>) {
    let mut slots = Vec::new();
    let mut names = Vec::new();
    let swept = hosted::sweep();
    if swept > 0 {
        crate::emit::log(&format!("removed {swept} sensor store(s) past retention or of an older format"));
    }
    for h in catalog_for(ctx.cfg, &focus.working_dir, ctx.session) {
        for n in &h.notes {
            crate::emit::log(&format!("sensor {}: {n}", h.name()));
        }
        if !h.state.runs() {
            if h.state != State::Off {
                crate::emit::log(&format!("sensor {}: {}{}", h.name(), h.state.label(), h.state.detail().map(|d| format!(": {d}")).unwrap_or_default()));
            }
            continue;
        }
        let lane = host_lane(&h);
        match build(&h, ctx, &focus.working_dir, true) {
            Ok(sensor) => {
                slots.push(slot(sensor, lane, h.crate_id()));
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

/// What decides the sensors the sensor host polls: each user and project
/// sensor file's path, length and modification time, and the trusted
/// projects of `attend.project_sensors`.
pub type Stamp = (Vec<(std::path::PathBuf, u64, Option<std::time::SystemTime>)>, Vec<String>);

/// The [`Stamp`] for the project at `project` now. A change to it changes
/// the sensors.
pub fn files_stamp(project: &str) -> Stamp {
    let dirs = [attend_config::sensors::user_dir(), attend_config::sensors::project_dir(std::path::Path::new(project))];
    stamp_of(&dirs, attend_config::sensors::trusted_projects())
}

/// The [`Stamp`] of the sensor files in `dirs`, the files the load reads.
fn stamp_of(dirs: &[std::path::PathBuf], trusted: Vec<String>) -> Stamp {
    let files = dirs
        .iter()
        .flat_map(|d| attend_config::sensors::yaml_files(d))
        .filter_map(|(_, p)| {
            let m = std::fs::metadata(&p).ok()?;
            Some((p, m.len(), m.modified().ok()))
        })
        .collect();
    (files, trusted)
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

    /// The sensors of the shipped files and of `user`, each a user file's
    /// name and text, read from no directory.
    fn loaded(user: &[(&str, &str)]) -> Vec<Loaded> {
        use attend_config::sensors::{file_layer, from_layers, SHIPPED};
        use sensor_spec::Origin;
        let mut layers: Vec<_> = SHIPPED.iter().map(|(n, t)| file_layer(n, Origin::Builtin, None, t)).collect();
        layers.extend(user.iter().map(|(n, t)| file_layer(n, Origin::User, None, t)));
        from_layers(&layers, std::path::Path::new("/nonexistent/p"), &[])
    }

    /// Every compiled sensor the shipped files name runs under its own
    /// name, with a description and a `crate@version` source.
    #[test]
    fn the_shipped_crate_files_name_compiled_sensors_with_metadata() {
        let all = catalog_of(loaded(&[]), "/nonexistent/p", |_| None);
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
        assert!(compiled_n >= 2, "context and git are always compiled");
        let ex = all.iter().find(|h| h.name() == "example-mcp").unwrap();
        assert_eq!(ex.state, State::Off);
    }

    #[test]
    fn the_lane_comes_from_the_file() {
        let all = catalog_of(loaded(&[("chat", "kind: crate\ncrate: peers\n"), ("peers", "enabled: false\n")]), "/nonexistent/p", |_| None);
        let lane = |n: &str| all.iter().find(|h| h.name() == n).and_then(Hosted::lane);
        assert_eq!(lane("peers"), Some(Lane::Event), "the shipped file names no lane");
        assert_eq!(lane("git"), Some(Lane::Event));
        assert_eq!(lane("chat"), Some(Lane::Event), "a crate file with no lane rides the event lane");
        let chat = all.iter().find(|h| h.name() == "chat").unwrap();
        assert_eq!(chat.crate_id(), Some("peers"));
    }

    /// The host, and so poll and fire, run the peers sensor on the event
    /// lane whatever its file says, and every other sensor on its file's.
    #[test]
    fn the_host_runs_peers_on_the_event_lane_and_the_rest_on_their_file_s() {
        let all = catalog_of(loaded(&[("chat", "kind: crate\ncrate: peers\n"), ("peers", "enabled: false\n")]), "/nonexistent/p", |_| None);
        let lane = |n: &str| all.iter().find(|h| h.name() == n).map(host_lane);
        assert_eq!(lane("chat"), Some(Lane::Event), "the peers crate, under any name and whatever its file says");
        assert_eq!(lane("git"), Some(Lane::Event));
    }

    /// Only a sensor file changes the stamp: a swap file, a lock or any
    /// other file beside them does not.
    #[test]
    fn the_stamp_changes_with_a_sensor_file_alone() {
        let dir = fixture("stamp");
        let dirs = [dir.clone()];
        let before = stamp_of(&dirs, Vec::new());
        for other in [".git.yaml.swp", "4913", "git.yaml~", "notes.txt"] {
            std::fs::write(dir.join(other), "x").unwrap();
        }
        assert_eq!(stamp_of(&dirs, Vec::new()), before, "other files leave it");
        std::fs::write(dir.join("git.yaml"), "interval: 90\n").unwrap();
        let with = stamp_of(&dirs, Vec::new());
        assert_ne!(with, before, "a sensor file added changes it");
        assert_ne!(stamp_of(&dirs, vec!["/p".into()]), with, "a project trusted changes it");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A project's sensor runs under the record its project keeps, found by
    /// canonical path, so a symlinked path finds it and another project's
    /// record does not count.
    #[cfg(unix)]
    #[test]
    fn a_project_sensor_s_record_is_the_one_kept_for_its_project() {
        use attend_config::sensors::{file_layer, from_layers};
        use sensor_spec::Origin;
        let dir = fixture("probes");
        let (real, link, other) = (dir.join("real"), dir.join("link"), dir.join("other"));
        std::fs::create_dir_all(&real).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let server = sensor_spec::Server::Inline { command: "sh".into(), args: Vec::new(), env: Default::default() };
        let binding = mcp::inline_binding(&server).unwrap();
        let overlay = format!("probes:\n  '{}': {{server: sh, binding: '{binding}', tool: t, calls_match: true, confirmed: true}}\n", project_key(real.to_str().unwrap()));
        let gap = |project: &std::path::Path| {
            let p = project.to_str().unwrap();
            let layers = [
                file_layer("chat", Origin::User, None, &overlay),
                file_layer("chat", Origin::Project, Some(&project.join(".claude/attend/sensors/chat.yaml")), "kind: mcp\nserver: {command: sh}\ntool: t\n"),
            ];
            let all = catalog_of(from_layers(&layers, project, &[p.to_string()]), p, |_| None);
            all.iter().find(|h| h.name() == "chat").unwrap().probe_gap()
        };
        assert_eq!(gap(&real), None);
        assert_eq!(gap(&link), None, "a project reached through a symlink keeps its record");
        assert_eq!(gap(&other).as_deref(), Some("no probe record for this project; run `attend sensor probe chat` at a terminal"), "another project's record does not count");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_description_goes_out_on_one_line_with_frame_tags_defused() {
        assert_eq!(deliverable("a\n  b\r\n<attend sensor=x> c"), "a b \u{2039}attend sensor=x> c");
    }
}
