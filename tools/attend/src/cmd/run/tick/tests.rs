// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-FileCopyrightText: 2026 Jonathan Muse
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

use super::*;
use crate::sensors::Sensor as _;

#[test]
fn build_engagement_returns_action_potential_curve() {
    // Reachable without standing up a full cmd_run shell, which
    // is the testability claim from issue #78.
    let cfg = config::Config::default();
    let curve = build_engagement(&cfg);
    match curve {
        sensor_trait::Curve::ActionPotential { peak_multiplier, .. } => {
            assert!(peak_multiplier > 1.0);
        }
        _ => panic!("expected ActionPotential curve"),
    }
}

struct Marked(&'static str);
impl sensors::Sensor for Marked {
    fn name(&self) -> &str {
        self.0
    }
    fn poll(&mut self, _: &sensors::Focus) -> Vec<(f64, String)> {
        Vec::new()
    }
    fn emission_threshold(&self) -> f64 {
        1.0
    }
    fn base_interval(&self) -> Duration {
        Duration::from_secs(30)
    }
    fn min_interval(&self) -> Duration {
        Duration::from_secs(10)
    }
    fn decay_threshold(&self) -> u32 {
        3
    }
}

/// A file sensor named `ways-update` whose session trigger has fired and
/// had its line delivered, and the trigger.
fn fired_update_sensor() -> (sensors::hosted::FileSensor, sensor_spec::Trigger) {
    let doc = "kind: command\nargv: [x]\noutput: json\ntriggers: [{name: behind, path: \"$\", on: enters, where: {behind: true}, memory: session}]\n";
    let t = sensor_spec::parse("ways-update", &serde_yaml::from_str(doc).unwrap()).spec.unwrap().triggers.remove(0);
    let mut s = sensors::hosted::FileSensor::new("ways-update".into(), sensor_spec::DEFAULTS, Box::new(Marked("inner")), "/p", None, false);
    let behind = sensors::hosted::Seen::from([("$".to_string(), serde_json::json!(true))]);
    assert_eq!(s.observe(&t, behind), vec!["$".to_string()]);
    s.spend(&t);
    (s, t)
}

/// The checkpoint carries a file sensor's session memory under the
/// slot's name, as the sensor exports it.
#[test]
fn the_snapshot_carries_a_file_sensor_s_session_memory_under_its_name() {
    let (s, _) = fired_update_sensor();
    let exported: Vec<String> = s.export_state().into_iter().filter(|(k, _)| k == sensors::hosted::SESSION_MEMORY).map(|(_, v)| v).collect();
    assert_eq!(exported.len(), 1);
    let snapshot = collect_snapshot(&[sensors::slot(Box::new(s), sensor_spec::Lane::Event, None)]);
    assert_eq!(snapshot.sensor_memory.keys().collect::<Vec<_>>(), ["ways-update"]);
    assert_eq!(snapshot.sensor_memory["ways-update"], exported[0]);
    assert!(snapshot.sensor_memory["ways-update"].contains("\"spent\":[\""), "{}", snapshot.sensor_memory["ways-update"]);
}

/// Export through the checkpoint, restore by name as a restarted run
/// does, and the trigger that fired stays spent.
#[test]
fn a_restored_file_sensor_does_not_fire_its_session_trigger_again() {
    let (s, t) = fired_update_sensor();
    let snapshot = collect_snapshot(&[sensors::slot(Box::new(s), sensor_spec::Lane::Event, None)]);
    let state = restored_state("ways-update", None, &snapshot);
    assert_eq!(state.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), [sensors::hosted::SESSION_MEMORY]);
    assert!(restored_state("git", Some("git"), &snapshot).is_empty(), "another sensor gets none of it");
    let mut restarted = sensors::hosted::FileSensor::new("ways-update".into(), sensor_spec::DEFAULTS, Box::new(Marked("inner")), "/p", None, false);
    restarted.import_state(&state);
    let behind = || sensors::hosted::Seen::from([("$".to_string(), serde_json::json!(true))]);
    assert_eq!(restarted.observe(&t, behind()), Vec::<String>::new());
    let mut fresh = sensors::hosted::FileSensor::new("ways-update".into(), sensor_spec::DEFAULTS, Box::new(Marked("inner")), "/p", None, false);
    assert_eq!(fresh.observe(&t, behind()), vec!["$".to_string()], "without the restore it would fire again");
}

