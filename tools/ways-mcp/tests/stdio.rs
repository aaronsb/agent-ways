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
    assert!(replies[1]["result"]["tools"].as_array().unwrap().iter().any(|t| t["name"] == "ways_status"));
    assert_eq!(replies[2]["result"]["structuredContent"]["server"], "agent-ways");
}
