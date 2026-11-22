// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

use super::*;
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
    FileSensor::open(key, sensor_spec::DEFAULTS, Box::new(Quiet), Some(path))
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

/// Each poll's fired keys, in order.
fn run(s: &mut FileSensor, t: &Trigger, polls: Vec<Seen>) -> Vec<Vec<String>> {
    polls.into_iter().map(|now| s.observe(t, now).unwrap()).collect()
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
fn a_sensor_keeps_a_store_only_for_a_resolved_session_id() {
    let open = |session, record| FileSensor::new("git".into(), sensor_spec::DEFAULTS, Box::new(Quiet), "/p", session, record).store;
    assert_eq!(open(Some("s1"), true), Some(Key::new("git", "/p", Some("s1")).path_in(&store_dir())));
    assert_eq!(open(None, true), None, "no session id, no store");
    assert_eq!(open(Some(""), true), None, "an empty id is no session");
    assert_eq!(open(Some("s1"), false), None, "a test poll keeps none");
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
    assert_eq!(other.observe(&t, seen(true)), Some(vec!["$".to_string()]));
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
    let later = enters("later", "session");
    let mut s = sensor(&d.0, Some("s1"));
    assert_eq!(s.observe(&behind, seen(true)), Some(vec!["$".to_string()]));
    assert_eq!(s.observe(&later, seen(false)), Some(vec![]));
    let state = s.export_state();
    assert_eq!(state.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), [SESSION_MEMORY]);
    let mut again = sensor(&d.0, Some("s1"));
    again.import_state(&state);
    assert_eq!(again.observe(&behind, seen(true)), Some(vec![]), "spent before the restart");
    assert_eq!(again.observe(&behind, seen(false)), Some(vec![]));
    assert_eq!(again.observe(&behind, seen(true)), Some(vec![]));
    assert_eq!(again.observe(&later, seen(true)), Some(vec!["$".to_string()]), "it saw false before the restart, so true is an entry");
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
    assert_eq!(again.observe(&t, seen(true)), Some(vec![]));
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

#[test]
fn a_state_trigger_is_not_evaluated_here() {
    let d = Dir::new("state");
    let mut s = sensor(&d.0, Some("s1"));
    assert_eq!(s.observe(&trig("{name: up, state: {n: rises}}"), seen(true)), None);
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
    assert_eq!(s.observe(&t, seen(false)), Some(vec![]));
    s.poll(&focus);
    let key = Key::new("ways-update", "/p", Some("s1"));
    let rec = read_at(&key.path_in(&d.0), &key).unwrap();
    assert_eq!((rec.poll.as_ref().map(|p| (p.ok, p.observations)), rec.memory["behind"].seen.clone()), (Some((true, 0)), seen(false)));
    let mut reopened = sensor(&d.0, Some("s1"));
    assert_eq!(reopened.observe(&t, seen(true)), Some(vec!["$".to_string()]), "it saw false before, so true is an entry and not a baseline");
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
    assert_eq!(s.observe(&enters("behind", "store"), seen(true)), Some(vec![]), "it starts from a baseline");
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
    assert_eq!(sweep_in(&d.0, SystemTime::now()), 3);
    let mut left: Vec<PathBuf> = std::fs::read_dir(&d.0).unwrap().flatten().map(|e| e.path()).collect();
    left.sort();
    let mut want = vec![fresh, in_flight];
    want.sort();
    assert_eq!(left, want);
    assert_eq!(sweep_in(&d.0, SystemTime::now()), 0, "a second sweep finds nothing");
}

#[test]
fn a_session_s_stores_move_with_it_and_its_memory_comes_too() {
    let d = Dir::new("move");
    let t = enters("behind", "store");
    let mut s = sensor(&d.0, Some("old"));
    s.observe(&t, seen(false));
    assert_eq!(move_stores_in(&d.0, "old", "old"), 0, "a move to the same id does nothing");
    assert!(read_at(&Key::new("ways-update", "/p", Some("old")).path_in(&d.0), &Key::new("ways-update", "/p", Some("old"))).is_some(), "and keeps the store");
    assert_eq!(move_stores_in(&d.0, "old", "new"), 1);
    assert!(read_at(&Key::new("ways-update", "/p", Some("old")).path_in(&d.0), &Key::new("ways-update", "/p", Some("old"))).is_none());
    let mut moved = sensor(&d.0, Some("new"));
    assert_eq!(moved.observe(&t, seen(true)), Some(vec!["$".to_string()]), "the memory moved, so this is an entry and not a baseline");
    assert_eq!(move_stores_in(&d.0, "old", "new"), 0, "a second move finds nothing");
}

/// A store whose copy cannot be written stays where it is.
#[test]
fn a_move_that_cannot_write_keeps_the_source() {
    let d = Dir::new("move-fail");
    let from = Key::new("ways-update", "/p", Some("old"));
    let mut s = sensor(&d.0, Some("old"));
    s.observe(&enters("behind", "store"), seen(false));
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
