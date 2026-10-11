//! The message verbs' `--json` documents, the contract a frontend that
//! shells out to `attend` reads (ADR-185, ADR-501 item 2).

mod common;
use common::Fixture;
use serde_json::Value;
use std::time::Duration;

fn json(out: &std::process::Output) -> Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!("stdout is one JSON document ({e}): {}", String::from_utf8_lossy(&out.stdout))
    })
}

fn ok_json(f: &Fixture, args: &[&str]) -> Value {
    let out = f.attend(args);
    assert!(out.status.success(), "attend {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    json(&out)
}

/// Text a shell would mangle reaches the bus as written when it is passed as
/// an argument, and comes back out of the inbox the same.
#[test]
fn a_send_round_trips_through_the_inbox_with_its_text_intact() {
    let f = Fixture::new("verbs-json-roundtrip");
    let text = r#"quotes "double" 'single', `backticks`, $HOME and $(date), history !! and a | pipe"#;
    let sent = f.without_session(|| ok_json(&f, &["send", "--json", text]));
    assert_eq!(sent["scope"], "#open");
    assert_eq!(sent["dirs"], 1);
    assert_eq!(sent["failed"], serde_json::json!([]));
    let id = sent["id"].as_str().expect("an id");

    let listing = ok_json(&f, &["inbox", "--json"]);
    assert_eq!(listing["total"], 1);
    let msg = &listing["messages"][0];
    assert_eq!(msg["id"], id);
    assert_eq!(msg["message"], text);
    assert!(msg["from"].as_str().unwrap().starts_with("external:"), "{msg}");

    let read = ok_json(&f, &["inbox", id, "--json"]);
    assert_eq!(read["message"], text);
    assert_eq!(read["id"], id);
}

#[test]
fn a_reply_threads_under_the_message_the_drain_delivered() {
    let f = Fixture::new("verbs-json-reply");
    // Enrolled through a channel, so the drain delivers; it records the
    // message it delivered as the one a reply threads under.
    f.ok(&["join", "room"]);
    f.drain("plain");
    f.put("_broadcast", "other-session-1-1", "a question", Duration::from_secs(1));
    assert!(f.drain("plain").contains("a question"));

    let sent = ok_json(&f, &["reply", "--json", "an answer"]);
    assert_eq!(sent["re"], "other-session-1-1");
}

#[test]
fn a_refusal_is_one_json_error_document_with_the_exit_code_unchanged() {
    let f = Fixture::new("verbs-json-refusal");
    let out = f.attend(&["send", "--json", ""]);
    assert_eq!(out.status.code(), Some(1));
    let doc = json(&out);
    assert_eq!(doc["error"]["kind"], "empty_message");
    assert!(doc["message"].is_string());
    assert!(String::from_utf8_lossy(&out.stderr).contains("usage: attend send"), "the human wording stays on stderr");

    let out = f.attend(&["reply", "--json", "nothing to answer"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(json(&out)["error"]["kind"], "no_inbound");

    let out = f.attend(&["inbox", "no-such-id", "--json"]);
    assert_eq!(out.status.code(), Some(0), "a missing message is a benign race");
    assert_eq!(json(&out)["error"]["kind"], "not_found");
}

#[test]
fn channel_status_and_peer_verbs_print_their_state() {
    let f = Fixture::new("verbs-json-state");
    let joined = ok_json(&f, &["join", "#deploy", "--pin", "--json"]);
    assert_eq!(joined["name"], "deploy");
    assert_eq!(joined["pinned"], true);

    let mine = ok_json(&f, &["channels", "--joined", "--json"]);
    assert_eq!(mine, serde_json::json!([{ "name": "deploy", "pinned": true }]));

    let status = ok_json(&f, &["status", "--json"]);
    assert_eq!(status["channels"][0]["name"], "deploy");
    assert!(status["signals"]["open"].is_number());

    let peers = ok_json(&f, &["peers", "--json"]);
    assert_eq!(peers["self"]["session_id"], f.sid.as_str());
    assert_eq!(peers["channels"][0]["name"], "deploy");

    let left = ok_json(&f, &["leave", "deploy", "--json"]);
    assert_eq!(left["was_member"], true);
    let again = ok_json(&f, &["leave", "deploy", "--json"]);
    assert_eq!(again["was_member"], false);

    let all = ok_json(&f, &["channels", "--json"]);
    assert!(all.as_array().unwrap().iter().any(|c| c["name"] == "deploy" && c["joined"] == false));
}
