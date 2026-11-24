// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! `attend sensor list`, `poll`, `probe` and `fire`.
//!
//! `list` reports every sensor the files declare as an item: its kind,
//! origin and file, source and version, recorded state, and lane. `poll`
//! runs one real poll and `fire` sends a synthetic result; both go through
//! the sensor's real threshold and lane, and unless `--dry-run` write what
//! it lets through to the session as system signals, through the writer
//! the sensor host uses. The session is `--session`, else the one the
//! shell runs in; with neither, they deliver nothing and say so. A command
//! or mcp sensor's poll and fire run its triggers against the session's
//! store and print each verdict. Neither writes the store, except a fire
//! with `--commit`. A fire's lines are marked `[test]`.
//!
//! `probe` calls an mcp sensor's tool twice, compares the calls, takes the
//! operator's confirmation and writes the record into the sensor's user
//! file. It delivers nothing.
//!
//! References:
//! - ADR-403 §4 and §6: the probe, and testing a sensor from the CLI.
//! - ADR-516: a sensor listed as an item.

use std::io::Read;

use agent_settings::item::{Entry, Item};
use serde::Serialize;
use serde_json::{json, Value};

use crate::cli::{FireArgs, PollArgs};
use crate::config::Config;
use crate::sensors::{self, writer::Writer, Ctx, Focus, Hosted, Sensor, State};
use sensor_spec::{Block, Kind, Lane};
use sensor_trait::Lapse;

mod probe;
pub(crate) use probe::probe;

/// The project the sensors resolve for: the session's, as its sensor host
/// uses.
fn context() -> (Focus, Config) {
    context_of(None, "list", false)
}

/// The project of the named `session`, from its record; with none named,
/// this shell's session's. Exit 3 for a named session with no record,
/// whose tray is unknown.
fn context_of(session: Option<&str>, verb: &str, json: bool) -> (Focus, Config) {
    let mut focus = Focus::default_focus();
    focus.working_dir = match session {
        None => crate::util::own_origin_cwd(),
        Some(id) => attend_presence::session::origin_path(id).unwrap_or_else(|| {
            let why = format!("no record of session {id}; its tray is unknown");
            if json {
                crate::cmd::print_json(&json!({ "error": why }));
            } else {
                eprintln!("attend sensor {verb}: {why}");
            }
            std::process::exit(agent_settings::exit::REJECTED);
        }),
    };
    let cfg = Config::load(&focus.working_dir);
    (focus, cfg)
}

/// The session a poll or fire reads the store of and delivers to:
/// `given`, else the session this shell runs in. Exit 2 for an id no
/// signal can be addressed to.
fn target(given: Option<&str>, verb: &str, json: bool) -> Option<String> {
    match given {
        Some(id) if !agent_identity::signal::is_tag_value(id) => {
            let why = format!("{id:?} is not a session id a signal can be addressed to");
            crate::cmd::refuse_usage(&format!("attend sensor {verb}: {why}"), &why, json)
        }
        Some(id) => Some(id.to_string()),
        None => {
            let ident = attend_presence::session::identity();
            ident.session_resolved.then(|| ident.session_id.clone())
        }
    }
}

/// The writer to `session`, or exit 3 when a delivery has no session to go
/// to. A dry run delivers nothing and needs none.
fn writer(session: Option<&str>, project: &str, dry_run: bool, verb: &str, json: bool) -> Option<Writer> {
    if dry_run {
        return None;
    }
    let refuse = |why: String| -> ! {
        if json {
            crate::cmd::print_json(&json!({ "error": why }));
        } else {
            eprintln!("attend sensor {verb}: {why}");
        }
        std::process::exit(agent_settings::exit::REJECTED);
    };
    let Some(session) = session else {
        refuse("no session to deliver to; name one with --session, run it from the session's shell, or add --dry-run".into())
    };
    Some(Writer::new(session, project).unwrap_or_else(|why| refuse(why)))
}

