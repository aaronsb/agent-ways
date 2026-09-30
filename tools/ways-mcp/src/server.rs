//! JSON-RPC dispatch and the module registry.

use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::sync::Once;

pub const SERVER_NAME: &str = "agent-ways";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Protocol revisions this server implements, newest first. The client's
/// requested revision is accepted when listed; otherwise the newest listed one
/// is offered. 2026-07-28 is absent on purpose: Claude Code does not register
/// a channel server that connects on it (ADR-501 item 7).
pub const SUPPORTED_PROTOCOLS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// The one channel method a module may push (ADR-402).
const CHANNEL_METHOD: &str = "notifications/claude/channel";

/// A tool a module exposes. The name carries the module name, `ways_status`
/// or `attend_send`, so permission entries group by prefix (ADR-501 item 3).
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}

/// A tool's outcome. `content` is the structured result, an object as the
/// protocol requires; `is_error` marks a failure the model should read.
pub struct ToolResult {
    pub content: Map<String, Value>,
    pub is_error: bool,
}

/// What a tool call may consult beyond its arguments.
pub struct Context<'a> {
    pub modules: &'a [&'static str],
}

/// The queue to the stdout writer. A module receives one at start and pushes
/// channel events through it; replies share the queue, so lines never interleave.
#[derive(Clone)]
pub struct Outbox(Sender<Option<String>>);

impl Outbox {
    pub fn new(tx: Sender<Option<String>>) -> Self {
        Self(tx)
    }

    /// Push a channel event: `content` becomes the event body and each `meta`
    /// entry an attribute on the `<channel>` tag. Returns false once the
    /// server is shutting down.
    #[allow(dead_code)] // first used by the keepalive module
    pub fn push(&self, content: &str, meta: &[(&str, &str)]) -> bool {
        let meta: Map<String, Value> = meta.iter().map(|(k, v)| ((*k).to_string(), json!(v))).collect();
        let msg = json!({ "jsonrpc": "2.0", "method": CHANNEL_METHOD, "params": { "content": content, "meta": meta } });
        self.0.send(Some(msg.to_string())).is_ok()
    }
}

pub trait Module: Send + Sync {
    fn name(&self) -> &'static str;
    /// This module's part of the server's `instructions` string.
    fn instructions(&self) -> &'static str;
    fn tools(&self) -> Vec<Tool>;
    /// Runs one of this module's tools; the server routes by name. Every call
    /// derives what it needs afresh and keeps nothing for the next (ADR-187
    /// item 5).
    fn call(&self, tool: &str, args: &Value, ctx: &Context) -> ToolResult;
    /// Called once, after the client's `notifications/initialized`. A module
    /// that pushes events starts its work here; events sent before the client
    /// is initialized would be lost.
    fn start(&self, _outbox: Outbox) {}
}

pub struct Server {
    modules: Vec<Box<dyn Module>>,
    names: Vec<&'static str>,
    routes: HashMap<&'static str, usize>,
    outbox: Outbox,
    started: Once,
}

impl Server {
    /// Panics when two tools share a name: a registry defect, caught by the
    /// test suite before it ships.
    pub fn new(modules: Vec<Box<dyn Module>>, outbox: Outbox) -> Self {
        let mut routes = HashMap::new();
        for (i, m) in modules.iter().enumerate() {
            for t in m.tools() {
                if let Some(prev) = routes.insert(t.name, i) {
                    panic!("tool {} is defined by both {} and {}", t.name, modules[prev].name(), m.name());
                }
            }
        }
        let names = modules.iter().map(|m| m.name()).collect();
        Self { modules, names, routes, outbox, started: Once::new() }
    }

