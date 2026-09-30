//! ways-mcp — the agent-ways MCP server (ADR-501).
//!
//! Claude Code launches one per session over stdio and registers it as
//! `agent-ways`. The server hosts modules; each contributes tools and may push
//! channel events. It speaks the handshake Claude Code sends (`initialize`,
//! then `notifications/initialized`) as newline-delimited JSON-RPC, and
//! declares the `claude/channel` capability so a session launched with the
//! development-channels flag receives those events (ADR-402).
//!
//! One writer thread owns stdout. Replies and module pushes reach it through
//! the same queue, so a line is never interleaved with another.

mod modules;
mod server;
mod session;

use std::io::{self, BufRead, Write};
use std::sync::mpsc;

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("ways-mcp {}", server::VERSION);
        return;
    }

    let (tx, rx) = mpsc::channel::<Option<String>>();
    let writer = std::thread::spawn(move || {
        let mut out = io::stdout().lock();
        // `None` is the shutdown marker: every line queued before it is written.
        while let Ok(Some(line)) = rx.recv() {
            if writeln!(out, "{line}").and_then(|_| out.flush()).is_err() {
                break;
            }
        }
    });

    let server = server::Server::new(modules::all(), server::Outbox::new(tx.clone()));
    let mut input = io::stdin().lock();
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match input.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let reply = match std::str::from_utf8(&buf) {
            Ok(line) => server.handle_line(line),
            Err(_) => Some(server::parse_error()),
        };
        if let Some(reply) = reply {
            if tx.send(Some(reply)).is_err() {
                break;
            }
        }
    }
    let _ = tx.send(None);
    let _ = writer.join();
}
