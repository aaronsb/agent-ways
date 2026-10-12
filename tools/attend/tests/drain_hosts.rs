//! What the Stop-hook drain prints (ADR-172, ADR-406 item 9), through the
//! CLI as the hook runs it.
//!
//! A delivery goes as the Stop hook's `additionalContext`, which Claude
//! Code shows once as `Stop hook feedback`; a `decision: block` showed
//! each delivery a second time as a hook error.

mod common;
use common::Fixture;
use serde_json::Value;

/// Write a signal from `peer-sess` into `tray`, threaded under `re` when given.
fn put_from_peer(f: &Fixture, tray: &str, id: &str, re: Option<&str>, body: &str) {
    let dir = f.signals().join(tray);
    std::fs::create_dir_all(&dir).unwrap();
    let line = agent_identity::signal::format_signal("claude:peer-sess", "elsewhere", "/tmp/elsewhere", re, body);
    std::fs::write(dir.join(format!("{id}.signal")), line).unwrap();
}

/// The text a hook drain hands the model, from its JSON.
fn delivered_text(out: &str) -> String {
    let doc: Value = serde_json::from_str(out).unwrap_or_else(|e| panic!("{e}: {out}"));
    assert!(doc.get("decision").is_none(), "a delivery is not a block decision: {out}");
    assert_eq!(doc["hookSpecificOutput"]["hookEventName"], "Stop", "{out}");
    doc["hookSpecificOutput"]["additionalContext"].as_str().unwrap_or_else(|| panic!("no additionalContext: {out}")).to_string()
}

/// Finding 1: a delivery continues the turn as context, not as a block.
#[test]
fn a_delivery_is_context_not_a_block() {
    let f = Fixture::new("drain-host-cli");
    f.ok(&["listen", "on"]);
    f.drain("plain");
    put_from_peer(&f, "_broadcast", "peer-sess-1-1", None, "hello from a peer");
    let text = delivered_text(&f.drain_hook_with("{}"));
    assert!(text.contains("hello from a peer"), "{text}");
    assert!(text.contains("id peer-sess-1-1"), "{text}");
    assert!(text.contains("attend reply \"...\" auto-threads"), "{text}");
}
