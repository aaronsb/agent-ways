//! One message is one signal, whatever its length (ADR-412). `attend send`
//! writes it whole, and every conduit delivers it whole under its one id:
//! the Stop-hook drain, the attend mod's format, and the Monitor line of
//! `attend run`. Signals an older peer sent as numbered `(i/n)` parts still
//! read, in order, as the messages they are.

mod common;
use common::Fixture;
use serde_json::Value;
use std::time::Duration;

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

/// A body of about 1,500 characters in sentences, ending in a marker.
fn long_body() -> String {
    let mut body: String =
        (0..30).map(|i| format!("Sentence {i:02} says one plain thing about the rebase.")).collect::<Vec<_>>().join(" ");
    body.push_str(" END-OF-MESSAGE");
    assert!(body.chars().count() > 1200);
    body
}

/// Write a signal from `peer-sess` into `tray`.
fn put_from_peer(f: &Fixture, tray: &str, id: &str, body: &str) {
    let dir = f.signals().join(tray);
    std::fs::create_dir_all(&dir).unwrap();
    let line = agent_identity::signal::format_signal("claude:peer-sess", "elsewhere", "/tmp/elsewhere", None, body);
    std::fs::write(dir.join(format!("{id}.signal")), line).unwrap();
}

/// The `.signal` files in `#open`'s tray.
fn open_tray_files(f: &Fixture) -> Vec<String> {
    let dir = f.signals().join("_broadcast");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.ends_with(".signal"))
        .collect();
    names.sort();
    names
}

#[test]
fn a_long_message_goes_out_as_one_signal() {
    let f = Fixture::new("one-signal-send");
    let body = long_body();

    let out = f.without_session(|| f.attend(&["send", "--json", "--", &body]));
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let sent = json(&out);
    let id = sent["id"].as_str().unwrap().to_string();
    assert!(sent.get("parts").is_none(), "no parts: {sent}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("part "), "{err}");

    assert_eq!(open_tray_files(&f), vec![format!("{id}.signal")], "one signal on disk");
    let msgs = open_messages(&f);
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["id"], id);
    assert_eq!(msgs[0]["message"], body, "the body as written, no (i/n) prefix");
}

#[test]
fn a_short_message_goes_out_as_one_signal() {
    let f = Fixture::new("one-signal-short");
    let out = f.without_session(|| f.attend(&["send", "--json", "hello"]));
    assert!(out.status.success());
    assert_eq!(open_messages(&f)[0]["message"], "hello");
}

/// A send no tray took exits 1 and says the message was not written.
#[test]
fn a_send_that_could_not_be_written_says_so() {
    let f = Fixture::new("one-signal-failed");
    // `#open`'s tray is a file, so no signal can be written into it.
    std::fs::create_dir_all(f.signals()).unwrap();
    std::fs::write(f.signals().join("_broadcast"), "not a directory").unwrap();
    let body = long_body();

    let out = f.without_session(|| f.attend(&["send", "--json", "--", &body]));
    assert_eq!(out.status.code(), Some(1));
    let doc = json(&out);
    assert_eq!(doc["error"]["kind"], "not_written");
    let msg = doc["message"].as_str().unwrap();
    assert!(msg.starts_with("the message could not be written: "), "{msg}");
    assert!(!msg.contains("part"), "{msg}");
}

