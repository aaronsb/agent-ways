//! `attend send` through the CLI: a long message goes out in numbered parts,
//! a send that could not be written says which parts went out, and a send
//! to this session's own project names that as the problem (ADR-406 item 10).

mod common;
use common::Fixture;
use serde_json::Value;

fn json(out: &std::process::Output) -> Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!("stdout is one JSON document ({e}): {}", String::from_utf8_lossy(&out.stdout))
    })
}

/// The messages in `#open`, oldest first, as `inbox --json` lists them.
fn open_messages(f: &Fixture) -> Vec<Value> {
    let out = f.attend(&["inbox", "--json", "--limit", "50"]);
    assert!(out.status.success());
    json(&out)["messages"].as_array().unwrap().clone()
}

#[test]
fn a_long_message_goes_out_as_numbered_parts_in_order() {
    let f = Fixture::new("send-parts-long");
    let body: String = (0..200).map(|i| format!("w{i:03}")).collect::<Vec<_>>().join(" ");
    assert!(body.chars().count() > 2 * attend::verbs::send::PART_MAX);

    let out = f.without_session(|| f.attend(&["send", "--json", "--", &body]));
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let sent = json(&out);
    let parts: Vec<String> = sent["parts"].as_array().unwrap().iter().map(|p| p.as_str().unwrap().to_string()).collect();
    assert_eq!(parts.len(), 3);
    assert_eq!(sent["id"], parts[0]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("part 1/3") && err.contains("part 3/3"), "{err}");

    let msgs = open_messages(&f);
    assert_eq!(msgs.len(), 3);
    let mut rejoined = Vec::new();
    for (i, m) in msgs.iter().enumerate() {
        assert_eq!(m["id"], parts[i], "listed in send order");
        let text = m["message"].as_str().unwrap();
        assert!(text.chars().count() <= attend::verbs::send::PART_MAX, "{text}");
        let (prefix, rest) = text.split_once(") ").unwrap();
        assert_eq!(prefix, format!("({}/3", i + 1));
        rejoined.push(rest.to_string());
    }
    assert_eq!(rejoined.join(" "), body, "nothing lost at the breaks");
}

#[test]
fn a_short_message_goes_out_as_one_part() {
    let f = Fixture::new("send-parts-short");
    let out = f.without_session(|| f.attend(&["send", "--json", "hello"]));
    let sent = json(&out);
    assert_eq!(sent["parts"], serde_json::json!([sent["id"].clone()]));
    assert_eq!(open_messages(&f)[0]["message"], "hello", "no part prefix");
}

/// A send no tray took exits 1 and says that no part went out.
#[test]
fn a_send_that_could_not_be_written_says_which_parts_went_out() {
    let f = Fixture::new("send-parts-failed");
    // `#open`'s tray is a file, so no signal can be written into it.
    std::fs::create_dir_all(f.signals()).unwrap();
    std::fs::write(f.signals().join("_broadcast"), "not a directory").unwrap();
    let body = "x ".repeat(300);

    let out = f.without_session(|| f.attend(&["send", "--json", "--", &body]));
    assert_eq!(out.status.code(), Some(1));
    let doc = json(&out);
    assert_eq!(doc["error"]["kind"], "incomplete");
    assert_eq!(doc["error"]["sent"], serde_json::json!([]));
    assert_eq!(doc["error"]["total"], 2);
    assert_eq!(doc["error"]["failed_part"], 1);
    let msg = doc["message"].as_str().unwrap();
    assert!(msg.contains("part 1 of 2 could not be written") && msg.contains("no part went out"), "{msg}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("parts 1-2 did not"));
}

/// The parts that went out are named when a later one fails.
#[test]
fn an_incomplete_send_names_the_parts_that_went_out() {
    let e = attend::verbs::SendError::Incomplete {
        sent: vec!["a-1-0".into(), "a-1-1".into()],
        total: 4,
        failed_part: 3,
        error: "disk full".into(),
    };
    assert_eq!(
        e.to_string(),
        "part 3 of 4 could not be written (disk full); parts 1-2 went out (a-1-0, a-1-1), parts 3-4 did not"
    );
}

/// `--to` with this session's own project, where no other session works,
/// says so instead of "no active peer", and writes nothing.
#[test]
fn a_send_to_the_own_project_names_the_problem() {
    let f = Fixture::new("send-own-project");
    let out = f.attend(&["send", "--json", "--to", &f.origin, "note to self"]);
    assert_eq!(out.status.code(), Some(1));
    let doc = json(&out);
    assert_eq!(doc["error"]["kind"], "own_project");
    let msg = doc["message"].as_str().unwrap();
    assert!(msg.contains("own project") && msg.contains("no other session"), "{msg}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("no active peer"), "{err}");
    assert!(!f.signals().join(f.project_tray()).exists(), "nothing written");
}

/// Two senders' signals with one mtime list in send order, which the id's
/// timestamp carries, not in the order of the senders' names.
#[test]
fn signals_with_one_mtime_list_in_send_order_across_senders() {
    let f = Fixture::new("send-order-senders");
    let dir = f.signals().join("_broadcast");
    std::fs::create_dir_all(&dir).unwrap();
    let mtime = std::time::SystemTime::now() - std::time::Duration::from_secs(5);
    for (id, body) in [("zed-1791000000000000001-0", "first"), ("abe-1791000000000000002-0", "second")] {
        let path = dir.join(format!("{id}.signal"));
        std::fs::write(&path, format!("claude:{id}|p|/tmp/p|{body}\n")).unwrap();
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(mtime).unwrap();
    }
    let bodies: Vec<String> = open_messages(&f).iter().map(|m| m["message"].as_str().unwrap().to_string()).collect();
    assert_eq!(bodies, ["first", "second"]);
}
