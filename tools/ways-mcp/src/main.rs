//! ways-mcp — the agent-ways MCP server (ADR-501).
//!
//! Claude Code launches one per session over stdio and registers it as
//! `agent-ways`. The server hosts modules; each contributes tools and, later,
//! inbound channel events. It speaks the handshake Claude Code sends
//! (`initialize`, then `notifications/initialized`), as newline-delimited
//! JSON-RPC, and declares the `claude/channel` capability so a session launched
//! with the development-channels flag can receive events (ADR-402).

mod modules;
mod server;
mod session;

use std::io::{self, BufRead, Write};

fn main() {
    let server = server::Server::new(modules::all());
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if let Some(reply) = server.handle_line(&line) {
            if writeln!(stdout, "{reply}").and_then(|_| stdout.flush()).is_err() {
                break;
            }
        }
    }
}
