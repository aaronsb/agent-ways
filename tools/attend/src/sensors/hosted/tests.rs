// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

use super::*;
use sensor_spec::bounds;
use serde_json::json;

/// A scratch store directory, removed when dropped.
struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("attend-hosted-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

struct Quiet;

impl Sensor for Quiet {
    fn name(&self) -> &str {
        "quiet"
    }
    fn poll(&mut self, _: &Focus) -> Vec<(f64, String)> {
        Vec::new()
    }
    fn emission_threshold(&self) -> f64 {
        1.0
    }
    fn base_interval(&self) -> Duration {
        Duration::from_secs(60)
    }
    fn min_interval(&self) -> Duration {
        Duration::from_secs(10)
    }
    fn decay_threshold(&self) -> u32 {
        3
    }
}

fn sensor(dir: &Path, session: Option<&str>) -> FileSensor {
    let key = Key::new("ways-update", "/p", session);
    let path = key.path_in(dir);
    FileSensor::open(key, sensor_spec::DEFAULTS, Poller::Crate(Box::new(Quiet)), Some(path), true)
}

/// The trigger a sensor file declares as `yaml`.
fn trig(yaml: &str) -> Trigger {
    let doc = format!("kind: command\nargv: [x]\noutput: json\nstate: {{n: {{type: number}}}}\ntriggers: [{yaml}]\n");
    let p = sensor_spec::parse("ways-update", &serde_yaml::from_str(&doc).unwrap());
    assert!(p.problems.is_empty(), "{yaml}: {:?}", p.problems);
    p.spec.unwrap().triggers.remove(0)
}

fn enters(name: &str, memory: &str) -> Trigger {
    trig(&format!("{{name: {name}, path: \"$\", on: enters, where: {{behind: true}}, memory: {memory}}}"))
}

fn seen(holds: bool) -> Seen {
    Seen::from([("$".to_string(), json!(holds))])
}

fn items(pairs: &[(&str, i64)]) -> Seen {
    pairs.iter().map(|(k, v)| (k.to_string(), json!(v))).collect()
}

/// What `trigger` fires on `now`, with its line delivered: a session
/// trigger that fires is spent, as its delivery spends it.
fn fire(s: &mut FileSensor, trigger: &Trigger, now: Seen) -> Vec<String> {
    let out = s.observe(trigger, now);
    if trigger.memory == Memory::Session && !out.is_empty() {
        s.spend(trigger);
    }
    out
}

/// Each poll's fired keys, in order, each line delivered, with store
/// memory saved after each poll; session memory reaches no store.
fn run(s: &mut FileSensor, t: &Trigger, polls: Vec<Seen>) -> Vec<Vec<String>> {
    polls
        .into_iter()
        .map(|now| {
            let out = fire(s, t, now);
            if t.memory == Memory::Store {
                s.commit().unwrap();
            }
            out
        })
        .collect()
}

fn fires(spec: &[&[&str]]) -> Vec<Vec<String>> {
    spec.iter().map(|keys| keys.iter().map(|k| k.to_string()).collect()).collect()
}

/// The FNV-1a name the store had when it was keyed by sensor and
/// project alone.
fn old_name(name: &str, project: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.bytes().chain([0]).chain(project.bytes()) {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}.json")
}

#[test]
fn a_store_is_named_by_sensor_project_and_session() {
    let d = Path::new("/s");
    let a = Key::new("git", "/p", Some("s1")).path_in(d);
    assert_eq!(a, Key::new("git", "/p", Some("s1")).path_in(d));
    assert_ne!(a, Key::new("git", "/p", Some("s2")).path_in(d), "each session has its own store");
    assert_ne!(a, Key::new("git", "/q", Some("s1")).path_in(d));
    assert_ne!(a, Key::new("gi", "t/p", Some("s1")).path_in(d), "the separator keeps the parts apart");
    assert_eq!(Key::new("git", "/p", Some("")), Key::new("git", "/p", None), "an empty id is no session");
    assert_ne!(a.file_name().unwrap().to_str().unwrap(), old_name("git", "/p"), "no key lands on a store of the old naming");
    let stem = a.file_stem().unwrap().to_str().unwrap();
    assert!(stem.len() == 16 && stem.chars().all(|c| c.is_ascii_hexdigit()), "{stem}");
}

#[test]
fn a_sensor_reads_a_store_only_for_a_resolved_session_id() {
    let open = |session, record| FileSensor::new("git".into(), sensor_spec::DEFAULTS, Box::new(Quiet), "/p", session, record).store;
    assert_eq!(open(Some("s1"), true), Some(Key::new("git", "/p", Some("s1")).path_in(&store_dir())));
    assert_eq!(open(None, true), None, "no session id, no store");
    assert_eq!(open(Some(""), true), None, "an empty id is no session");
    assert_eq!(open(Some("s1"), false), Some(Key::new("git", "/p", Some("s1")).path_in(&store_dir())), "a test poll reads the store");
}

#[test]
fn session_memory_fires_enters_on_the_first_poll_that_meets_where_and_not_again() {
    let d = Dir::new("session");
    let t = enters("behind", "session");
    let mut s = sensor(&d.0, Some("s1"));
    assert_eq!(run(&mut s, &t, vec![seen(true); 4]), fires(&[&["$"], &[], &[], &[]]));
    assert!(std::fs::read_dir(&d.0).unwrap().next().is_none(), "session memory writes no store");
    // A host of another session starts empty and fires once of its own.
    let mut other = sensor(&d.0, Some("s2"));
    assert_eq!(fire(&mut other, &t, seen(true)), vec!["$".to_string()]);
}

/// Session memory fires a trigger at most once in the session,
/// whatever it compares on: a condition that leaves and enters again,
/// a later item or a later change fires nothing more.
#[test]
fn session_memory_fires_at_most_once_even_when_the_condition_leaves_and_enters_again() {
    let d = Dir::new("once");
    let mut s = sensor(&d.0, Some("s1"));
    let polls = |hs: &[bool]| hs.iter().map(|h| seen(*h)).collect::<Vec<_>>();
    assert_eq!(run(&mut s, &enters("behind", "session"), polls(&[true, false, true, false, true])), fires(&[&["$"], &[], &[], &[], &[]]));
    let leaves = trig("{name: gone, path: \"$\", on: leaves, where: {behind: true}, memory: session}");
    assert_eq!(run(&mut s, &leaves, polls(&[true, false, true, false])), fires(&[&[], &["$"], &[], &[]]));
    let added = trig("{name: new, path: \"$.items[*]\", key: id, on: added, memory: session}");
    assert_eq!(run(&mut s, &added, vec![items(&[("a", 1)]), items(&[("a", 1), ("b", 1)]), items(&[]), items(&[("c", 1)])]), fires(&[&["a"], &[], &[], &[]]));
    // Store memory is not limited: it fires on each entry after its baseline.
    assert_eq!(run(&mut s, &enters("store", "store"), polls(&[false, true, false, true])), fires(&[&[], &["$"], &[], &["$"]]));
}

/// The once-per-session mark belongs to what the trigger tests. Editing
/// the condition after a fire gives a new definition, which fires once of
/// its own; renaming the trigger keeps the definition, which stays spent.
#[test]
fn after_a_fire_an_edit_fires_once_and_a_rename_does_not() {
    let d = Dir::new("spent-key");
    let mut s = sensor(&d.0, Some("s1"));
    assert_eq!(fire(&mut s, &enters("behind", "session"), seen(true)), vec!["$".to_string()]);
    let renamed = enters("outdated", "session");
    assert_eq!(run(&mut s, &renamed, vec![seen(true), seen(false), seen(true)]), fires(&[&[], &[], &[]]), "a rename is the same definition");
    let edited = trig("{name: behind, path: \"$\", on: enters, where: {behind: true, channel: release}, memory: session}");
    let polls = vec![Seen::from([("$".into(), json!(true))]), Seen::from([("$".into(), json!(false))]), Seen::from([("$".into(), json!(true))])];
    assert_eq!(run(&mut s, &edited, polls), fires(&[&["$"], &[], &[]]), "the edited condition fires once");
}

/// A session trigger offers its line once and fires nothing more while the
/// loop holds it. A dropped line releases it to fire on the next poll that
/// meets it, a condition that left and entered again included, and once its
/// line is handled it is spent and fires no more.
#[test]
fn a_session_trigger_is_not_offered_again_while_its_line_is_out() {
    let d = Dir::new("outstanding");
    let behind = || json!({"behind": true});
    let yaml = "triggers: [{name: late, path: \"$\", on: enters, where: {behind: true}, memory: session, description: \"behind\"}]\n";
    let mut s = declared(&d.0, yaml, Vec::new(), true);
    let line = vec![(1.0, "behind".to_string())];
    assert_eq!(s.evaluate(&behind()), line);
    for _ in 0..3 {
        assert_eq!(s.evaluate(&behind()), vec![], "held, it is not offered again");
    }
    s.dropped("behind");
    assert_eq!(s.evaluate(&behind()), line, "dropped, it fires again");
    s.dropped("behind");
    assert_eq!(s.evaluate(&json!({"behind": false})), vec![]);
    assert_eq!(s.evaluate(&behind()), line, "an entry after the drop fires");
    assert!(!s.take_spent());
    s.delivered("another line");
    assert!(!s.take_spent(), "another line spends nothing");
    s.delivered("behind");
    assert!(s.take_spent());
    for result in [behind(), json!({"behind": false}), behind()] {
        assert_eq!(s.evaluate(&result), vec![], "spent by its delivery");
    }
    assert!(s.export_state().iter().any(|(k, v)| k == SESSION_MEMORY && v.contains("\"spent\":[\"")), "{:?}", s.export_state());
}

/// A session trigger that offers several lines is spent only once all of
/// them are handled; one written and one dropped leaves it unspent, and it
/// offers its lines again.
#[test]
fn a_trigger_with_several_lines_is_spent_only_when_every_line_is_handled() {
    let d = Dir::new("several");
    let yaml = "triggers: [{name: new, path: \"$.items[*]\", key: id, on: added, memory: session, description: \"new {id}\"}]\n";
    let both = || json!({"items": [{"id": "a"}, {"id": "b"}]});
    let lines = || vec![(1.0, "new a".to_string()), (1.0, "new b".to_string())];
    let mut s = declared(&d.0, yaml, Vec::new(), true);
    assert_eq!(s.evaluate(&both()), lines());
    s.delivered("new a");
    assert!(!s.take_spent(), "one line of two handled");
    s.dropped("new b");
    assert!(!s.take_spent());
    assert_eq!(s.evaluate(&both()), lines(), "one line dropped: the trigger offers both again");
    s.delivered("new b");
    assert!(!s.take_spent(), "new a of this offer is still out");
    s.delivered("new a");
    assert!(s.take_spent(), "every line handled");
    assert_eq!(s.evaluate(&both()), vec![]);
}

/// The host checkpoints the moment a session trigger is spent: the sensor
/// says so once, after the fire that spent it, and not for a store
/// trigger's fire or a spent trigger's quiet polls.
#[test]
fn a_sensor_says_once_that_a_session_trigger_became_spent() {
    let d = Dir::new("take-spent");
    let mut s = sensor(&d.0, Some("s1"));
    assert!(!s.take_spent(), "nothing fired yet");
    assert_eq!(run(&mut s, &enters("store", "store"), vec![seen(false), seen(true)]), fires(&[&[], &["$"]]));
    assert!(!s.take_spent(), "a store trigger's fire spends nothing");
    assert_eq!(fire(&mut s, &enters("behind", "session"), seen(true)), vec!["$".to_string()]);
    assert!(s.take_spent());
    assert!(!s.take_spent(), "taken once");
    assert_eq!(fire(&mut s, &enters("behind", "session"), seen(true)), Vec::<String>::new());
    assert!(!s.take_spent(), "a spent trigger's later polls spend nothing new");
}

#[test]
fn session_memory_on_changed_fires_nothing_on_the_first_poll_then_once() {
    let d = Dir::new("changed");
    let mut s = sensor(&d.0, Some("s1"));
    let t = trig("{name: moved, path: \"$.items[*]\", key: id, on: changed, memory: session}");
    let polls = vec![items(&[("a", 1)]), items(&[("a", 2)]), items(&[("a", 3)]), items(&[("a", 4), ("b", 1)])];
    assert_eq!(run(&mut s, &t, polls), fires(&[&[], &["a"], &[], &[]]));
}

#[test]
fn session_memory_on_removed_fires_once() {
    let d = Dir::new("removed");
    let mut s = sensor(&d.0, Some("s1"));
    let t = trig("{name: gone, path: \"$.items[*]\", key: id, on: removed, memory: session}");
    let polls = vec![items(&[("a", 1), ("b", 1), ("c", 1)]), items(&[("b", 1), ("c", 1)]), items(&[("c", 1)]), items(&[])];
    assert_eq!(run(&mut s, &t, polls), fires(&[&[], &["a"], &[], &[]]));
}

/// A re-executed run reads the session memory back from the
/// checkpoint, so a trigger that fired before the restart stays spent,
/// and one that had not fired keeps what it saw.
#[test]
fn session_memory_survives_an_export_and_import_so_the_trigger_does_not_fire_twice() {
    let d = Dir::new("export");
    let behind = enters("behind", "session");
    let later = trig("{name: later, path: \"$\", on: enters, where: {ahead: true}, memory: session}");
    let mut s = sensor(&d.0, Some("s1"));
    assert_eq!(fire(&mut s, &behind, seen(true)), vec!["$".to_string()]);
    assert_eq!(fire(&mut s, &later, seen(false)), Vec::<String>::new());
    let state = s.export_state();
    assert_eq!(state.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), [SESSION_MEMORY]);
    let mut again = sensor(&d.0, Some("s1"));
    again.import_state(&state);
    assert_eq!(fire(&mut again, &behind, seen(true)), Vec::<String>::new(), "spent before the restart");
    assert_eq!(fire(&mut again, &behind, seen(false)), Vec::<String>::new());
    assert_eq!(fire(&mut again, &behind, seen(true)), Vec::<String>::new());
    assert_eq!(fire(&mut again, &later, seen(true)), vec!["$".to_string()], "it saw false before the restart, so true is an entry");
    assert_eq!(again.export_state(), {
        let mut fresh = sensor(&d.0, Some("s1"));
        fresh.import_state(&again.export_state());
        fresh.export_state()
    });
    assert!(sensor(&d.0, Some("s1")).export_state().is_empty(), "a sensor with no session memory exports nothing of it");
}