pub(crate) fn list(json: bool) {
    let (focus, cfg) = context();
    let all = sensors::catalog(&cfg, &focus.working_dir);
    if json {
        crate::cmd::print_json(&Value::Array(all.iter().map(entry).collect()));
        // stdout stays one JSON document; the note goes beside it.
        for note in removed(&focus.working_dir) {
            eprintln!("{note}");
        }
        return;
    }
    let mut t = agent_fmt::Table::new(&["Sensor", "Kind", "Origin", "State", "Lane", "Interval", "Source", "File"]);
    t.align(0, agent_fmt::Align::Left);
    // Column 0 is short; auto-fit would push the rest far right.
    t.no_auto_fit();
    for h in &all {
        let i = item(h);
        let interval = sensors::intervals(h).map(|(a, b)| format!("{}s / {}s", a.as_secs(), b.as_secs())).unwrap_or_else(|| "-".into());
        let source = match (&i.source, &i.version) {
            (Some(s), Some(v)) => format!("{s}@{v}"),
            (Some(s), None) => s.clone(),
            (None, _) => "-".into(),
        };
        let path = i.file().map_or("-".into(), |p| tilde(&p.display().to_string()));
        let lane = h.lane().map_or("-", Lane::as_str);
        let kind = i.kind.map_or("-", |k| k.word());
        t.add_owned(vec![h.name().to_string(), kind.into(), i.origin_text(), i.state.word().into(), lane.into(), interval, source, path]);
    }
    t.print();
    let mut lines: Vec<String> = all
        .iter()
        .flat_map(|h| h.state.detail().map(|d| format!("{}: {}: {d}", h.name(), h.state.label())).into_iter().chain(h.notes.iter().map(|n| format!("{}: {n}", h.name()))))
        .collect();
    lines.extend(removed(&focus.working_dir));
    if !lines.is_empty() {
        println!();
        for l in lines {
            println!("  {l}");
        }
    }
}

/// A note for each file kept for a sensor attend removed
/// ([`attend_config::sensors::removed_notes`]): keepwarm's names the config
/// key that replaced it. Such a file defines no sensor, so the table does
/// not list it.
// #1316 grace: remove after the next release.
fn removed(project: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (p, note) in attend_config::sensors::removed_notes(std::path::Path::new(project)) {
        let name = p.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        // Each note names its file: keepwarm's names every file already,
        // so the one line serves all of them.
        let line = match note.contains(&p.display().to_string()) {
            true => format!("{name}: {note}"),
            false => format!("{name}: {}: {note}", p.display()),
        };
        if !out.contains(&line) {
            out.push(line);
        }
    }
    out
}

/// `~` for the home directory, so the table stays narrow.
fn tilde(p: &str) -> String {
    match claude_sessions::env_home_dir().map(|h| h.display().to_string()) {
        Some(h) if p.starts_with(&h) => format!("~{}", &p[h.len()..]),
        _ => p.to_string(),
    }
}

/// One sensor as an item (ADR-516): its files' kind, origin, path and
/// overrides, and attend's state with why, the source that runs it, and a
/// crate's version split from `<crate>@<version>`.
fn item(h: &Hosted) -> Item {
    let spec = h.loaded.spec.as_ref();
    let (source, version) = match (spec.map(|s| &s.block), &h.compiled) {
        (Some(Block::Crate(_)), Some(c)) => match c.source.split_once('@') {
            Some((s, v)) => (Some(s.to_string()), Some(v.to_string())),
            None => (Some(c.source.clone()), None),
        },
        (Some(_), _) => (spec.map(|s| s.source()), None),
        (None, _) => (None, None),
    };
    let item = Item { source, version, detail: h.state.detail().map(str::to_string), ..h.loaded.item() };
    item.with_state(h.state.word())
}

/// A sensor's own fields, after its descriptor: `description`, `interval`
/// and `min_interval` (ADR-516 §2), then what only attend knows of it.
#[derive(Serialize)]
struct Fields {
    description: Option<String>,
    interval: Option<u64>,
    min_interval: Option<u64>,
    threshold: Option<f64>,
    lane: Option<&'static str>,
    /// What runs but is not as written.
    notes: Vec<String>,
    last_poll: Option<Value>,
}

/// One sensor as `--json` prints it: the descriptor, then its fields.
fn entry(h: &Hosted) -> Value {
    let spec = h.loaded.spec.as_ref();
    let description = spec.map(|s| s.description.clone()).filter(|d| !d.is_empty()).or_else(|| h.compiled.as_ref().map(|c| c.description.clone()));
    let fields = Fields {
        description,
        interval: spec.map(|s| s.cadence.interval),
        min_interval: spec.map(|s| s.cadence.min_interval),
        threshold: spec.map(|s| s.cadence.threshold),
        lane: h.lane().map(Lane::as_str),
        notes: h.notes.clone(),
        last_poll: h.last.as_ref().map(|p| json!({ "at": p.at, "ok": p.ok, "detail": p.detail, "observations": p.observations })),
    };
    serde_json::to_value(Entry { item: item(h), fields }).unwrap_or_default()
}

