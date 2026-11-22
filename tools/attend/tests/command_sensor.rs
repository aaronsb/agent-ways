// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! `attend sensor poll` and `fire` on a command sensor: the command runs,
//! its triggers are evaluated against the session's store, each verdict is
//! printed, a failing command says why, and a test poll or fire writes
//! nothing to the store.

#![cfg(unix)]

mod common;

use common::Fixture;
use serde_json::{json, Value};

fn doc(out: &str) -> Value {
    serde_json::from_str(out).unwrap_or_else(|e| panic!("{e}: {out}"))
}

#[test]
fn a_poll_runs_the_command_and_prints_each_trigger_s_verdict() {
    let f = Fixture::new("cmd-poll");
    f.sensor(
        "behind",
        "kind: command\nargv: [sh, -c, 'printf \"{\\\"behind\\\": true, \\\"n\\\": 3}\"']\noutput: json\n\
         triggers:\n  - {name: late, path: \"$\", on: enters, where: {behind: true}, memory: session, magnitude: 2, description: \"behind by {n}\"}\n  \
         - {name: early, path: \"$\", on: enters, where: {behind: false}}\n",
    );
    let out = doc(&f.ok(&["sensor", "poll", "behind", "--dry-run", "--json"]));
    assert_eq!(out["state"], json!("active"));
    assert_eq!(out["failure"], Value::Null);
    assert_eq!(out["result"], json!([{"magnitude": 2.0, "description": "behind by 3"}]));
    assert_eq!(out["triggers"], json!([{"trigger": "late", "fired": true, "keys": ["$"]}, {"trigger": "early", "fired": false, "keys": []}]));
    let text = f.ok(&["sensor", "poll", "behind", "--dry-run"]);
    assert!(text.contains("  trigger late: fired for $\n  trigger early: quiet\n"), "{text}");
    assert!(text.contains("   2.0  behind by 3\n"), "{text}");
}

#[test]
fn a_failing_command_says_why() {
    let f = Fixture::new("cmd-fail");
    f.sensor("broken", "kind: command\nargv: [sh, -c, 'echo gone >&2; exit 3']\noutput: json\n");
    let out = doc(&f.ok(&["sensor", "poll", "broken", "--dry-run", "--json"]));
    assert_eq!(out["failure"], json!("sh exited with status 3: gone"));
    assert_eq!(out["result"], json!([]));
}

/// A committed fire sets the store; a fire without `--commit` compares
/// against it and leaves it as it was.
#[test]
fn a_test_fire_compares_against_the_store_and_writes_only_with_commit() {
    let f = Fixture::new("cmd-fire");
    f.sensor("units", "kind: command\nargv: ['true']\noutput: json\ntriggers:\n  - {name: added, path: \"$.units[*]\", key: unit, on: added, description: \"unit {unit} failed\"}\n");
    let input = |name: &str, units: &[&str]| {
        let p = f.home.join(name);
        std::fs::write(&p, json!({"units": units.iter().map(|u| json!({"unit": u})).collect::<Vec<_>>()}).to_string()).unwrap();
        p.to_string_lossy().into_owned()
    };
    let (one, two) = (input("one.json", &["a"]), input("two.json", &["a", "b"]));
    let first = doc(&f.ok(&["sensor", "fire", "units", "--input", &one, "--commit", "--dry-run", "--json"]));
    assert_eq!(first["triggers"], json!([{"trigger": "added", "fired": false, "keys": []}]), "the committed fire is the baseline");
    let stores = || std::fs::read_dir(f.home.join("state").join("agent-ways").join("attend").join("sensor-state")).unwrap().flatten().map(|e| std::fs::read(e.path()).unwrap()).collect::<Vec<_>>();
    let before = stores();
    assert_eq!(before.len(), 1);
    for _ in 0..2 {
        let next = doc(&f.ok(&["sensor", "fire", "units/added", "--input", &two, "--dry-run", "--json"]));
        assert_eq!(next["triggers"], json!([{"trigger": "added", "fired": true, "keys": ["b"]}]));
        assert_eq!(next["input"], json!([{"magnitude": 1.0, "description": "[test] unit b failed"}]));
    }
    assert_eq!(stores(), before, "a fire without --commit writes nothing");
}
