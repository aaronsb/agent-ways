//! The core module: what the server itself reports.

use crate::server::{Module, Tool, ToolResult, SERVER_NAME, VERSION};
use crate::session;
use serde_json::{json, Value};

pub struct Ways;

impl Module for Ways {
    fn name(&self) -> &'static str {
        "ways"
    }

    fn instructions(&self) -> &'static str {
        "The agent-ways server hosts agent-ways modules. `ways_status` reports its version, this session, \
         whether the session loaded the server's channel, and which modules are live."
    }

    fn tools(&self) -> Vec<Tool> {
        vec![Tool {
            name: "ways_status",
            description: "Report the agent-ways server: its version, the Claude Code session it serves, \
                          whether that session was launched with this server as a development channel \
                          (channel_loaded; null when it cannot be read), and the live modules. Read-only.",
            input_schema: json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        }]
    }

    fn call(&self, tool: &str, _args: &Value, registry: &[&'static str]) -> Option<ToolResult> {
        (tool == "ways_status").then(|| ToolResult {
            content: json!({
                "server": SERVER_NAME,
                "version": VERSION,
                "session": session::describe(),
                "modules": registry,
            }),
            is_error: false,
        })
    }
}