/// A sensor that returns one prepared result, under another sensor's
/// settings: a real poll's result, or a test fire's input.
struct Replay {
    of: Box<dyn Sensor>,
    result: Vec<(f64, String)>,
}

/// What a test fire marks its lines with.
const TEST_MARK: &str = "[test] ";

impl Sensor for Replay {
    fn name(&self) -> &str {
        self.of.name()
    }
    fn poll(&mut self, _focus: &Focus) -> Vec<(f64, String)> {
        std::mem::take(&mut self.result)
    }
    fn emission_threshold(&self) -> f64 {
        self.of.emission_threshold()
    }
    fn base_interval(&self) -> std::time::Duration {
        self.of.base_interval()
    }
    fn min_interval(&self) -> std::time::Duration {
        self.of.min_interval()
    }
    fn decay_threshold(&self) -> u32 {
        self.of.decay_threshold()
    }
    /// The lapse the sensor gave the line, marked or not.
    fn lapse(&self, description: &str) -> Option<Lapse> {
        self.of.lapse(description.strip_prefix(TEST_MARK).unwrap_or(description))
    }
}

/// What passing `result` through the sensor's slot comes to.
struct Outcome {
    lane: Lane,
    /// The lines that go out, each with its lapse, or none when the
    /// threshold holds them.
    delivered: Option<Vec<(String, Option<Lapse>)>>,
    /// The threshold let the lines through and the event lane's cut held
    /// them, so they are logged and not written.
    cut: bool,
    accumulated: f64,
    threshold: f64,
}

fn run_through(of: Box<dyn Sensor>, lane: Lane, result: Vec<(f64, String)>, focus: &Focus) -> Outcome {
    let threshold = of.emission_threshold();
    let mut slot = sensors::slot(Box::new(Replay { of, result }), lane, None);
    slot.poll(focus);
    let accumulated = slot.accumulator.magnitude;
    let ready = (slot.message_lane && accumulated > 0.0) || slot.ready_to_disclose();
    let cut = ready && crate::sensors::writer::below_cut(lane, accumulated);
    let delivered = (ready && !cut).then(|| slot.accumulator.drain_events().into_iter().map(|l| (l.clone(), slot.accumulator.lapse_of(&l))).collect());
    Outcome { lane, delivered, cut, accumulated, threshold }
}

/// Find `name`, or exit 2 naming the ones there are.
fn find(all: Vec<Hosted>, name: &str, json: bool) -> Hosted {
    let names: Vec<String> = all.iter().map(|h| h.name().to_string()).collect();
    all.into_iter().find(|h| h.name() == name).unwrap_or_else(|| {
        let why = format!("no sensor named {name}; there are {}", names.join(", "));
        crate::cmd::refuse_usage(&format!("attend sensor: {why}"), &why, json)
    })
}

/// Exit 3 when `h` is not a sensor attend can poll here. A poll of an mcp
/// sensor calls its tool, so it needs a probe record that holds whether
/// or not the sensor is enabled; a fire calls no server, so it takes one
/// with no record yet.
fn require_hosted(h: &Hosted, verb: &str, json: bool) {
    let runnable = h.state.runs() || h.state == State::Off || (verb == "fire" && matches!(h.state, State::Unprobed(_)));
    let kind_runs = matches!(h.loaded.spec.as_ref().map(|s| s.kind), Some(Kind::Crate | Kind::Command | Kind::Mcp));
    let gap = (verb == "poll").then(|| h.probe_gap()).flatten();
    if runnable && kind_runs && gap.is_none() {
        return;
    }
    let why = match &gap {
        Some(g) => format!("{} has no probe record that holds: {g}", h.name()),
        None => format!("{} is {}{}", h.name(), h.state.label(), h.state.detail().map(|d| format!(": {d}")).unwrap_or_default()),
    };
    if json {
        crate::cmd::print_json(&json!({ "sensor": h.name(), "state": h.state.label(), "detail": h.state.detail(), "error": why }));
    } else {
        eprintln!("attend sensor {verb}: {why}");
    }
    std::process::exit(agent_settings::exit::REJECTED);
}