/// The drain delivers a long message whole, once, under one id, in the
/// CLI's format and in the mod's, and it counts as one unread message.
#[test]
fn the_drain_delivers_a_long_message_whole_under_one_handle() {
    let body = long_body();
    let handle = attend::verbs::inbox::short_id("peer-sess-1791000000000000001-0");

    let f = Fixture::new("one-signal-drain");
    f.ok(&["listen", "on"]);
    f.drain("plain");
    put_from_peer(&f, "_broadcast", "peer-sess-1791000000000000001-0", &body);
    let unread = json(&f.attend(&["unread", "--json"]));
    assert_eq!(unread["unread"], 1, "{unread}");
    let drained = f.drain("plain");
    assert!(drained.contains(&body), "whole: {drained}");
    assert_eq!(drained.matches("END-OF-MESSAGE").count(), 1, "once: {drained}");
    assert!(drained.contains("1 message(s) drained"), "{drained}");
    assert_eq!(json(&f.attend(&["unread", "--json"]))["unread"], 0);
    assert!(!f.drain("plain").contains("END-OF-MESSAGE"), "delivered once");

    let m = Fixture::new("one-signal-drain-mod");
    m.ok(&["listen", "on", "--host", "mod"]);
    m.drain("plain");
    put_from_peer(&m, "_broadcast", "peer-sess-1791000000000000001-0", &body);
    let out = m.drain_hook_with("{}");
    let doc: Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out}"));
    let text = doc["hookSpecificOutput"]["additionalContext"].as_str().unwrap();
    let lines: Vec<&str> = text.lines().filter(|l| l.contains("Sentence 00")).collect();
    assert_eq!(lines.len(), 1, "one line: {text}");
    assert!(lines[0].contains(&body), "whole: {text}");
    assert!(lines[0].ends_with(&format!("END-OF-MESSAGE ~{handle}")), "{text}");
    assert_eq!(lines[0].matches(" ~").count(), 1, "one handle: {text}");
}

/// The Monitor line of `attend run` carries the message whole, on one line,
/// with its handle ahead of the body, so a reply to it threads to the
/// message.
#[cfg(feature = "sensor-peers")]
#[test]
fn the_monitor_line_carries_a_long_message_whole_with_its_handle() {
    let f = Fixture::new("one-signal-run");
    let body = long_body();
    let id = "peer-sess-1791000000000000001-0";
    // Fresh when the first scan finds it, so the cold start delivers it.
    put_from_peer(&f, "_broadcast", id, &body);
    let run = f.run();
    run.wait_for("the message", Duration::from_secs(30), |r| r.output().contains("END-OF-MESSAGE"));
    let out = run.output();
    let lines: Vec<&str> = out.lines().filter(|l| l.contains("message from")).collect();
    assert_eq!(lines.len(), 1, "one line: {out}");
    let line = lines[0];
    assert!(line.contains(&body), "whole: {line}");
    assert!(!line.contains("(1/"), "no part counter: {line}");
    let handle = attend::verbs::inbox::short_id(id);
    let (head, _) = line.split_once("Sentence 00").unwrap();
    assert!(head.contains(&format!("~{handle}")), "the handle leads the body: {line}");
}

/// Numbered parts an older peer sent still read, in order, each as the
/// signal it is, through the inbox and the drain.
#[test]
fn legacy_numbered_parts_still_read_in_order() {
    let f = Fixture::new("one-signal-legacy");
    f.ok(&["listen", "on"]);
    f.drain("plain");
    let dir = f.signals().join("_broadcast");
    std::fs::create_dir_all(&dir).unwrap();
    let mtime = std::time::SystemTime::now() - Duration::from_secs(5);
    let parts = [
        ("old-peer-1791000000000000001-0", "(1/3) the first third"),
        ("old-peer-1791000000000000002-0", "(2/3) the second third"),
        ("old-peer-1791000000000000003-0", "(3/3) the last third"),
    ];
    // Written out of order, with one mtime, as a fast multi-part send left them.
    for (id, body) in parts.iter().rev() {
        let path = dir.join(format!("{id}.signal"));
        std::fs::write(&path, format!("claude:old-peer|p|/tmp/p|{body}\n")).unwrap();
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(mtime).unwrap();
    }
    let bodies: Vec<String> = open_messages(&f).iter().map(|m| m["message"].as_str().unwrap().to_string()).collect();
    assert_eq!(bodies, parts.iter().map(|(_, b)| b.to_string()).collect::<Vec<_>>());

    let drained = f.drain("plain");
    let at: Vec<usize> = parts.iter().map(|(_, b)| drained.find(b).unwrap_or_else(|| panic!("{b}: {drained}"))).collect();
    assert!(at.windows(2).all(|w| w[0] < w[1]), "in order: {drained}");
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