    /// One line in, at most one line out. Notifications and responses get no reply.
    pub fn handle_line(&self, line: &str) -> Option<String> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        let Ok(msg) = serde_json::from_str::<Value>(line) else {
            return Some(parse_error());
        };
        let method = msg.get("method").and_then(Value::as_str)?; // a response to us: nothing to answer
        let id = match msg.get("id") {
            Some(id) if !id.is_null() => id.clone(),
            _ => {
                self.notification(method);
                return None;
            }
        };
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let reply = match method {
            "initialize" => ok(id, self.initialize(&params)),
            "ping" => ok(id, json!({})),
            "tools/list" => ok(id, self.tools_list()),
            "tools/call" => self.tools_call(id, &params),
            _ => error(id, -32601, "method not found"),
        };
        Some(reply.to_string())
    }

    fn notification(&self, method: &str) {
        if method == "notifications/initialized" {
            self.started.call_once(|| {
                for m in &self.modules {
                    m.start(self.outbox.clone());
                }
            });
        }
    }

    fn initialize(&self, params: &Value) -> Value {
        let requested = params.get("protocolVersion").and_then(Value::as_str);
        let protocol = requested.filter(|v| SUPPORTED_PROTOCOLS.contains(v)).unwrap_or(SUPPORTED_PROTOCOLS[0]);
        let instructions: Vec<&str> = self.modules.iter().map(|m| m.instructions()).collect();
        json!({
            "protocolVersion": protocol,
            "capabilities": {
                "tools": {},
                "experimental": { "claude/channel": {} },
            },
            "serverInfo": { "name": SERVER_NAME, "version": VERSION },
            "instructions": instructions.join(" "),
        })
    }

    fn tools_list(&self) -> Value {
        let tools: Vec<Value> = self
            .modules
            .iter()
            .flat_map(|m| m.tools())
            .map(|t| json!({ "name": t.name, "description": t.description, "inputSchema": t.input_schema }))
            .collect();
        json!({ "tools": tools })
    }

    fn tools_call(&self, id: Value, params: &Value) -> Value {
        let name = params.get("name").and_then(Value::as_str).unwrap_or("");
        let Some(&i) = self.routes.get(name) else {
            return error(id, -32602, &format!("unknown tool: {name}"));
        };
        let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
        let result = self.modules[i].call(name, &args, &Context { modules: &self.names });
        let content = Value::Object(result.content);
        ok(
            id,
            json!({
                "content": [{ "type": "text", "text": content.to_string() }],
                "structuredContent": content,
                "isError": result.is_error,
            }),
        )
    }
}

pub fn parse_error() -> String {
    error(Value::Null, -32700, "parse error").to_string()
}