/// The sensor `h`, keyed by `session` so it reads the session's store,
/// writing it only with `record`; or exit 3 saying why it cannot run.
fn open(h: &Hosted, cfg: &Config, project: &str, session: Option<&str>, record: bool, verb: &str) -> sensors::hosted::FileSensor {
    let root = attend_presence::process::claude_ancestor(std::process::id());
    sensors::file_sensor(h, &Ctx { cfg, session, root }, project, record).unwrap_or_else(|why| {
        eprintln!("attend sensor {verb}: {}: {why}", h.name());
        std::process::exit(agent_settings::exit::REJECTED);
    })
}

/// Whether `h` is a sensor whose triggers read a JSON poll result: a
/// command sensor with `output: json`, or an mcp sensor.
fn declares_triggers(h: &Hosted) -> bool {
    match h.loaded.spec.as_ref().map(|s| &s.block) {
        Some(Block::Command(c)) => c.output == sensor_spec::Output::Json,
        Some(Block::Mcp(_)) => true,
        _ => false,
    }
}

/// Each trigger's verdict as `--json` prints it.
fn verdicts_json(s: &sensors::hosted::FileSensor) -> Value {
    Value::Array(s.verdicts().iter().map(|v| json!({ "trigger": v.trigger, "fired": !v.fired.is_empty(), "keys": v.fired })).collect())
}

/// Print each trigger's verdict, one line each.
fn print_verdicts(h: &Hosted, s: &sensors::hosted::FileSensor) {
    if !declares_triggers(h) {
        println!("triggers: none; {}", if matches!(h.loaded.spec.as_ref().map(|s| s.kind), Some(Kind::Crate)) { "a crate sensor's poll is compiled" } else { "output: lines takes none" });
        return;
    }
    if s.verdicts().is_empty() {
        println!("triggers: none declared");
    }
    for v in s.verdicts() {
        match v.fired.as_slice() {
            [] => println!("  trigger {}: quiet", v.trigger),
            keys => println!("  trigger {}: fired for {}", v.trigger, keys.join(", ")),
        }
    }
}

pub(crate) fn poll(a: &PollArgs) {
    let session = target(a.session.as_deref(), "poll", a.json);
    let (focus, cfg) = context_of(a.session.as_deref(), "poll", a.json);
    let h = find(sensors::catalog_for(&cfg, &focus.working_dir, session.as_deref()), &a.name, a.json);
    require_hosted(&h, "poll", a.json);
    let writer = writer(session.as_deref(), &focus.working_dir, a.dry_run, "poll", a.json);
    // A test poll compares against the session's store and writes nothing.
    let mut sensor = open(&h, &cfg, &focus.working_dir, session.as_deref(), false, "poll");
    let result = sensor.poll(&focus);
    let failure = sensor.take_failure();
    let triggers = verdicts_json(&sensor);
    let state = sensor.state().cloned();
    if !a.json {
        print_verdicts(&h, &sensor);
    }
    let out = run_through(Box::new(sensor), sensors::host_lane(&h), result.clone(), &focus);
    let (delivered, ids) = deliver(&h, &out, writer.as_ref());
    if a.json {
        crate::cmd::print_json(&json!({
            "sensor": h.name(),
            "session": session,
            "state": h.state.label(),
            "failure": failure,
            "result": result.iter().map(|(m, d)| json!({ "magnitude": m, "description": d })).collect::<Vec<_>>(),
            "triggers": triggers,
            "fields": state,
            "lane": out.lane.as_str(),
            "accumulated": out.accumulated,
            "threshold": out.threshold,
            "delivered": delivered,
            "cut": out.cut,
            "signals": ids,
            "dry_run": a.dry_run,
        }));
        return;
    }
    if let Some(f) = &failure {
        println!("{}: poll failed: {f}", h.name());
    }
    if result.is_empty() {
        println!("{}: no observations; a fresh store's first poll sets its baseline", h.name());
    }
    for (m, d) in &result {
        println!("  {m:>4.1}  {d}");
    }
    verdict(&h, &out, writer.as_ref(), ids.as_deref());
}

