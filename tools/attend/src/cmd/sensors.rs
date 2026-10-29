//! `attend sensor list`, `poll` and `fire` (ADR-403 §6).
//!
//! `list` reports every sensor the files declare: its kind, origin and file,
//! source and version, lane and recorded state. `poll` runs one real poll
//! and `fire` sends a synthetic result; both go through the sensor's real
//! threshold and lane, deliver as `attend run` would unless `--dry-run`, and
//! record nothing in the sensor's store. A fire's lines are marked `[test]`.

use std::io::Read;

use serde_json::{json, Value};

use crate::cli::{FireArgs, PollArgs};
use crate::config::Config;
use crate::sensors::{self, Ctx, Focus, Hosted, Sensor, State};
use sensor_spec::{Block, Kind, Lane};

/// The project the sensors resolve for: the session's, as `attend run` uses.
fn context() -> (Focus, Config) {
    let mut focus = Focus::default_focus();
    focus.working_dir = crate::util::own_origin_cwd();
    let cfg = Config::load(&focus.working_dir);
    (focus, cfg)
}

pub(crate) fn list(json: bool) {
    let (focus, cfg) = context();
    let all = sensors::catalog(&cfg, &focus.working_dir);
    if json {
        crate::cmd::print_json(&Value::Array(all.iter().map(entry).collect()));
        return;
    }
    let mut t = agent_fmt::Table::new(&["Sensor", "Kind", "Origin", "State", "Lane", "Interval", "Source", "File"]);
    t.align(0, agent_fmt::Align::Left);
    // Column 0 is short; auto-fit would push the rest far right.
    t.no_auto_fit();
    for h in &all {
        let e = entry(h);
        let text = |k: &str| e[k].as_str().unwrap_or("-").to_string();
        let interval = sensors::intervals(h).map(|(a, b)| format!("{}s / {}s", a.as_secs(), b.as_secs())).unwrap_or_else(|| "-".into());
        let source = match e["version"].as_str() {
            Some(v) => format!("{}@{v}", text("source")),
            None => text("source"),
        };
        t.add_owned(vec![h.name().to_string(), text("kind"), text("origin"), text("state"), text("lane"), interval, source, tilde(&text("path"))]);
    }
    t.print();
    let lines: Vec<String> = all
        .iter()
        .flat_map(|h| h.state.detail().map(|d| format!("{}: {}: {d}", h.name(), h.state.label())).into_iter().chain(h.notes.iter().map(|n| format!("{}: {n}", h.name()))))
        .collect();
    if !lines.is_empty() {
        println!();
        for l in lines {
            println!("  {l}");
        }
    }
}

/// `~` for the home directory, so the table stays narrow.
fn tilde(p: &str) -> String {
    match claude_sessions::env_home_dir().map(|h| h.display().to_string()) {
        Some(h) if p.starts_with(&h) => format!("~{}", &p[h.len()..]),
        _ => p.to_string(),
    }
}

/// One sensor as `--json` prints it. `kind`, `origin` with `path`,
/// `source`, `version` and `state` name what the settings screens show.
fn entry(h: &Hosted) -> Value {
    let spec = h.loaded.spec.as_ref();
    let (source, version) = match (spec.map(|s| &s.block), &h.compiled) {
        (Some(Block::Crate(_)), Some(c)) => match c.source.split_once('@') {
            Some((s, v)) => (Some(s.to_string()), Some(v.to_string())),
            None => (Some(c.source.clone()), None),
        },
        (Some(_), _) => (spec.map(|s| s.source()), None),
        (None, _) => (None, None),
    };
    let description = spec.map(|s| s.description.clone()).filter(|d| !d.is_empty()).or_else(|| h.compiled.as_ref().map(|c| c.description.clone()));
    json!({
        "name": h.name(),
        "kind": spec.map(|s| s.kind.as_str()),
        "origin": h.loaded.origin().as_str(),
        "path": h.loaded.path().map(|p| p.display().to_string()),
        "source": source,
        "version": version,
        "state": h.state.label(),
        "detail": h.state.detail(),
        "enabled": spec.map(|s| s.enabled),
        "lane": h.lane().map(Lane::as_str),
        "description": description,
        "interval": spec.map(|s| s.cadence.interval),
        "min_interval": spec.map(|s| s.cadence.min_interval),
        "threshold": spec.map(|s| s.cadence.threshold),
        "files": h.loaded.files.iter().map(|f| json!({ "origin": f.origin.as_str(), "path": f.path.as_ref().map(|p| p.display().to_string()), "refused": f.refused })).collect::<Vec<_>>(),
        "notes": h.notes,
        "last_poll": h.last.as_ref().map(|p| json!({ "at": p.at, "ok": p.ok, "detail": p.detail, "observations": p.observations })),
    })
}

