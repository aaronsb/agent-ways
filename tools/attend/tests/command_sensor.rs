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
    let one_trigger = f.attend(&["sensor", "fire", "units/added", "--input", &two, "--commit", "--dry-run"]);
    assert!(!one_trigger.status.success());
    assert!(
        String::from_utf8_lossy(&one_trigger.stderr).contains("--commit fires the whole sensor, since advancing added alone would leave the other triggers' memory behind; fire units --commit"),
        "{}",
        String::from_utf8_lossy(&one_trigger.stderr)
    );
    assert_eq!(stores(), before, "the refused commit wrote nothing");
    let sessionless = f.without_session(|| f.attend(&["sensor", "fire", "units", "--input", &two, "--commit", "--dry-run"]));
    assert!(!sessionless.status.success());
    assert!(String::from_utf8_lossy(&sessionless.stderr).contains("no session here, so --commit keeps nothing; name one with --session or run it from the session's shell"), "{}", String::from_utf8_lossy(&sessionless.stderr));
    assert_eq!(stores(), before, "a commit with no session wrote nothing");
}

/// What a command sensor's child sees, by variable name, from `attend
/// sensor poll` run with two fake secrets and a harmless variable set.
fn child_env(f: &Fixture, sensor: &str) -> std::collections::BTreeMap<String, String> {
    let fake = [("CLAUDE_CODE_SESSION_ID", "fake-session"), ("ANTHROPIC_API_KEY", "fake-key"), ("KEEP_ME", "kept")];
    let env: Vec<(&str, &std::ffi::OsStr)> = fake.iter().map(|(k, v)| (*k, std::ffi::OsStr::new(*v))).collect();
    let out = f.attend_with(&["sensor", "poll", sensor, "--dry-run", "--json"], &env);
    assert!(out.status.success(), "attend sensor poll {sensor} failed");
    let doc = doc(&String::from_utf8_lossy(&out.stdout));
    assert_eq!(doc["failure"], Value::Null, "the command ran");
    doc["result"].as_array().unwrap().iter().filter_map(|o| o["description"].as_str()?.split_once('=').map(|(k, v)| (k.to_string(), v.to_string()))).collect()
}

/// `env -0` with each variable on one line, as `output: lines`.
const ENV_SENSOR: &str = "kind: command\nargv: [sh, -c, \"env -0 | tr '\\\\n\\\\000' ' \\\\n' | sed 's/^/1|/'\"]\noutput: lines\nenv: {ADDED: by-file}\n";

/// A user file's command inherits attend's environment less Claude
/// Code's, Anthropic's and attend's variables; its `env` adds on top.
#[test]
fn a_user_command_inherits_the_environment_without_the_session_s_secrets() {
    let f = Fixture::new("cmd-env-user");
    f.sensor("envu", ENV_SENSOR);
    let seen = child_env(&f, "envu");
    assert_eq!(seen.get("KEEP_ME").map(String::as_str), Some("kept"));
    assert_eq!(seen.get("ADDED").map(String::as_str), Some("by-file"));
    assert!(!seen.contains_key("CLAUDE_CODE_SESSION_ID"), "the session id reached the command");
    assert!(!seen.contains_key("ANTHROPIC_API_KEY"), "the API key reached the command");
    assert!(seen.keys().all(|k| !k.starts_with("CLAUDE") && !k.starts_with("ANTHROPIC_") && !k.starts_with("ATTEND_")), "{:?}", seen.keys().collect::<Vec<_>>());
}

/// A project file's command starts from the fixed set alone, plus its
/// `env`.
#[test]
fn a_project_command_inherits_only_the_fixed_set() {
    let f = Fixture::new("cmd-env-project");
    std::fs::write(f.home.join("config").join("attend").join("config.yaml"), format!("cleanup:\n  enabled: false\nproject_sensors:\n  - {}\n", f.origin)).unwrap();
    let dir = std::path::Path::new(&f.origin).join(".claude").join("attend").join("sensors");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("envp.yaml"), ENV_SENSOR).unwrap();
    let seen = child_env(&f, "envp");
    assert_eq!(seen.get("ADDED").map(String::as_str), Some("by-file"));
    assert_eq!(seen.get("HOME").map(String::as_str), Some(f.home.to_str().unwrap()));
    assert!(seen.contains_key("PATH"));
    for gone in ["KEEP_ME", "CLAUDE_CODE_SESSION_ID", "ANTHROPIC_API_KEY", "XDG_CONFIG_HOME"] {
        assert!(!seen.contains_key(gone), "{gone} reached a project command");
    }
}