pub(crate) fn fire(a: &FireArgs) {
    let session = target(a.session.as_deref(), "fire", a.json);
    let (focus, cfg) = context_of(a.session.as_deref(), "fire", a.json);
    let (name, trigger) = match a.target.split_once('/') {
        Some((n, t)) => (n, Some(t)),
        None => (a.target.as_str(), None),
    };
    let h = find(sensors::catalog_for(&cfg, &focus.working_dir, session.as_deref()), name, a.json);
    if let (Some(t), Some(spec)) = (trigger, h.loaded.spec.as_ref()) {
        if !spec.triggers.iter().any(|x| x.name == t) {
            let names: Vec<&str> = spec.triggers.iter().map(|x| x.name.as_str()).collect();
            let why = match names.is_empty() {
                true => format!("{name} has no triggers; fire the sensor itself"),
                false => format!("{name} has no trigger {t}; it has {}", names.join(", ")),
            };
            crate::cmd::refuse_usage(&format!("attend sensor fire: {why}"), &why, a.json);
        }
    }
    require_hosted(&h, "fire", a.json);
    let spec = h.loaded.spec.as_ref().expect("a hosted sensor loads");
    let refuse = |why: String| -> ! { crate::cmd::refuse_usage(&format!("attend sensor fire: {why}"), &why, a.json) };
    if a.commit && session.is_none() {
        refuse("no session here, so --commit keeps nothing; name one with --session or run it from the session's shell".into());
    }
    if let (Some(t), true) = (trigger, a.commit) {
        refuse(format!("--commit fires the whole sensor, since advancing {t} alone would leave the other triggers' memory behind; fire {name} --commit"));
    }
    let writer = writer(session.as_deref(), &focus.working_dir, a.dry_run, "fire", a.json);
    // A test fire leaves the store as it was unless --commit keeps it.
    let mut sensor = open(&h, &cfg, &focus.working_dir, session.as_deref(), a.commit, "fire");
    let input: Vec<(f64, String)> = if declares_triggers(&h) {
        let Some(src) = &a.input else { refuse(format!("{name} reads a JSON poll result; give one with --input FILE or --input -")) };
        let doc = read_text(src).unwrap_or_else(|why| refuse(why));
        let result: Value = serde_json::from_str(&doc).unwrap_or_else(|e| refuse(format!("the input is not one JSON document: {e}")));
        if let Some(t) = trigger {
            sensor.only(t);
        }
        let out = sensor.evaluate(&result);
        if a.commit {
            if let Err(why) = sensor.commit() {
                if a.json {
                    crate::cmd::print_json(&json!({ "sensor": h.name(), "error": why }));
                } else {
                    eprintln!("attend sensor fire: {why}");
                }
                std::process::exit(agent_settings::exit::REJECTED);
            }
        }
        if let Some(f) = sensor.take_failure() {
            eprintln!("attend sensor fire: {name}: {f}");
        }
        out
    } else {
        match &a.input {
            Some(src) => read_text(src).and_then(|t| parse_input(&t)).unwrap_or_else(|why| refuse(why)),
            None => vec![(spec.cadence.threshold, format!("test fire of {name}"))],
        }
    };
    let triggers = verdicts_json(&sensor);
    if !a.json && declares_triggers(&h) {
        print_verdicts(&h, &sensor);
    }
    let input: Vec<(f64, String)> = input.into_iter().map(|(m, d)| (m, format!("{TEST_MARK}{}", sensors::deliverable(&d)))).collect();
    let out = run_through(Box::new(sensor), sensors::host_lane(&h), input.clone(), &focus);
    let (delivered, ids) = deliver(&h, &out, writer.as_ref());
    let commit_note = (a.commit && !declares_triggers(&h)).then_some("this sensor keeps no trigger memory or declared state, so --commit changes nothing");
    if a.json {
        crate::cmd::print_json(&json!({
            "sensor": h.name(),
            "session": session,
            "trigger": trigger,
            "test": true,
            "input": input.iter().map(|(m, d)| json!({ "magnitude": m, "description": d })).collect::<Vec<_>>(),
            "triggers": triggers,
            "lane": out.lane.as_str(),
            "accumulated": out.accumulated,
            "threshold": out.threshold,
            "delivered": delivered,
            "cut": out.cut,
            "signals": ids,
            "dry_run": a.dry_run,
            "note": commit_note,
        }));
        return;
    }
    if let Some(n) = commit_note {
        println!("{n}");
    }
    verdict(&h, &out, writer.as_ref(), ids.as_deref());
}