#[test]
fn store_memory_takes_a_fresh_store_s_first_poll_as_its_baseline() {
    let d = Dir::new("store");
    let t = enters("behind", "store");
    let mut s = sensor(&d.0, Some("s1"));
    let polls = [true, true, false, true].map(seen).to_vec();
    assert_eq!(run(&mut s, &t, polls), fires(&[&[], &[], &[], &["$"]]), "the condition held at the baseline, so only the later entry fires");
    // A restart in the same session reads the memory back and fires nothing new.
    let mut again = sensor(&d.0, Some("s1"));
    assert_eq!(fire(&mut again, &t, seen(true)), Vec::<String>::new());
    // Another session's store is its own, so it starts from a baseline.
    let mut other = sensor(&d.0, Some("s2"));
    assert_eq!(run(&mut other, &t, vec![seen(true), seen(true)]), fires(&[&[], &[]]));
}

/// Memory kept under an earlier definition of a trigger with the same
/// name reads as none, so the edited trigger starts from a baseline.
#[test]
fn an_edited_trigger_that_keeps_its_name_starts_from_a_fresh_baseline() {
    let d = Dir::new("edited");
    let before = enters("behind", "store");
    let after = trig("{name: behind, path: \"$\", on: enters, where: {behind: false}, memory: store}");
    assert_ne!(before.fingerprint(), after.fingerprint());
    let mut s = sensor(&d.0, Some("s1"));
    assert_eq!(run(&mut s, &before, vec![seen(true), seen(true)]), fires(&[&[], &[]]));
    // `after` holds when behind is false. Under `before`'s memory a false
    // would read as an entry; under its own it is a baseline.
    let mut reopened = sensor(&d.0, Some("s1"));
    let polls = vec![Seen::from([("$".into(), json!(true))]), Seen::from([("$".into(), json!(false))]), Seen::from([("$".into(), json!(true))])];
    assert_eq!(run(&mut reopened, &after, polls), fires(&[&[], &[], &["$"]]), "a baseline first, then the edited trigger's own entry");
    let rec = read_at(&Key::new("ways-update", "/p", Some("s1")).path_in(&d.0), &Key::new("ways-update", "/p", Some("s1"))).unwrap();
    assert_eq!(rec.memory["behind"].test, after.fingerprint(), "the store keeps the definition it saw under");
}