/// A sensor that returns one prepared result, under another sensor's
/// settings: a real poll's result, or a test fire's input.
struct Replay {
    of: Box<dyn Sensor>,
    result: Vec<(f64, String)>,
}

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
}

/// What passing `result` through the sensor's slot comes to.
struct Outcome {
    lane: Lane,
    /// The lines that go out, or none when the threshold holds them.
    delivered: Option<Vec<String>>,
    priority: &'static str,
    accumulated: f64,
    threshold: f64,
}

fn run_through(of: Box<dyn Sensor>, lane: Lane, result: Vec<(f64, String)>, focus: &Focus) -> Outcome {
    let threshold = of.emission_threshold();
    let mut slot = sensors::slot(Box::new(Replay { of, result }), lane);
    slot.poll(focus);
    let accumulated = slot.accumulator.magnitude;
    let ready = (slot.message_lane && accumulated > 0.0) || slot.ready_to_disclose();
    // The priority `attend run` gives a batch of this magnitude.
    let priority = match accumulated {
        m if m >= 5.0 => "high",
        m if m >= 3.0 => "medium",
        _ => "low",
    };
    Outcome { lane, delivered: ready.then(|| slot.accumulator.drain_events()), priority, accumulated, threshold }
}

/// Find `name`, or exit 2 naming the ones there are.
fn find(all: Vec<Hosted>, name: &str, json: bool) -> Hosted {
    let names: Vec<String> = all.iter().map(|h| h.name().to_string()).collect();
    all.into_iter().find(|h| h.name() == name).unwrap_or_else(|| {
        let why = format!("no sensor named {name}; there are {}", names.join(", "));
        crate::cmd::refuse_usage(&format!("attend sensor: {why}"), &why, json)
    })
}

/// Exit 3 when `h` is not a sensor attend can poll here.
fn require_hosted(h: &Hosted, verb: &str, json: bool) {
    let runnable = h.state.runs() || h.state == State::Off;
    let crate_kind = matches!(h.loaded.spec.as_ref().map(|s| s.kind), Some(Kind::Crate));
    if runnable && crate_kind {
        return;
    }
    let why = format!("{} is {}{}", h.name(), h.state.label(), h.state.detail().map(|d| format!(": {d}")).unwrap_or_default());
    if json {
        crate::cmd::print_json(&json!({ "sensor": h.name(), "state": h.state.label(), "detail": h.state.detail(), "error": why }));
    } else {
        eprintln!("attend sensor {verb}: {why}");
    }
    std::process::exit(agent_settings::exit::REJECTED);
}

pub(crate) fn poll(a: &PollArgs) {
    let (focus, cfg) = context();
    let h = find(sensors::catalog(&cfg, &focus.working_dir), &a.name, a.json);
    require_hosted(&h, "poll", a.json);
    let mut sensor = sensors::build(&h, &Ctx { cfg: &cfg, groups: None }, &focus.working_dir, false).unwrap_or_else(|why| {
        eprintln!("attend sensor poll: {}: {why}", h.name());
        std::process::exit(agent_settings::exit::REJECTED);
    });
    let result = sensor.poll(&focus);
    let failure = sensor.take_failure();
    let out = run_through(sensor, h.lane().unwrap_or(Lane::Event), result.clone(), &focus);
    let delivered = deliver(&h, &out, a.dry_run);
    if a.json {
        crate::cmd::print_json(&json!({
            "sensor": h.name(),
            "state": h.state.label(),
            "failure": failure,
            "result": result.iter().map(|(m, d)| json!({ "magnitude": m, "description": d })).collect::<Vec<_>>(),
            "triggers": [],
            "lane": out.lane.as_str(),
            "accumulated": out.accumulated,
            "threshold": out.threshold,
            "delivered": delivered,
            "dry_run": a.dry_run,
        }));
        return;
    }
    if let Some(f) = &failure {
        println!("{}: poll failed: {f}", h.name());
    }
    if result.is_empty() {
        println!("{}: no observations; a compiled sensor's first poll sets its baseline", h.name());
    }
    for (m, d) in &result {
        println!("  {m:>4.1}  {d}");
    }
    println!("triggers: none; a crate sensor's poll is compiled");
    verdict(&h, &out, a.dry_run);
}