/// Write what the slot let through through `writer`, one system signal a
/// line; with no writer, a dry run, write nothing. Returns the lines that
/// went out, or would have, and the id of each signal written. A line that
/// could not be written is said on stderr and has no id.
fn deliver(h: &Hosted, out: &Outcome, writer: Option<&Writer>) -> (Option<Vec<String>>, Option<Vec<String>>) {
    let Some(lines) = &out.delivered else { return (None, None) };
    let ids = writer.map(|w| {
        lines
            .iter()
            .filter_map(|(line, lapse)| {
                w.write(h.name(), out.lane, *lapse, line).map_err(|why| eprintln!("attend sensor: {}: not written: {why}", h.name())).ok()
            })
            .collect()
    });
    (Some(lines.iter().map(|(l, _)| l.clone()).collect()), ids)
}

fn verdict(h: &Hosted, out: &Outcome, writer: Option<&Writer>, ids: Option<&[String]>) {
    let lane = format!("{} lane", out.lane.as_str());
    match (&out.delivered, writer, ids) {
        (Some(_), Some(w), Some(ids)) => println!("{}: delivered on the {lane} to session {}: {}", h.name(), w.session(), ids.join(", ")),
        (Some(_), _, _) => println!("{}: would deliver on the {lane} (dry run)", h.name()),
        (None, _, _) if out.cut => println!(
            "{}: {:.1} accumulated is under the {lane}'s cut of {:.1}, so attend logs it and writes no signal",
            h.name(),
            out.accumulated,
            crate::sensors::writer::EVENT_CUT
        ),
        (None, _, _) => println!("{}: held on the {lane}: {:.1} accumulated of a {:.1} threshold", h.name(), out.accumulated, out.threshold),
    }
    if h.state == State::Off {
        println!("{}: off, so the sensor host does not poll it", h.name());
    }
}

/// The text of a file, or of stdin for `-`.
fn read_text(src: &str) -> Result<String, String> {
    let mut text = String::new();
    match src {
        "-" => std::io::stdin().read_to_string(&mut text).map(|_| ()).map_err(|e| format!("cannot read stdin: {e}"))?,
        p => text = std::fs::read_to_string(p).map_err(|e| format!("cannot read {p}: {e}"))?,
    }
    Ok(text)
}