/// A sensor of kind command named `units` whose file is `yaml`, its polls
/// returning `results` in turn, its store under `dir` for session `s1`.
fn declared(dir: &Path, yaml: &str, results: Vec<Value>, record: bool) -> FileSensor {
    declared_from(dir, yaml, results.into_iter().map(Fetched::Json).collect(), record)
}

/// [`declared`] with each poll's result as the source fetches it.
fn declared_from(dir: &Path, yaml: &str, results: Vec<Fetched>, record: bool) -> FileSensor {
    let doc = format!("kind: command\nargv: [x]\noutput: json\n{yaml}");
    let p = sensor_spec::parse("units", &serde_yaml::from_str(&doc).unwrap());
    assert!(p.problems.is_empty(), "{yaml}: {:?}", p.problems);
    let spec = p.spec.unwrap();
    let key = Key::new("units", "/p", Some("s1"));
    let path = key.path_in(dir);
    FileSensor::open(key, spec.cadence, Poller::Spec(Box::new(Declared::of(&spec, Box::new(Canned(results))))), Some(path), record)
}

/// A source that returns prepared results in turn, then fails.
struct Canned(Vec<Fetched>);

impl Source for Canned {
    fn fetch(&mut self) -> Result<Fetched, String> {
        match self.0.is_empty() {
            true => Err("no more results".into()),
            false => Ok(self.0.remove(0)),
        }
    }
}