pub(crate) fn fire(a: &FireArgs) {
    let (focus, cfg) = context();
    let (name, trigger) = match a.target.split_once('/') {
        Some((n, t)) => (n, Some(t)),
        None => (a.target.as_str(), None),
    };
    let h = find(sensors::catalog(&cfg, &focus.working_dir), name, a.json);
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
    let input = match &a.input {
        Some(src) => read_input(src).unwrap_or_else(|why| crate::cmd::refuse_usage(&format!("attend sensor fire: {why}"), &why, a.json)),
        None => vec![(spec.cadence.threshold, format!("test fire of {name}"))],
    };
    let input: Vec<(f64, String)> = input.into_iter().map(|(m, d)| (m, format!("[test] {}", sensors::deliverable(&d)))).collect();
    let sensor = sensors::build(&h, &Ctx { cfg: &cfg, groups: None }, &focus.working_dir, false).unwrap_or_else(|why| {
        eprintln!("attend sensor fire: {}: {why}", h.name());
        std::process::exit(agent_settings::exit::REJECTED);
    });
    let out = run_through(sensor, h.lane().unwrap_or(Lane::Event), input.clone(), &focus);
    let delivered = deliver(&h, &out, a.dry_run);
    let commit_note = a.commit.then_some("a crate sensor keeps no trigger state or state store, so --commit changes nothing");
    if a.json {
        crate::cmd::print_json(&json!({
            "sensor": h.name(),
            "trigger": trigger,
            "test": true,
            "input": input.iter().map(|(m, d)| json!({ "magnitude": m, "description": d })).collect::<Vec<_>>(),
            "lane": out.lane.as_str(),
            "accumulated": out.accumulated,
            "threshold": out.threshold,
            "delivered": delivered,
            "dry_run": a.dry_run,
            "note": commit_note,
        }));
        return;
    }
    if let Some(n) = commit_note {
        println!("{n}");
    }
    verdict(&h, &out, a.dry_run);
}

/// Emit what the slot let through, as `attend run` does, unless `dry_run`.
/// Returns the lines that went out, or would have.
fn deliver(h: &Hosted, out: &Outcome, dry_run: bool) -> Option<Vec<String>> {
    let lines = out.delivered.clone()?;
    if !dry_run {
        crate::emit::emit_batch(&[(h.name().to_string(), out.priority.to_string(), lines.clone())]);
    }
    Some(lines)
}

fn verdict(h: &Hosted, out: &Outcome, dry_run: bool) {
    let lane = format!("{} lane", out.lane.as_str());
    match (&out.delivered, dry_run) {
        (Some(_), true) => println!("{}: would deliver on the {lane} at priority {} (dry run)", h.name(), out.priority),
        (Some(_), false) if out.priority == "low" => println!("{}: delivered on the {lane} at priority low, which attend logs and does not wake on", h.name()),
        (Some(_), false) => println!("{}: delivered on the {lane} at priority {}", h.name(), out.priority),
        (None, _) => println!("{}: held on the {lane}: {:.1} accumulated of a {:.1} threshold", h.name(), out.accumulated, out.threshold),
    }
    if h.state == State::Off {
        println!("{}: off, so attend run does not poll it", h.name());
    }
}

/// A crate sensor's synthetic result: a JSON list of `{magnitude,
/// description}`, or `magnitude|description` lines.
fn read_input(src: &str) -> Result<Vec<(f64, String)>, String> {
    let mut text = String::new();
    match src {
        "-" => std::io::stdin().read_to_string(&mut text).map(|_| ()).map_err(|e| format!("cannot read stdin: {e}"))?,
        p => text = std::fs::read_to_string(p).map_err(|e| format!("cannot read {p}: {e}"))?,
    }
    parse_input(&text)
}

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
        assert_eq!(out.delivered, Some(vec!["x".to_string()]));
        let out = run_through(Box::new(Fixed(2.0)), Lane::Event, vec![(3.5, "y".into())], &focus);
        assert_eq!((out.delivered.is_some(), out.priority), (true, "medium"));
    }
}