/// A source that reports the update behind on every poll.
struct Behind;
impl sensors::hosted::Source for Behind {
    fn fetch(&mut self) -> Result<sensors::hosted::Fetched, String> {
        Ok(sensors::hosted::Fetched::Json(serde_json::json!({"behind": true})))
    }
}

/// A line the slot's refractory drops leaves its session trigger
/// unspent, so the next poll past the refractory fires it again, and
/// only the line's delivery spends it.
#[test]
fn a_line_the_refractory_drops_leaves_its_session_trigger_to_fire_again() {
    let doc = "kind: command\nargv: [x]\noutput: json\nthreshold: 1\ntriggers: [{name: behind, path: \"$\", on: enters, where: {behind: true}, memory: session, magnitude: 3, description: behind}]\n";
    let spec = sensor_spec::parse("ways-update", &serde_yaml::from_str(doc).unwrap()).spec.unwrap();
    let sensor = sensors::hosted::FileSensor::declared(&spec, Box::new(Behind), "/p", None, false);
    let mut slot = sensors::slot(Box::new(sensor), sensor_spec::Lane::Event, None);
    let focus = Focus { description: String::new(), working_dir: "/p".into(), keywords: Vec::new() };
    slot.engagement.record_fire(sensor_trait::epoch_secs(), 1.0);
    assert!(slot.engagement.in_absolute_refractory(sensor_trait::epoch_secs()), "the slot starts in its absolute refractory");
    slot.poll(&focus);
    assert!(slot.accumulator.events.is_empty(), "the refractory dropped the line");
    assert!(!slot.sensor.take_spent());
    slot.engagement = sensor_trait::EngagementState::new(build_engagement(&config::Config::default()));
    slot.poll(&focus);
    assert_eq!(slot.accumulator.events, ["behind"], "past the refractory the trigger fires again");
    slot.sensor.delivered("behind");
    assert!(slot.sensor.take_spent(), "its delivery spends it");
    slot.accumulator.reset();
    slot.poll(&focus);
    assert!(slot.accumulator.events.is_empty(), "spent, it fires no more");
}

/// A sensor that reports one scripted poll at a time, and lets a line
/// with "finished" in it skip the refractory, as the processes sensor
/// does for a build finishing.
struct Builds(Vec<Vec<(f64, String)>>);
impl sensors::Sensor for Builds {
    fn name(&self) -> &str {
        "processes"
    }
    fn poll(&mut self, _: &sensors::Focus) -> Vec<(f64, String)> {
        if self.0.is_empty() { Vec::new() } else { self.0.remove(0) }
    }
    fn emission_threshold(&self) -> f64 {
        2.0
    }
    fn base_interval(&self) -> Duration {
        Duration::from_secs(30)
    }
    fn min_interval(&self) -> Duration {
        Duration::from_secs(5)
    }
    fn decay_threshold(&self) -> u32 {
        5
    }
    fn skips_refractory(&self, description: &str) -> bool {
        description.contains(" finished")
    }
}

/// A sink that writes every line and keeps what it was handed.
#[derive(Default)]
struct Written(Vec<String>);
impl Sink for Written {
    fn deliver(&mut self, batch: &[Disclosure]) -> Vec<Vec<Delivery>> {
        batch.iter().map(|d| d.lines.iter().map(|(l, _)| { self.0.push(l.clone()); Delivery::Written }).collect()).collect()
    }
}

/// Builds finishing one after another, within seconds, are each written:
/// a success, a failure, a second failure, then a success. None of them
/// starts a refractory that would drop the next.
#[test]
fn builds_finishing_back_to_back_are_each_written() {
    let results = ["cargo finished (success)", "cargo finished (failure, code 101)", "cargo finished (failure, code 101)", "cargo finished (success)"];
    let polls = results.iter().map(|r| vec![(3.0, r.to_string())]).collect();
    let mut slots = vec![sensors::slot(Box::new(Builds(polls)), sensor_spec::Lane::Event, None)];
    slots[0].engagement = sensor_trait::EngagementState::new(build_engagement(&config::Config::default()));
    let focus = Focus { description: String::new(), working_dir: "/p".into(), keywords: Vec::new() };
    let mut governor = DisclosureGovernor::new_permissive(Duration::ZERO, 100, Duration::from_secs(60));
    let mut sink = Written::default();
    for _ in results {
        slots[0].poll(&focus);
        assert!(slots[0].ready_to_disclose(), "{:?}", slots[0].accumulator.events);
        try_disclose(&mut slots, &[0], &mut governor, true, &mut sink);
        assert!(!slots[0].engagement.in_absolute_refractory(sensor_trait::epoch_secs()), "a build's line starts no refractory");
    }
    assert_eq!(sink.0, results);
}

