//! The core module: what the server itself reports.

use super::lookup;
use crate::server::{Context, Module, Tool, ToolResult, SERVER_NAME, VERSION};
use crate::session;
use serde_json::{json, Value};

pub struct Ways;

impl Module for Ways {
    fn name(&self) -> &'static str {
        "ways"
    }

    fn instructions(&self) -> &'static str {
        "The agent-ways server hosts agent-ways modules. `ways_status` reports its version, this session, \
         whether the session was launched with the server's channel flag, and which modules are live. \
         `ways_search`, `ways_read` and `ways_neighbors` look ways up on request: search ranks them for a \
         query, read returns one, which the PostToolUse hook then marks disclosed so injection does not repeat it, neighbors lists \
         what surrounds one."
    }

    fn tools(&self) -> Vec<Tool> {
        let mut tools = vec![Tool {
            name: "ways_status",
            description: "Report the agent-ways server: its version, the Claude Code session it serves, \
                          whether that session was launched naming this server under \
                          --dangerously-load-development-channels (channel_flag; null when the launch \
                          command cannot be read), and the live modules. Read-only.",
            input_schema: json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        }];
        tools.extend(lookup::tools());
        tools
    }

    fn call(&self, tool: &str, args: &Value, ctx: &Context) -> ToolResult {
        if lookup::handles(tool) {
            return lookup::Lookup::current().call(tool, args);
        }
        let content = json!({
            "server": SERVER_NAME,
            "version": VERSION,
            "session": session::describe(),
            "modules": ctx.modules,
        });
        let Value::Object(content) = content else { unreachable!("a json! object") };
        ToolResult { content, is_error: false }
    }
}
