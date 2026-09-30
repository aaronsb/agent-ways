//! JSON-RPC dispatch and the module registry.

use serde_json::{json, Value};

pub const SERVER_NAME: &str = "agent-ways";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A tool a module exposes. The name carries the module name, `ways_status`
/// or `attend_send`, so permission entries group by prefix (ADR-501 item 3).
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}

/// A tool's outcome: structured content, and whether it is an error the model
/// should read rather than a protocol failure.
pub struct ToolResult {
    pub content: Value,
    pub is_error: bool,
}

pub trait Module {
    fn name(&self) -> &'static str;
    /// This module's part of the server's `instructions` string.
    fn instructions(&self) -> &'static str;
    fn tools(&self) -> Vec<Tool>;
    /// Runs one of this module's tools. Every call derives what it needs
    /// afresh and keeps nothing for the next (ADR-187 item 5).
    fn call(&self, tool: &str, args: &Value, registry: &[&'static str]) -> Option<ToolResult>;
}

pub struct Server {
    modules: Vec<Box<dyn Module>>,
}

impl Server {
    pub fn new(modules: Vec<Box<dyn Module>>) -> Self {
        Self { modules }
    }

    fn module_names(&self) -> Vec<&'static str> {
        self.modules.iter().map(|m| m.name()).collect()
    }

    /// One line in, at most one line out. Notifications get no reply.
    pub fn handle_line(&self, line: &str) -> Option<String> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => return Some(error(Value::Null, -32700, "parse error").to_string()),
        };
        let id = msg.get("id").cloned()?; // a notification: nothing to answer
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
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

    fn initialize(&self, params: &Value) -> Value {
        let protocol = params
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or("2025-06-18");
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
        let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
        let registry = self.module_names();
        let result = self
            .modules
            .iter()
            .find_map(|m| m.call(name, &args, &registry))
            .unwrap_or_else(|| ToolResult {
                content: json!({ "error": format!("unknown tool: {name}") }),
                is_error: true,
            });
        ok(
            id,
            json!({
                "content": [{ "type": "text", "text": result.content.to_string() }],
                "structuredContent": result.content,
                "isError": result.is_error,
            }),
        )
    }
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

    fn server() -> Server {
        Server::new(crate::modules::all())
    }

    fn call(s: &Server, line: &str) -> Value {
        serde_json::from_str(&s.handle_line(line).expect("a reply")).unwrap()
    }

    #[test]
    fn initialize_declares_tools_and_the_channel_capability() {
        let r = call(&server(), r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#);
        let res = &r["result"];
        assert_eq!(res["protocolVersion"], "2025-06-18");
        assert_eq!(res["serverInfo"]["name"], "agent-ways");
        assert!(res["capabilities"]["tools"].is_object());
        assert!(res["capabilities"]["experimental"]["claude/channel"].is_object());
        assert!(res["instructions"].as_str().unwrap().contains("ways_status"));
    }

    #[test]
    fn notifications_get_no_reply() {
        assert!(server().handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
        assert!(server().handle_line("   ").is_none());
    }

    #[test]
    fn unknown_method_and_bad_json_are_errors() {
        let s = server();
        assert_eq!(call(&s, r#"{"jsonrpc":"2.0","id":7,"method":"nope"}"#)["error"]["code"], -32601);
        let r = call(&s, "{not json");
        assert_eq!(r["error"]["code"], -32700);
        assert!(r["id"].is_null());
    }

    #[test]
    fn tool_names_carry_their_module() {
        let s = server();
        let r = call(&s, r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
        let tools = r["result"]["tools"].as_array().unwrap();
        assert!(!tools.is_empty());
        for m in crate::modules::all() {
            for t in m.tools() {
                assert!(t.name.starts_with(&format!("{}_", m.name())), "{} lacks its module prefix", t.name);
            }
        }
    }

    #[test]
    fn unknown_tool_is_a_tool_error_not_a_protocol_error() {
        let r = call(&server(), r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"nope"}}"#);
        assert_eq!(r["result"]["isError"], true);
        assert!(r.get("error").is_none());
    }

    #[test]
    fn ways_status_reports_server_and_modules() {
        let r = call(&server(), r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"ways_status","arguments":{}}}"#);
        let sc = &r["result"]["structuredContent"];
        assert_eq!(r["result"]["isError"], false);
        assert_eq!(sc["server"], "agent-ways");
        assert_eq!(sc["version"], VERSION);
        assert!(sc["modules"].as_array().unwrap().iter().any(|m| m == "ways"));
        assert!(sc["session"].is_object());
    }
}
