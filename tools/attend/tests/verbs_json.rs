//! The message verbs' `--json` documents, the contract a frontend that
//! shells out to `attend` reads (ADR-185, ADR-501 item 2).

mod common;
use common::Fixture;
use serde_json::Value;
#[cfg(feature = "sensor-peers")]
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

/// A message passed to `send` as one argv element comes back from `inbox
/// --json`, listed and read, byte for byte. No shell runs here: this proves
/// attend keeps the text it is handed, not how a shell would pass it.
#[test]
fn a_send_round_trips_through_the_inbox_with_its_text_intact() {
    let f = Fixture::new("verbs-json-roundtrip");
    let texts = [
        r#"quotes "double" 'single', `backticks`, $HOME and $(date), history !! and a | pipe"#,
        "trailing spaces   ",
        "  leading spaces",
        "line one\nline two\n",
        "non-ASCII: héllo, wörld, 日本語, 🚀",
        r"backslashes C:\path\to\file and a literal \n",
        "re: a reply-shaped start with no id",
        "re:not an id|so not a thread",
    ];
    for (n, text) in texts.iter().enumerate() {
        let sent = f.without_session(|| ok_json(&f, &["send", "--json", "--", text]));
        assert_eq!(sent["scope"], "#open");
        assert_eq!(sent["dirs"], 1);
        assert_eq!(sent["failed"], serde_json::json!([]));
        assert_eq!(sent["re"], Value::Null);
        let id = sent["id"].as_str().expect("an id");

        let listing = ok_json(&f, &["inbox", "--json"]);
        assert_eq!(listing["total"], n + 1);
        let msg = listing["messages"].as_array().unwrap().iter().find(|m| m["id"] == id).expect("listed");
        assert_eq!(msg["message"], *text, "listed");
        assert_eq!(msg["re"], Value::Null, "the reader sees no thread: {text:?}");
        assert!(msg["from"].as_str().unwrap().starts_with("external:"), "{msg}");

        let read = ok_json(&f, &["inbox", id, "--json"]);
        assert_eq!(read["message"], *text, "read");
        assert_eq!(read["re"], Value::Null);
        assert_eq!(read["id"], id);
    }
}

/// An unthreaded message that starts `re:`, even one shaped like a thread
/// id, goes out and reads back as written, unthreaded: the wire marks it
/// with an empty thread field.
#[test]
fn a_message_starting_re_reads_back_as_written() {
    let f = Fixture::new("verbs-json-re-prefix");
    for text in ["re:abc-1|looks threaded", "re: the plan"] {
        let sent = f.without_session(|| ok_json(&f, &["send", "--json", text]));
        assert_eq!(sent["re"], serde_json::Value::Null, "{text}");
        let id = sent["id"].as_str().unwrap().to_string();
        let read = ok_json(&f, &["inbox", &id, "--json"]);
        assert_eq!(read["message"], text);
        assert_eq!(read["re"], serde_json::Value::Null, "{text}");
    }
}

// Needs `attend send`/`reply`, compiled out without the peers sensor.
#[cfg(feature = "sensor-peers")]
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
    assert_eq!(sent["threading"], "threaded");
}

// Needs `attend send`/`reply`, compiled out without the peers sensor.
#[cfg(feature = "sensor-peers")]
#[test]
fn a_refusal_is_one_json_error_document_with_the_exit_code_unchanged() {
    let f = Fixture::new("verbs-json-refusal");
    let out = f.attend(&["reply", "--json", "nothing to answer"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(json(&out)["error"]["kind"], "no_inbound");

    let out = f.attend(&["inbox", "no-such-id", "--json"]);
    assert_eq!(out.status.code(), Some(0), "a missing message is a benign race");
    assert_eq!(json(&out)["error"]["kind"], "not_found");

    let out = f.attend(&["join", "bad name!", "--json"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(json(&out)["error"]["kind"], "invalid_name");
}

/// Exit 2 is usage; under `--json` its document is `{"error":{"kind":
/// "usage"},"message":...}` and nothing is sent (ADR-185, note of
/// 2026-10-10).
// Needs `attend send`/`reply`, compiled out without the peers sensor.
#[cfg(feature = "sensor-peers")]
#[test]
fn a_usage_refusal_under_json_is_kind_usage_with_exit_2() {
    let f = Fixture::new("verbs-json-usage");
    let usage = |args: &[&str]| {
        let out = f.attend(args);
        assert_eq!(out.status.code(), Some(2), "attend {args:?}");
        let doc = json(&out);
        assert_eq!(doc["error"], serde_json::json!({ "kind": "usage" }), "attend {args:?}");
        assert!(doc["message"].as_str().is_some_and(|m| !m.is_empty()));
        assert!(!out.stderr.is_empty(), "the human wording stays on stderr");
    };
    usage(&["send", "--json", "--chanel", "x", "hello"]);
    usage(&["channels", "--json", "pin", "x"]);
    // An empty message is a malformed command line, not a failed send.
    usage(&["send", "--json", ""]);
    let out = f.attend(&["send", ""]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("usage: attend send"));
    // An empty reply is the same refusal, under its own verb's usage, even
    // with nothing received to reply to.
    usage(&["reply", "--json", ""]);
    let out = f.attend(&["reply", ""]);
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("usage: attend reply") && !err.contains("attend send <"), "{err}");
    // A conflict clap raises itself is exit 2 in clap's text, with no JSON.
    let out = f.attend(&["inbox", "--drain", "--format", "hook", "--json"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    // `--json` after the message is caught, not sent as text, and still
    // answers in JSON.
    usage(&["send", "hello", "--json"]);
    usage(&["reply", "thanks", "--json"]);
    let out = f.attend(&["send", "hello", "--channel", "deploy"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("after the message"));
    assert_eq!(ok_json(&f, &["inbox", "--json"])["total"], 0, "nothing was sent");
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