/// A synthetic result for a sensor without triggers: a JSON list of
/// `{magnitude, description}`, or `magnitude|description` lines.
fn parse_input(text: &str) -> Result<Vec<(f64, String)>, String> {
    let check = |m: f64, d: &str| -> Result<(f64, String), String> {
        if !(0.0..=100.0).contains(&m) {
            return Err(format!("magnitude {m} is outside 0 to 100"));
        }
        if d.trim().is_empty() {
            return Err("an observation needs a description".into());
        }
        Ok((m, d.to_string()))
    };
    if text.trim_start().starts_with('[') {
        let v: Vec<Value> = serde_json::from_str(text).map_err(|e| format!("the input is not a JSON list: {e}"))?;
        return v
            .iter()
            .map(|o| {
                let m = o.get("magnitude").and_then(Value::as_f64).ok_or("each item needs a number magnitude")?;
                check(m, o.get("description").and_then(Value::as_str).unwrap_or_default())
            })
            .collect();
    }
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let (m, d) = l.split_once('|').ok_or_else(|| format!("`{l}` is not magnitude|description"))?;
            check(m.trim().parse().map_err(|_| format!("`{}` is not a magnitude", m.trim()))?, d.trim())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped sensors with a user's command sensor and a user file
    /// over the shipped git, read from no directory.
    fn listed() -> Vec<Value> {
        use attend_config::sensors::{file_layer, from_layers, SHIPPED};
        use sensor_spec::Origin;
        let mut layers: Vec<_> = SHIPPED.iter().map(|(n, t)| file_layer(n, Origin::Builtin, None, t)).collect();
        let user = std::path::Path::new("/c/attend/sensors");
        layers.push(file_layer("weather", Origin::User, Some(&user.join("weather.yaml")), "kind: command\nargv: [curl, -s, x]\noutput: json\n"));
        layers.push(file_layer("chat", Origin::User, Some(&user.join("chat.yaml")), "kind: mcp\nserver: slack\ntool: list\n"));
        let probed = "kind: mcp\nserver: slack\ntool: list\nallow_unannotated: true\nprobe: {server: slack, binding: user:0, tool: list, args: {}, calls_match: true, confirmed: true}\n";
        layers.push(file_layer("probed", Origin::User, Some(&user.join("probed.yaml")), probed));
        layers.push(file_layer("git", Origin::User, Some(&user.join("git.yaml")), "interval: 90\n"));
        let loaded = from_layers(&layers, std::path::Path::new("/nonexistent/p"), &[]);
        sensors::catalog_of(loaded, "/nonexistent/p", |_| None).iter().map(entry).collect()
    }

    #[test]
    fn each_sensor_lists_as_an_item_then_its_fields() {
        let all = listed();
        let find = |n: &str| all.iter().find(|e| e["name"] == n).unwrap_or_else(|| panic!("no {n}")).clone();
        let git = find("git");
        let keys: Vec<&str> = git.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(&keys[..13], ["family", "name", "kind", "origin", "path", "overrides", "source", "version", "state", "detail", "description", "interval", "min_interval"]);
        assert_eq!((git["family"].as_str(), git["kind"].as_str(), git["origin"].as_str(), git["state"].as_str()), (Some("sensor"), Some("crate"), Some("builtin"), Some("active")));
        assert_eq!(git["source"].as_str(), Some("attend"), "the crate, split from its version");
        assert!(git["version"].as_str().is_some_and(|v| !v.contains('@')));
        assert_eq!(git["overrides"], json!([{ "origin": "user", "how": "extends", "path": "/c/attend/sensors/git.yaml" }]));
        assert_eq!(git["interval"], json!(90));
        // A command sensor runs, and an mcp sensor with no probe record
        // reads degraded, with the reason.
        let weather = find("weather");
        assert_eq!((weather["kind"].as_str(), weather["origin"].as_str(), weather["state"].as_str()), (Some("command"), Some("user"), Some("active")));
        assert_eq!((weather["path"].as_str(), weather["source"].as_str(), weather["detail"].as_str()), (Some("/c/attend/sensors/weather.yaml"), Some("curl"), None));
        let chat = find("chat");
        assert_eq!((chat["kind"].as_str(), chat["state"].as_str()), (Some("mcp"), Some("degraded")));
        assert_eq!(chat["detail"].as_str(), Some("no probe record; run `attend sensor probe chat` at a terminal"), "{chat}");
        // One whose record holds runs, and its override is noted.
        let probed = find("probed");
        assert_eq!((probed["state"].as_str(), probed["detail"].as_str()), (Some("active"), None), "{probed}");
        assert_eq!(probed["notes"], json!(["overridden: allow_unannotated: true calls list even though it declares no readOnlyHint"]));
    }

    #[test]
    fn input_reads_as_json_or_lines() {
        assert_eq!(parse_input(r#"[{"magnitude": 2.5, "description": "a"}]"#).unwrap(), vec![(2.5, "a".to_string())]);
        assert_eq!(parse_input("3|b\n\n1.5 | c\n").unwrap(), vec![(3.0, "b".to_string()), (1.5, "c".to_string())]);
        assert!(parse_input("200|x").is_err());
        assert!(parse_input("2|").is_err());
        assert!(parse_input("nope").is_err());
    }

    struct Fixed(f64);
    impl Sensor for Fixed {
        fn name(&self) -> &str {
            "fixed"
        }
        fn poll(&mut self, _: &Focus) -> Vec<(f64, String)> {
            Vec::new()
        }
        fn emission_threshold(&self) -> f64 {
            self.0
        }
        fn base_interval(&self) -> std::time::Duration {
            std::time::Duration::from_secs(30)
        }
        fn min_interval(&self) -> std::time::Duration {
            std::time::Duration::from_secs(10)
        }
        fn decay_threshold(&self) -> u32 {
            3
        }
    }

    #[test]
    fn the_threshold_holds_an_event_and_the_message_lane_lets_it_through() {
        let focus = Focus::default_focus();
        let held = run_through(Box::new(Fixed(2.0)), Lane::Event, vec![(1.0, "x".into())], &focus);
        assert!(held.delivered.is_none() && held.accumulated == 1.0);
        let out = run_through(Box::new(Fixed(2.0)), Lane::Message, vec![(1.0, "x".into())], &focus);
        assert_eq!(out.delivered, Some(vec![("x".to_string(), None)]));
        let out = run_through(Box::new(Fixed(2.0)), Lane::Event, vec![(3.5, "y".into())], &focus);
        assert!(out.delivered.is_some(), "a magnitude over the threshold goes out");
    }
}