/// While another line holds the sensor in its refractory, a build
/// finishing still gets through, and an app's start is still dropped.
#[test]
fn a_build_finishing_passes_a_refractory_another_line_started() {
    let polls = vec![vec![(2.0, "docker started".to_string()), (3.0, "make finished".to_string())]];
    let mut slot = sensors::slot(Box::new(Builds(polls)), sensor_spec::Lane::Event, None);
    slot.engagement = sensor_trait::EngagementState::new(build_engagement(&config::Config::default()));
    slot.engagement.record_fire(sensor_trait::epoch_secs(), 1.0);
    assert!(slot.engagement.in_absolute_refractory(sensor_trait::epoch_secs()));
    let focus = Focus { description: String::new(), working_dir: "/p".into(), keywords: Vec::new() };
    slot.poll(&focus);
    assert_eq!(slot.accumulator.events, ["make finished"]);
    assert!(slot.ready_to_disclose());
}

/// A crate sensor wrapped in its file passes on which lines skip the
/// refractory, as the host runs the processes sensor.
#[test]
fn a_file_sensor_passes_on_what_its_crate_sensor_lets_skip_the_refractory() {
    let s = sensors::hosted::FileSensor::new("processes".into(), sensor_spec::DEFAULTS, Box::new(Builds(Vec::new())), "/p", None, false);
    assert!(s.skips_refractory("cargo finished"));
    assert!(!s.skips_refractory("docker started"));
}

/// The event lane's governor may hold a build finishing behind a recent
/// disclosure. It holds the line, and writes it once it opens.
#[test]
fn a_build_finishing_the_governor_holds_is_written_once_it_opens() {
    let polls = vec![vec![(3.0, "cargo finished (success)".to_string())], vec![(3.0, "cargo finished (failure, code 101)".to_string())]];
    let mut slots = vec![sensors::slot(Box::new(Builds(polls)), sensor_spec::Lane::Event, None)];
    slots[0].engagement = sensor_trait::EngagementState::new(build_engagement(&config::Config::default()));
    let focus = Focus { description: String::new(), working_dir: "/p".into(), keywords: Vec::new() };
    let mut governor = DisclosureGovernor::new(Duration::from_secs(60), 3, Duration::from_secs(120));
    let mut sink = Written::default();
    slots[0].poll(&focus);
    try_disclose(&mut slots, &[0], &mut governor, true, &mut sink);
    assert_eq!(sink.0, ["cargo finished (success)"]);
    slots[0].poll(&focus);
    assert!(slots[0].ready_to_disclose());
    try_disclose(&mut slots, &[0], &mut governor, true, &mut sink);
    assert_eq!(sink.0.len(), 1, "the governor holds the second batch");
    assert_eq!(slots[0].accumulator.events, ["cargo finished (failure, code 101)"], "held, not dropped");
    governor.expire_cooldown();
    assert!(slots[0].ready_to_disclose());
    try_disclose(&mut slots, &[0], &mut governor, true, &mut sink);
    assert_eq!(sink.0, ["cargo finished (success)", "cargo finished (failure, code 101)"]);
}

#[test]
fn hash_file_is_stable_and_content_sensitive() {
    // The same-hash skip on the reload path (issue #140) depends on
    // hash_file being deterministic for identical bytes and different
    // for changed bytes.
    let dir = std::env::temp_dir().join(format!("attend-hash-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let a = dir.join("bin-a");
    let b = dir.join("bin-b");
    std::fs::write(&a, b"identical rebuild bytes").unwrap();
    std::fs::write(&b, b"identical rebuild bytes").unwrap();
    // Byte-identical files hash equal even as distinct paths — this is
    // the "mtime moved, content didn't" case we skip the reload on.
    assert_eq!(hash_file(&a), hash_file(&b));

    std::fs::write(&b, b"a genuinely changed binary").unwrap();
    assert_ne!(hash_file(&a), hash_file(&b));

    assert!(hash_file(&dir.join("does-not-exist")).is_none());
    std::fs::remove_dir_all(&dir).ok();
}
