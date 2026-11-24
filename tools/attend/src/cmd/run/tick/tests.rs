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
