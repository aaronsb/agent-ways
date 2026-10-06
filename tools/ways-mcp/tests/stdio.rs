//! Drives the built binary over stdio in the order Claude Code sends.

use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn handshake_list_and_call_over_stdio() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ways-mcp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn ways-mcp");
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"ways_status","arguments":{}}}"#,
    ];
    child.stdin.take().unwrap().write_all((input.join("\n") + "\n").as_bytes()).unwrap();
    let out = child.wait_with_output().expect("ways-mcp exits when stdin closes");
    assert!(out.status.success());

    let replies: Vec<Value> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).expect("each line is one JSON-RPC message"))
        .collect();
    // Three requests, one notification: three replies, in order.
    let ids: Vec<i64> = replies.iter().map(|r| r["id"].as_i64().unwrap()).collect();
    assert_eq!(ids, [1, 2, 3]);
    assert!(replies[0]["result"]["capabilities"]["experimental"]["claude/channel"].is_object());
    let listed: Vec<&str> = replies[1]["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(listed, ["ways_status", "ways_search", "ways_read", "ways_neighbors"]);
    assert_eq!(replies[2]["result"]["structuredContent"]["server"], "agent-ways");
}

/// A lookup tool over the wire, against a stand-in `ways` named by `WAYS_BIN`:
/// the result is the JSON `ways lookup` printed, as structured content, and a
/// failure comes back as `isError`.
#[cfg(unix)]
#[test]
fn a_lookup_tool_returns_what_ways_lookup_prints() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("ways-mcp-stdio-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let stub = dir.join("ways");
    std::fs::write(
        &stub,
        r#"#!/bin/sh
case "$*" in
  *" read "*) printf '{"error":"way d/off is disabled"}\n'; exit 1 ;;
  *) printf '{"lane":"en","candidates":[{"way":"d/w","route":"d > w","cosine":0.5,"share":0.9,"margin":0.1}]}\n' ;;
esac
"#,
    )
    .unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_ways-mcp"))
        .env("WAYS_BIN", &stub)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn ways-mcp");
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ways_search","arguments":{"query":"commit"}}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"ways_read","arguments":{"id":"d/off"}}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"ways_read","arguments":{}}}"#,
    ];
    child.stdin.take().unwrap().write_all((input.join("\n") + "\n").as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    let replies: Vec<Value> = String::from_utf8(out.stdout).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();

    let found = &replies[0]["result"];
    assert_eq!(found["isError"], false);
    assert_eq!(found["structuredContent"]["candidates"][0]["way"], "d/w");
    assert!(found["structuredContent"].get("session").is_some(), "the result says which session it acted for");
    assert_eq!(replies[1]["result"]["isError"], true);
    assert_eq!(replies[1]["result"]["structuredContent"]["error"], "way d/off is disabled");
    assert_eq!(replies[2]["result"]["isError"], true, "a missing id never reaches ways");
    let _ = std::fs::remove_dir_all(&dir);
}