fn ok(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{self, Receiver};

    fn server() -> (Server, Receiver<Option<String>>) {
        let (tx, rx) = mpsc::channel();
        (Server::new(crate::modules::all(), Outbox::new(tx)), rx)
    }

    fn call(s: &Server, line: &str) -> Value {
        serde_json::from_str(&s.handle_line(line).expect("a reply")).unwrap()
    }

    fn init(version: &str) -> String {
        format!(r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"{version}"}}}}"#)
    }

    #[test]
    fn initialize_declares_tools_and_the_channel_capability() {
        let (s, _) = server();
        let res = &call(&s, &init("2025-06-18"))["result"];
        assert_eq!(res["protocolVersion"], "2025-06-18");
        assert_eq!(res["serverInfo"]["name"], "agent-ways");
        assert!(res["capabilities"]["tools"].is_object());
        assert!(res["capabilities"]["experimental"]["claude/channel"].is_object());
        assert!(res["instructions"].as_str().unwrap().contains("ways_status"));
    }

    #[test]
    fn protocol_is_negotiated_against_the_supported_list() {
        let (s, _) = server();
        assert_eq!(call(&s, &init("2025-03-26"))["result"]["protocolVersion"], "2025-03-26");
        // The revision Claude Code won't register a channel server on.
        assert_eq!(call(&s, &init("2026-07-28"))["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(call(&s, r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#)["result"]["protocolVersion"], "2025-06-18");
    }

    #[test]
    fn notifications_responses_and_blank_lines_get_no_reply() {
        let (s, _) = server();
        for line in [
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":null,"method":"notifications/cancelled"}"#,
            r#"{"jsonrpc":"2.0","id":9,"result":{}}"#,
            "   ",
        ] {
            assert!(s.handle_line(line).is_none(), "{line}");
        }
    }

    #[test]
    fn unknown_method_and_bad_json_are_errors() {
        let (s, _) = server();
        assert_eq!(call(&s, r#"{"jsonrpc":"2.0","id":7,"method":"nope"}"#)["error"]["code"], -32601);
        let r: Value = serde_json::from_str(&parse_error()).unwrap();
        assert_eq!(call(&s, "{not json"), r);
        assert!(r["id"].is_null());
        assert_eq!(r["error"]["code"], -32700);
    }

    #[test]
    fn unknown_tool_is_invalid_params() {
        let (s, _) = server();
        let r = call(&s, r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"nope"}}"#);
        assert_eq!(r["error"]["code"], -32602);
    }

    #[test]
    fn tool_names_carry_their_module() {
        for m in crate::modules::all() {
            for t in m.tools() {
                assert!(t.name.starts_with(&format!("{}_", m.name())), "{} lacks its module prefix", t.name);
            }
        }
    }

    struct Dup;
    impl Module for Dup {
        fn name(&self) -> &'static str {
            "dup"
        }
        fn instructions(&self) -> &'static str {
            ""
        }
        fn tools(&self) -> Vec<Tool> {
            vec![Tool { name: "ways_status", description: "", input_schema: json!({}) }]
        }
        fn call(&self, _: &str, _: &Value, _: &Context) -> ToolResult {
            ToolResult { content: Map::new(), is_error: false }
        }
    }

    #[test]
    #[should_panic(expected = "tool ways_status is defined by both ways and dup")]
    fn duplicate_tool_names_are_refused() {
        let (tx, _rx) = mpsc::channel();
        let mut modules = crate::modules::all();
        modules.push(Box::new(Dup));
        Server::new(modules, Outbox::new(tx));
    }

    struct Pusher;
    impl Module for Pusher {
        fn name(&self) -> &'static str {
            "pusher"
        }
        fn instructions(&self) -> &'static str {
            ""
        }
        fn tools(&self) -> Vec<Tool> {
            vec![]
        }
        fn call(&self, _: &str, _: &Value, _: &Context) -> ToolResult {
            unreachable!()
        }
        fn start(&self, outbox: Outbox) {
            outbox.push("hello", &[("kind", "test")]);
        }
    }

    #[test]
    fn modules_start_once_after_initialized_and_push_channel_events() {
        let (tx, rx) = mpsc::channel();
        let s = Server::new(vec![Box::new(Pusher)], Outbox::new(tx));
        s.handle_line(&init("2025-06-18"));
        assert!(rx.try_recv().is_err(), "nothing is pushed before initialized");
        let initialized = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        s.handle_line(initialized);
        s.handle_line(initialized);
        let event: Value = serde_json::from_str(&rx.try_recv().unwrap().unwrap()).unwrap();
        assert_eq!(event["method"], "notifications/claude/channel");
        assert_eq!(event["params"]["content"], "hello");
        assert_eq!(event["params"]["meta"]["kind"], "test");
        assert!(rx.try_recv().is_err(), "start runs once");
    }

    #[test]
    fn ways_status_returns_an_object() {
        let (s, _) = server();
        let r = call(&s, r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"ways_status","arguments":{}}}"#);
        let sc = &r["result"]["structuredContent"];
        assert_eq!(r["result"]["isError"], false);
        assert_eq!(sc["server"], "agent-ways");
        assert_eq!(sc["version"], VERSION);
        assert_eq!(sc["modules"], json!(["ways"]));
        for key in ["session_id", "claude_pid", "channel_flag", "origin_path"] {
            assert!(sc["session"].get(key).is_some(), "session.{key} is reported");
        }
    }
}
