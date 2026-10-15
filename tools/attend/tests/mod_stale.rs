//! The Stop-hook drain tells a session once that its attend mod is older than
//! the installed one, through the CLI as the hook runs it.
//!
//! The fixture's session started a day ago. The installed app's mod manifest
//! lives under `$XDG_DATA_HOME/agent-ways`, which the fixture points into its
//! own home.

mod common;
use common::Fixture;
use serde_json::Value;

fn install_manifest(f: &Fixture, version: &str) {
    let dir = f.home.join("data").join("agent-ways").join("plugins").join("attend").join(".claude-plugin");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("plugin.json"), format!(r#"{{"name": "attend", "version": "{version}"}}"#)).unwrap();
}

/// The operator-facing message of a hook drain, if it printed one.
fn system_message(out: &str) -> Option<String> {
    if out.trim().is_empty() {
        return None;
    }
    let doc: Value = serde_json::from_str(out).unwrap_or_else(|e| panic!("{e}: {out}"));
    doc["systemMessage"].as_str().map(str::to_string)
}

#[test]
fn an_early_session_is_told_once_through_the_stop_hook() {
    let f = Fixture::new("mod-stale-time");
    f.ok(&["listen", "on", "--host", "mod"]);
    f.drain("plain");
    assert_eq!(system_message(&f.drain_hook_with("{}")), None, "no installed manifest, nothing to say");
    install_manifest(&f, "0.28.0");
    let note = system_message(&f.drain_hook_with("{}")).expect("the session predates the installed mod");
    assert!(note.contains("/reload-plugins") && note.contains("idle wakes may not fire"), "{note}");
    assert_eq!(system_message(&f.drain_hook_with("{}")), None, "once per session");
}

#[test]
fn a_session_reporting_the_installed_version_is_not_told() {
    let f = Fixture::new("mod-stale-current");
    f.ok(&["listen", "on", "--host", "mod"]);
    f.drain("plain");
    install_manifest(&f, "0.28.0");
    f.ok(&["listen", "beat", "--mod", "0.28.0"]);
    assert_eq!(system_message(&f.drain_hook_with("{}")), None, "the mod on the installed version, whatever the start time");
}

#[test]
fn a_session_reporting_an_older_version_is_told_even_on_a_delivery() {
    let f = Fixture::new("mod-stale-older");
    f.ok(&["listen", "on", "--host", "mod"]);
    f.drain("plain");
    install_manifest(&f, "0.28.0");
    f.ok(&["listen", "beat", "--mod", "0.27.1"]);
    let dir = f.signals().join("_broadcast");
    std::fs::create_dir_all(&dir).unwrap();
    let line = agent_identity::signal::format_signal("claude:peer-sess", "elsewhere", "/tmp/elsewhere", None, "hello");
    std::fs::write(dir.join("peer-sess-1-1.signal"), line).unwrap();
    let out = f.drain_hook_with("{}");
    let doc: Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out}"));
    assert!(doc["hookSpecificOutput"]["additionalContext"].as_str().unwrap().contains("hello"), "{out}");
    assert!(doc["systemMessage"].as_str().unwrap().contains("/reload-plugins"), "{out}");
}