/// Each poll's observations, in order.
fn polls(s: &mut FileSensor, n: usize) -> Vec<Vec<(f64, String)>> {
    let focus = Focus::default_focus();
    (0..n).map(|_| s.poll(&focus)).collect()
}

const UNITS: &str = "state: {failed: {type: number}, first: {type: string, from: \"$.units[0].unit\"}}\ntriggers:\n  - {name: unit-failed, path: \"$.units[*]\", key: unit, on: added, where: {unit: \"*.service\"}, magnitude: 2, description: \"unit {unit} failed ({state.failed} in all, was {prev.failed}; {trigger.count} new)\", expires: 120}\n  - {name: all-clear, state: {failed: falls}, description: \"down to {state.failed} from {prev.failed}\", expires: session}\n";

fn units(names: &[&str]) -> Value {
    json!({"units": names.iter().map(|n| json!({"unit": n})).collect::<Vec<_>>(), "state": {"failed": names.len()}})
}

/// A command's result runs through the declared state and each trigger:
/// the first poll sets the baseline, a unit added fires the item trigger
/// with its description filled, and a count that falls fires the state
/// trigger. Each observation carries its trigger's expiry.
#[test]
fn a_result_runs_through_the_state_and_every_trigger() {
    let d = Dir::new("declared");
    let mut s = declared(&d.0, UNITS, vec![units(&["a.service"]), units(&["a.service", "b.service", "c.timer"]), units(&["b.service"])], true);
    let out = polls(&mut s, 3);
    assert_eq!(out[0], vec![], "the first poll of a fresh store is the baseline");
    assert_eq!(out[1], vec![(2.0, "unit b.service failed (3 in all, was 1; 1 new)".to_string())], "c.timer does not meet where");
    assert_eq!(out[2], vec![(1.0, "down to 1 from 3".to_string())]);
    assert_eq!(s.verdicts(), [Verdict { trigger: "unit-failed".into(), fired: vec![] }, Verdict { trigger: "all-clear".into(), fired: vec!["$".into()] }]);
    assert_eq!(s.lapse("down to 1 from 3"), Some(sensor_trait::Lapse::UntilRead));
    assert_eq!(serde_json::to_string(s.state().unwrap()).unwrap(), r#"{"failed":1,"first":"b.service"}"#);
    let mut again = declared(&d.0, UNITS, vec![units(&["b.service", "d.service"])], true);
    assert_eq!(polls(&mut again, 1)[0], vec![(2.0, "unit d.service failed (2 in all, was 1; 1 new)".to_string())], "the store carried state and memory");
    assert_eq!(again.lapse("unit d.service failed (2 in all, was 1; 1 new)"), Some(sensor_trait::Lapse::After(120)));
}

/// `changed`, `rises` and `falls` each fire on their own change, combined
/// by `match`, and only against a value seen before.
#[test]
fn state_triggers_fire_on_their_change_with_store_memory() {
    let d = Dir::new("state-triggers");
    let yaml = "state: {n: {type: number, from: \"$.n\"}, s: {type: string, from: \"$.s\"}}\ntriggers:\n  - {name: up, state: {n: rises}}\n  - {name: down, state: {n: falls}}\n  - {name: moved, state: {s: changed}}\n  - {name: both, state: {n: rises, s: changed}}\n  - {name: either, state: {n: rises, s: changed}, match: any}\n";
    let results = vec![json!({"n": 1, "s": "a"}), json!({"n": 2, "s": "a"}), json!({"n": 1, "s": "b"}), json!({"n": 5, "s": "c"})];
    let mut s = declared(&d.0, yaml, results, true);
    let fired = |s: &FileSensor| s.verdicts().iter().filter(|v| !v.fired.is_empty()).map(|v| v.trigger.clone()).collect::<Vec<_>>();
    let mut each = Vec::new();
    for _ in 0..4 {
        polls(&mut s, 1);
        each.push(fired(&s));
    }
    assert_eq!(each, [vec![], vec!["up", "either"], vec!["down", "moved", "either"], vec!["up", "moved", "both", "either"]]);
}

/// A write a check refuses keeps the old state, so the state triggers see
/// no change, and the refusal is the poll's failure. The item triggers
/// still run.
#[test]
fn a_refused_state_write_is_the_poll_s_failure_and_changes_nothing() {
    let d = Dir::new("refused");
    let mut s = declared(&d.0, UNITS, vec![units(&["a.service"]), json!({"units": [{"unit": "a.service"}, {"unit": "b.service"}], "state": {"failed": "two"}})], true);
    let out = polls(&mut s, 2);
    assert_eq!(out[1], vec![(2.0, "unit b.service failed (1 in all, was 1; 1 new)".to_string())]);
    let key = Key::new("units", "/p", Some("s1"));
    let poll = read_at(&key.path_in(&d.0), &key).unwrap().poll.unwrap();
    assert_eq!((poll.ok, poll.detail.as_deref()), (false, Some("the state write was refused: state field failed: \"two\" is not a number")));
    assert_eq!(s.state().unwrap()["failed"], json!(1));
}

/// A test poll reads the store and compares against what it holds, and
/// writes nothing: the file is byte for byte what the recorded poll left.
#[test]
fn a_poll_that_does_not_record_compares_against_the_store_and_leaves_it_unchanged() {
    let d = Dir::new("read-only");
    polls(&mut declared(&d.0, UNITS, vec![units(&["a.service"])], true), 1);
    let path = Key::new("units", "/p", Some("s1")).path_in(&d.0);
    let before = std::fs::read(&path).unwrap();
    let mut test = declared(&d.0, UNITS, vec![units(&["a.service", "b.service"])], false);
    assert_eq!(polls(&mut test, 1)[0], vec![(2.0, "unit b.service failed (2 in all, was 1; 1 new)".to_string())], "not a baseline");
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

/// A stored state that another declaration wrote is discarded with the
/// state triggers' memory, and the discard is said once.
#[test]
fn a_changed_declaration_discards_the_stored_state() {
    let d = Dir::new("discard");
    polls(&mut declared(&d.0, UNITS, vec![units(&["a.service"]), units(&["a.service"])], true), 2);
    let changed = UNITS.replace("failed: {type: number}", "failed: {type: number}, extra: {type: boolean}");
    let mut s = declared(&d.0, &changed, vec![units(&[]), units(&[])], true);
    assert_eq!(s.state(), None);
    let out = polls(&mut s, 1);
    assert_eq!(out[0], vec![], "the fall from 1 to 0 is a baseline, not a change");
    let key = Key::new("units", "/p", Some("s1"));
    let detail = read_at(&key.path_in(&d.0), &key).unwrap().poll.unwrap().detail;
    assert_eq!(detail.as_deref(), Some("its stored state was discarded, so it starts from a baseline: the state declaration changed"));
    polls(&mut s, 1);
    assert_eq!(read_at(&key.path_in(&d.0), &key).unwrap().poll.unwrap().detail, None);
}

/// A failed fetch is the poll's failure.
#[test]
fn a_failed_fetch_records_its_reason() {
    let d = Dir::new("fetch-failed");
    let mut s = declared(&d.0, UNITS, vec![], true);
    assert_eq!(polls(&mut s, 1)[0], vec![]);
    assert_eq!(s.take_failure().as_deref(), Some("no more results"));
}

/// `output: lines` passes its observations through, one line each with
/// frame tags defused, and runs no trigger.
#[test]
fn lines_pass_through_with_no_triggers() {
    let d = Dir::new("lines");
    let mut s = declared_from(&d.0, UNITS, vec![Fetched::Lines(vec![(2.5, "disk\nfull <attend x>".into())])], true);
    assert_eq!(polls(&mut s, 1)[0], vec![(2.5, "disk full \u{2039}attend x>".to_string())]);
    assert_eq!(s.verdicts(), []);
}

/// A committed evaluation writes the declared state, even with no
/// trigger to observe, and an evaluation alone writes nothing.
#[test]
fn a_commit_writes_the_state_of_a_sensor_with_no_triggers() {
    let d = Dir::new("commit-state");
    let yaml = "state: {n: {type: number, from: \"$.n\"}}\n";
    let mut s = declared(&d.0, yaml, vec![], true);
    s.evaluate(&json!({"n": 4}));
    let key = Key::new("units", "/p", Some("s1"));
    assert_eq!(read_at(&key.path_in(&d.0), &key), None, "evaluate writes nothing");
    s.commit().unwrap();
    let reopened = declared(&d.0, yaml, vec![], true);
    assert_eq!(serde_json::to_string(reopened.state().unwrap()).unwrap(), r#"{"n":4}"#);
}

/// A recording sensor reads its store again when another writer changed
/// it, so a committed fire is not overwritten by the next poll.
#[test]
fn a_recording_sensor_keeps_what_a_commit_wrote() {
    let d = Dir::new("commit-reread");
    let mut host = declared(&d.0, UNITS, vec![units(&["a.service"]), units(&["a.service", "b.service"])], true);
    polls(&mut host, 1);
    let mut fire = declared(&d.0, UNITS, vec![], true);
    assert_eq!(fire.evaluate(&units(&["a.service", "b.service"])), vec![(2.0, "unit b.service failed (2 in all, was 1; 1 new)".to_string())]);
    fire.commit().unwrap();
    assert_eq!(polls(&mut host, 1)[0], vec![], "b.service is in the committed memory");
    let key = Key::new("units", "/p", Some("s1"));
    let rec = read_at(&key.path_in(&d.0), &key).unwrap();
    assert_eq!(rec.memory["unit-failed"].seen.keys().collect::<Vec<_>>(), ["a.service", "b.service"]);
    assert_eq!(rec.state.unwrap().fields["failed"], json!(2));
}

/// A commit that leaves the store's mtime where the host last saw it, as
/// a filesystem with coarse timestamps can, is still read: the record's
/// nonce tells the writes apart.
#[test]
fn a_commit_with_the_host_s_mtime_is_still_read() {
    let d = Dir::new("commit-mtime");
    let mut host = declared(&d.0, UNITS, vec![units(&["a.service"]), units(&["a.service", "b.service"])], true);
    polls(&mut host, 1);
    let mut fire = declared(&d.0, UNITS, vec![], true);
    fire.evaluate(&units(&["a.service", "b.service"]));
    fire.commit().unwrap();
    let path = Key::new("units", "/p", Some("s1")).path_in(&d.0);
    std::fs::File::options().write(true).open(&path).unwrap().set_modified(host.written.unwrap()).unwrap();
    assert_eq!(modified(&path), host.written);
    assert_eq!(polls(&mut host, 1)[0], vec![], "b.service is in the committed memory");
}

/// A poll that saves between a fire's evaluation and its commit makes the
/// commit refuse, so the poll's write stands.
#[test]
fn a_commit_refuses_when_the_store_changed_during_the_fire() {
    let d = Dir::new("commit-raced");
    let mut host = declared(&d.0, UNITS, vec![units(&["a.service"]), units(&["a.service", "c.service"])], true);
    polls(&mut host, 1);
    let mut fire = declared(&d.0, UNITS, vec![], true);
    fire.evaluate(&units(&["a.service", "b.service"]));
    polls(&mut host, 1);
    let key = Key::new("units", "/p", Some("s1"));
    let before = std::fs::read(key.path_in(&d.0)).unwrap();
    assert_eq!(fire.commit(), Err("the store changed during the fire; fire again".to_string()));
    assert_eq!(std::fs::read(key.path_in(&d.0)).unwrap(), before);
}

/// A refused first write sets no baseline for the state triggers, so the
/// first accepted write is the baseline, not a change from zero.
#[test]
fn a_refused_write_creates_no_state_trigger_memory() {
    let d = Dir::new("refused-first");
    let yaml = "state: {unread: {type: number, from: \"$.unread\"}, text: {type: string, from: \"$.text\"}}\ntriggers:\n  - {name: more, state: {unread: changed}, description: \"{state.unread} unread\"}\n";
    let results = vec![json!({"unread": 5, "text": "a\nb"}), json!({"unread": 5, "text": "hello"}), json!({"unread": 6, "text": "hi"})];
    let mut s = declared(&d.0, yaml, results, true);
    let out = polls(&mut s, 3);
    assert_eq!(out, [vec![], vec![], vec![(1.0, "6 unread".to_string())]]);
}

/// `changed` on a state field compares numbers by value.
#[test]
fn changed_compares_numbers_by_value() {
    let d = Dir::new("changed-value");
    let yaml = "state: {n: {type: number, from: \"$.n\"}}\ntriggers:\n  - {name: moved, state: {n: changed}}\n";
    let mut s = declared(&d.0, yaml, vec![json!({"n": 1}), json!({"n": 1.0}), json!({"n": 2})], true);
    assert_eq!(polls(&mut s, 3), [vec![], vec![], vec![(1.0, "units moved".to_string())]]);
}

/// A `leaves` trigger whose item is gone from the result fills its
/// description from the item's key.
#[test]
fn leaves_on_a_vanished_item_names_it_by_its_key() {
    let d = Dir::new("leaves-gone");
    let yaml = "triggers:\n  - {name: gone, path: \"$.units[*]\", key: unit, on: leaves, where: {active: true}, description: \"unit {unit} left\"}\n";
    let mut s = declared(&d.0, yaml, vec![json!({"units": [{"unit": "a", "active": true}]}), json!({"units": []})], true);
    assert_eq!(polls(&mut s, 2), [vec![], vec![(1.0, "unit a left".to_string())]]);
}

/// A poll writes the store whole, with the trigger memory an earlier
/// observation left, so a reopened sensor has both.
#[test]
fn a_poll_s_save_keeps_the_trigger_memory() {
    let d = Dir::new("poll");
    let t = enters("behind", "store");
    let focus = Focus::default_focus();
    let mut s = sensor(&d.0, Some("s1"));
    s.poll(&focus);
    assert_eq!(fire(&mut s, &t, seen(false)), Vec::<String>::new());
    s.poll(&focus);
    let key = Key::new("ways-update", "/p", Some("s1"));
    let rec = read_at(&key.path_in(&d.0), &key).unwrap();
    assert_eq!((rec.poll.as_ref().map(|p| (p.ok, p.observations)), rec.memory["behind"].seen.clone()), (Some((true, 0)), seen(false)));
    let mut reopened = sensor(&d.0, Some("s1"));
    assert_eq!(fire(&mut reopened, &t, seen(true)), vec!["$".to_string()], "it saw false before, so true is an entry and not a baseline");
}

#[test]
fn each_comparison_fires_on_its_change() {
    let prev = Seen::from([("a".into(), json!(true)), ("b".into(), json!(1)), ("c".into(), json!(false))]);
    let now = Seen::from([("a".into(), json!(false)), ("b".into(), json!(2)), ("c".into(), json!(true)), ("d".into(), json!(true))]);
    let at = |on| fired(on, Memory::Store, Some(&prev), &now);
    assert_eq!(at(On::Added), ["d"]);
    assert_eq!(fired(On::Removed, Memory::Store, Some(&now), &prev), ["d"]);
    assert_eq!(at(On::Changed), ["a", "b", "c"]);
    assert_eq!(at(On::Enters), ["c", "d"]);
    assert_eq!(at(On::Leaves), ["a"]);
    assert_eq!(fired(On::Added, Memory::Store, None, &now), Vec::<String>::new(), "store memory with nothing seen sets a baseline");
    assert_eq!(fired(On::Added, Memory::Session, None, &now), ["a", "b", "c", "d"], "session memory compares against nothing seen");
    assert_eq!(fired(On::Leaves, Memory::Session, None, &now), Vec::<String>::new());
}

#[test]
fn a_store_of_the_old_format_or_of_another_key_reads_as_no_store() {
    let d = Dir::new("old");
    let key = Key::new("ways-update", "/p", Some("s1"));
    let path = key.path_in(&d.0);
    // A file written before stores were keyed by session: no format and no session.
    std::fs::write(&path, r#"{"sensor":"ways-update","project":"/p","poll":{"at":1,"ok":true,"observations":0}}"#).unwrap();
    assert_eq!(read_at(&path, &key), None);
    let mut s = sensor(&d.0, Some("s1"));
    assert_eq!(fire(&mut s, &enters("behind", "store"), seen(true)), Vec::<String>::new(), "it starts from a baseline");
    s.commit().unwrap();
    // A current file whose key is not the one asked for is not taken as its store.
    let other = Key::new("ways-update", "/p", Some("s2"));
    assert_eq!(read_at(&path, &other), None);
    assert_eq!(read_at(&path, &key).map(|r| (r.store, r.session)), Some((STORE_FORMAT, Some("s1".to_string()))));
}

/// The sweep removes stores past the retention limit and files that are
/// not stores of this format, and keeps current stores and the files a
/// writer has in flight.
#[test]
fn the_sweep_removes_expired_and_foreign_stores() {
    let d = Dir::new("sweep");
    let put = |session: &str| {
        let key = Key::new("git", "/p", Some(session));
        assert!(write(&key.path_in(&d.0), &Record::new(&key, None, BTreeMap::new())));
        key.path_in(&d.0)
    };
    let fresh = put("fresh");
    let expired = put("expired");
    let month = Duration::from_secs((bounds::RETENTION_DAYS + 1) * 86_400);
    std::fs::File::options().write(true).open(&expired).unwrap().set_modified(SystemTime::now() - month).unwrap();
    let old = d.0.join(old_name("git", "/p"));
    std::fs::write(&old, r#"{"sensor":"git","project":"/p","poll":{"at":1,"ok":true}}"#).unwrap();
    let garbage = d.0.join("0000000000000000.json");
    std::fs::write(&garbage, "not json").unwrap();
    let in_flight = d.0.join("0000000000000001.json.4242");
    std::fs::write(&in_flight, "partial").unwrap();
    let older = d.0.join("0000000000000002.json");
    std::fs::write(&older, r#"{"store":1,"sensor":"git","project":"/p"}"#).unwrap();
    // A newer attend's store, as a dev build beside the release writes.
    let newer = d.0.join("0000000000000003.json");
    std::fs::write(&newer, r#"{"store":3,"sensor":"git","project":"/p","shape":"unknown here"}"#).unwrap();
    // A file the sweep cannot read is skipped, not judged.
    let unreadable = d.0.join("0000000000000004.json");
    std::fs::write(&unreadable, "not json either").unwrap();
    std::fs::set_permissions(&unreadable, std::os::unix::fs::PermissionsExt::from_mode(0o000)).unwrap();
    let readable_anyway = std::fs::read(&unreadable).is_ok();
    assert_eq!(sweep_in(&d.0, SystemTime::now()), if readable_anyway { 5 } else { 4 });
    let mut left: Vec<PathBuf> = std::fs::read_dir(&d.0).unwrap().flatten().map(|e| e.path()).collect();
    left.sort();
    let mut want = vec![fresh, in_flight, newer];
    if !readable_anyway {
        want.push(unreadable.clone());
    }
    want.sort();
    assert_eq!(left, want, "expired, per-project, garbage and format-1 files go; format 2 and 3, in-flight and unreadable files stay");
    assert_eq!(sweep_in(&d.0, SystemTime::now()), 0, "a second sweep finds nothing");
    std::fs::set_permissions(&unreadable, std::os::unix::fs::PermissionsExt::from_mode(0o600)).ok();
}

#[test]
fn a_session_s_stores_move_with_it_and_its_memory_comes_too() {
    let d = Dir::new("move");
    let t = enters("behind", "store");
    let mut s = sensor(&d.0, Some("old"));
    fire(&mut s, &t, seen(false));
    s.commit().unwrap();
    assert_eq!(move_stores_in(&d.0, "old", "old"), 0, "a move to the same id does nothing");
    assert!(read_at(&Key::new("ways-update", "/p", Some("old")).path_in(&d.0), &Key::new("ways-update", "/p", Some("old"))).is_some(), "and keeps the store");
    assert_eq!(move_stores_in(&d.0, "old", "new"), 1);
    assert!(read_at(&Key::new("ways-update", "/p", Some("old")).path_in(&d.0), &Key::new("ways-update", "/p", Some("old"))).is_none());
    let mut moved = sensor(&d.0, Some("new"));
    assert_eq!(fire(&mut moved, &t, seen(true)), vec!["$".to_string()], "the memory moved, so this is an entry and not a baseline");
    assert_eq!(move_stores_in(&d.0, "old", "new"), 0, "a second move finds nothing");
}

/// A store whose copy cannot be written stays where it is.
#[test]
fn a_move_that_cannot_write_keeps_the_source() {
    let d = Dir::new("move-fail");
    let from = Key::new("ways-update", "/p", Some("old"));
    let mut s = sensor(&d.0, Some("old"));
    fire(&mut s, &enters("behind", "store"), seen(false));
    s.commit().unwrap();
    // A directory at the target's path makes the rename fail.
    std::fs::create_dir_all(Key::new("ways-update", "/p", Some("new")).path_in(&d.0)).unwrap();
    assert_eq!(move_stores_in(&d.0, "old", "new"), 0);
    assert!(read_at(&from.path_in(&d.0), &from).is_some(), "the source is kept");
}

#[test]
fn the_last_poll_is_the_session_s_own_or_with_no_session_the_newest_in_the_project() {
    let d = Dir::new("last");
    let put = |sensor: &str, project: &str, session: &str, at: u64| {
        let key = Key::new(sensor, project, Some(session));
        let rec = Record::new(&key, Some(Poll { at, ok: true, detail: None, observations: 0 }), BTreeMap::new());
        assert!(write(&key.path_in(&d.0), &rec));
    };
    put("git", "/p", "s1", 10);
    put("git", "/p", "s2", 20);
    put("git", "/q", "s3", 30);
    put("processes", "/p", "s1", 5);
    std::fs::write(d.0.join(old_name("git", "/p")), r#"{"sensor":"git","project":"/p","poll":{"at":99,"ok":true}}"#).unwrap();
    let at = |session| last_polls_in(&d.0, "/p", session).into_iter().map(|(n, p)| (n, p.at)).collect::<Vec<_>>();
    assert_eq!(at(Some("s2")), [("git".to_string(), 20)]);
    assert_eq!(at(None), [("git".to_string(), 20), ("processes".to_string(), 5)], "the old-format store at 99 is not read");
}

/// The probe's comparison: an item trigger needs every item of the first
/// result again, by its key; a state trigger the same value in each field
/// it watches; a sensor with no triggers the same result.
#[test]
fn a_second_result_keeps_the_first_when_each_trigger_sees_what_it_saw() {
    let spec = |yaml: &str| {
        let doc = format!("kind: mcp\nserver: s\ntool: t\n{yaml}");
        let p = sensor_spec::parse("probe", &serde_yaml::from_str(&doc).unwrap());
        assert!(p.problems.is_empty(), "{yaml}: {:?}", p.problems);
        p.spec.unwrap()
    };
    let items = spec("triggers:\n  - {name: new, path: \"$.items[*]\", key: id, on: added}\n");
    let r = |ids: &[&str]| json!({ "items": ids.iter().map(|i| json!({ "id": i })).collect::<Vec<_>>() });
    assert_eq!(kept(&items, &r(&["a", "b"]), &r(&["a", "b", "c"])), Ok(()), "a new item is not a loss");
    assert_eq!(kept(&items, &r(&["a", "b"]), &r(&["b"])), Err("trigger new: the second call did not return the item a".into()));
    let state = spec("state:\n  unread: {type: number, from: \"$.unread\"}\ntriggers:\n  - {name: more, state: {unread: rises}}\n");
    let n = |u: u32| json!({ "unread": u, "noise": u * 7 });
    assert_eq!(kept(&state, &n(3), &n(3)), Ok(()));
    assert_eq!(kept(&state, &n(3), &n(0)), Err("trigger more: the second call left unread at another value".into()));
    let none = spec("");
    assert_eq!(kept(&none, &n(1), &n(1)), Ok(()));
    assert_eq!(kept(&none, &n(1), &n(2)), Err("the second call returned another result".into()));
}

/// A first result with nothing in it proves nothing, and an `enters`
/// trigger needs each item to meet `where` as it did.
#[test]
fn a_probe_refuses_an_empty_first_result_and_a_changed_where() {
    let spec = |yaml: &str| sensor_spec::parse("probe", &serde_yaml::from_str(&format!("kind: mcp\nserver: s\ntool: t\n{yaml}")).unwrap()).spec.unwrap();
    let nothing = Err("nothing to compare; probe while the tool returns items".to_string());
    let items = spec("triggers:\n  - {name: new, path: \"$.items[*]\", key: id, on: added}\n");
    assert_eq!(kept(&items, &json!({"items": []}), &json!({"items": []})), nothing);
    assert_eq!(kept(&spec(""), &json!({}), &json!({})), nothing);
    let unread = spec("triggers:\n  - {name: unread, path: \"$.items[*]\", key: id, on: enters, where: {unread: true}}\n");
    let r = |u: bool| json!({"items": [{"id": "a", "unread": u}]});
    assert_eq!(kept(&unread, &r(true), &r(true)), Ok(()));
    assert_eq!(kept(&unread, &r(true), &r(false)), Err("trigger unread: the item a meets where differently in the second call".into()), "a read that marks the item read");
}
