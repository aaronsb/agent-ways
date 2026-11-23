// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! One running MCP server and its session: its start, the requests, the
//! read-only check of a tool, and its end.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command as Process, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::super::command::{kill_group, leader_exited, sensor_env};
use super::{ledger, Launch, Limits};

/// How many messages from a server wait unread before its reader waits.
const BACKLOG: usize = 64;

/// The most one message from a server may hold, in bytes.
const MAX_MESSAGE: u64 = 1024 * 1024;

/// The protocol version attend asks for.
const PROTOCOL: &str = "2025-06-18";

/// The most `tools/list` pages read before the tool counts as missing.
const MAX_PAGES: usize = 50;

/// How long an ending server is given to exit at each step: after its
/// input closes, and after TERM.
#[derive(Debug, Clone, Copy)]
pub(super) enum Grace {
    /// A server ended because nothing uses it.
    Normal,
    /// A server that already failed.
    Short,
}

impl Grace {
    fn waits(self) -> (Duration, Duration) {
        match self {
            Grace::Normal => (Duration::from_secs(1), Duration::from_secs(1)),
            Grace::Short => (Duration::from_millis(100), Duration::from_millis(250)),
        }
    }
}

/// What the reader thread passes on from a server's stdout.
enum Incoming {
    Message(Value),
    /// A line that is not one JSON-RPC message, or is too long.
    Bad(String),
    /// stdout closed.
    Closed,
}

/// One running server and its session.
pub(super) struct Session {
    pub(super) launch: Launch,
    pub(super) child: Child,
    /// The server's input; none once it is closed.
    stdin: Option<ChildStdin>,
    rx: mpsc::Receiver<Incoming>,
    /// The server's entry in the ledger, removed when it ends.
    noted: Option<PathBuf>,
    /// The last line the server wrote on stderr.
    said: Arc<Mutex<String>>,
    /// Set when the server's stderr closed.
    said_all: Arc<std::sync::atomic::AtomicBool>,
    next_id: u64,
    /// What `tools/list` returned, by tool name.
    tools: BTreeMap<String, Value>,
    /// Set once the session failed and its server was ended.
    pub(super) ended: bool,
}

impl Session {
    /// Spawn the server, then `initialize` and `tools/list` it, each step
    /// within its limit.
    pub(super) fn start(launch: &Launch, limits: Limits, ledger: &Path) -> Result<Session, String> {
        let mut cmd = Process::new(&launch.program);
        sensor_env(&mut cmd, launch.inherit, &launch.env);
        cmd.args(&launch.args).current_dir(&launch.cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        let mut child = cmd.spawn().map_err(|e| format!("cannot start server {}: {e}", launch.label))?;
        let (Some(stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take()) else {
            kill_group(&mut child);
            child.wait().ok();
            return Err(format!("cannot start server {}: no pipes", launch.label));
        };
        let noted = ledger::note(ledger, &launch.cwd, child.id());
        let (tx, rx) = mpsc::sync_channel(BACKLOG);
        std::thread::spawn(move || read_messages(stdout, tx));
        let said = Arc::new(Mutex::new(String::new()));
        let said_all = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (last, closed) = (Arc::clone(&said), Arc::clone(&said_all));
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                let line = line.trim();
                if !line.is_empty() {
                    if let Ok(mut l) = last.lock() {
                        *l = line.chars().take(200).collect();
                    }
                }
            }
            closed.store(true, std::sync::atomic::Ordering::Release);
        });
        let mut s = Session { launch: launch.clone(), child, stdin: Some(stdin), rx, noted, said, said_all, next_id: 0, tools: BTreeMap::new(), ended: false };
        let deadline = Instant::now() + limits.start;
        let init = json!({ "protocolVersion": PROTOCOL, "capabilities": {}, "clientInfo": { "name": "attend", "version": env!("CARGO_PKG_VERSION") } });
        s.request("initialize", init, deadline, limits.start)?;
        s.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))?;
        s.list(limits.list)?;
        Ok(s)
    }

    /// Read every page of `tools/list` into [`Session::tools`].
    fn list(&mut self, limit: Duration) -> Result<(), String> {
        let deadline = Instant::now() + limit;
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let params = match &cursor {
                Some(c) => json!({ "cursor": c }),
                None => json!({}),
            };
            let page = self.request("tools/list", params, deadline, limit)?;
            for t in page.get("tools").and_then(Value::as_array).into_iter().flatten() {
                if let Some(name) = t.get("name").and_then(Value::as_str) {
                    self.tools.insert(name.to_string(), t.clone());
                }
            }
            cursor = page.get("nextCursor").and_then(Value::as_str).filter(|c| !c.is_empty()).map(str::to_string);
            if cursor.is_none() {
                break;
            }
        }
        Ok(())
    }

    /// Write one message as a line. A write to a server that has gone
    /// returns an error rather than raising SIGPIPE.
    fn send(&mut self, msg: &Value) -> Result<(), String> {
        let _quiet = PipeQuiet::new();
        let mut line = msg.to_string();
        line.push('\n');
        let sent = self.stdin.as_mut().is_some_and(|i| i.write_all(line.as_bytes()).and_then(|_| i.flush()).is_ok());
        if !sent {
            return Err(self.gone());
        }
        Ok(())
    }

    /// Send `method` and wait until `deadline` for its answer. A request the
    /// server makes meanwhile is answered: `ping` with an empty result, any
    /// other with method-not-found. The reader drops notifications.
    fn request(&mut self, method: &str, params: Value, deadline: Instant, limit: Duration) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let msg = match self.rx.recv_timeout(left) {
                Ok(Incoming::Message(m)) => m,
                Ok(Incoming::Bad(why)) => return Err(self.fail(format!("server {} broke the protocol: {why}", self.launch.label))),
                Ok(Incoming::Closed) | Err(mpsc::RecvTimeoutError::Disconnected) => return Err(self.gone()),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return Err(self.fail(format!("server {} timed out after {}s at {method}", self.launch.label, limit.as_secs_f64())));
                }
            };
            match (msg.get("id"), msg.get("method")) {
                (Some(i), None) if i.as_u64() == Some(id) => {
                    if let Some(e) = msg.get("error") {
                        let text = e.get("message").and_then(Value::as_str).unwrap_or("no message");
                        return Err(format!("server {} refused {method}: {text}", self.launch.label));
                    }
                    return Ok(msg.get("result").cloned().unwrap_or(Value::Null));
                }
                (Some(i), Some(m)) => {
                    let answer = match m.as_str() {
                        Some("ping") => json!({ "jsonrpc": "2.0", "id": i, "result": {} }),
                        _ => json!({ "jsonrpc": "2.0", "id": i, "error": { "code": -32601, "message": "attend answers no requests" } }),
                    };
                    self.send(&answer)?;
                }
                _ => {}
            }
        }
    }

    /// The server's stdout closed or its stdin broke: it exited, or is
    /// about to. Wait a moment for the exit, end it, and say how it ended.
    pub(super) fn gone(&mut self) -> String {
        let until = Instant::now() + Duration::from_millis(500);
        while Instant::now() < until && !matches!(leader_exited(&mut self.child), Ok(true)) {
            std::thread::sleep(Duration::from_millis(10));
        }
        let status = self.end(Grace::Short);
        // The group is gone, so stderr closes unless something outside it
        // holds it; its last line is read for a moment.
        let until = Instant::now() + Duration::from_millis(200);
        while Instant::now() < until && !self.said_all.load(std::sync::atomic::Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(5));
        }
        let how = status.map_or("exited".to_string(), |s| match s.code() {
            Some(c) => format!("exited with status {c}"),
            None => "was ended by a signal".to_string(),
        });
        let said = self.said.lock().map(|s| s.clone()).unwrap_or_default();
        match said.is_empty() {
            true => format!("server {} {how}", self.launch.label),
            false => format!("server {} {how}: {said}", self.launch.label),
        }
    }

    /// End the failed server and return `why`.
    fn fail(&mut self, why: String) -> String {
        self.end(Grace::Short);
        why
    }

    /// End the server, once: close its input and wait for the leader to
    /// exit, then TERM its process group and wait again, then KILL the
    /// group and reap the leader. The group is signalled even when the
    /// leader exited, so what it started ends too. The leader is reaped
    /// only after the KILL, so its group id stays reserved until then.
    pub(super) fn end(&mut self, grace: Grace) -> Option<std::process::ExitStatus> {
        if self.ended {
            return None;
        }
        self.ended = true;
        let (eof, term) = grace.waits();
        self.stdin = None;
        let exited = |child: &mut Child, wait: Duration| {
            let until = Instant::now() + wait;
            while !matches!(leader_exited(child), Ok(true)) && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        exited(&mut self.child, eof);
        #[cfg(unix)]
        // SAFETY: kill(2) takes no pointers; the unreaped leader keeps the
        // group id ours.
        unsafe {
            libc::kill(-(self.child.id() as libc::pid_t), libc::SIGTERM);
        }
        exited(&mut self.child, term);
        kill_group(&mut self.child);
        let status = self.child.wait().ok();
        if let Some(p) = self.noted.take() {
            ledger::forget(&p);
        }
        status
    }

    /// Why `tool` may not be called on this server, if it may not.
    pub(super) fn refuses(&self, tool: &str, allow_unannotated: bool) -> Option<String> {
        let Some(t) = self.tools.get(tool) else { return Some(format!("server {} has no tool {tool}", self.launch.label)) };
        match t.get("annotations").and_then(|a| a.get("readOnlyHint")) {
            Some(Value::Bool(true)) => None,
            Some(Value::Bool(false)) => Some(format!("tool {tool} declares readOnlyHint: false, so attend does not call it")),
            _ if allow_unannotated => None,
            _ => Some(format!("tool {tool} does not declare readOnlyHint: true, so attend does not call it; allow_unannotated: true calls it anyway")),
        }
    }

    /// Call `tool` with `args` and return its result as the triggers read
    /// it: the `structuredContent`, else the first text item when it is
    /// JSON, else the result as the server sent it.
    pub(super) fn call(&mut self, tool: &str, args: &Value, limit: Duration) -> Result<Value, String> {
        let result = self.request("tools/call", json!({ "name": tool, "arguments": args }), Instant::now() + limit, limit)?;
        let text = result.get("content").and_then(Value::as_array).and_then(|c| c.iter().find(|i| i.get("type").and_then(Value::as_str) == Some("text"))).and_then(|i| i.get("text")?.as_str());
        if result.get("isError") == Some(&Value::Bool(true)) {
            let first = text.and_then(|t| t.lines().map(str::trim).find(|l| !l.is_empty())).unwrap_or("no message");
            return Err(format!("tool {tool} returned an error: {first}"));
        }
        if let Some(s) = result.get("structuredContent") {
            return Ok(s.clone());
        }
        Ok(text.and_then(|t| serde_json::from_str(t).ok()).unwrap_or(result))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.end(Grace::Normal);
    }
}

/// Pass each answer and request on a server's stdout on as a message,
/// until it closes or sends what is not one. A notification carries no
/// id, needs no answer, and is dropped here, so a server that sends them
/// between polls fills nothing. The channel is bounded: a reader with a
/// full backlog waits, and so does the server writing to it.
fn read_messages(out: impl Read, tx: mpsc::SyncSender<Incoming>) {
    let mut r = BufReader::new(out);
    loop {
        let mut buf = Vec::new();
        match (&mut r).take(MAX_MESSAGE + 1).read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => {
                tx.send(Incoming::Closed).ok();
                return;
            }
            Ok(_) => {}
        }
        if buf.len() as u64 > MAX_MESSAGE {
            tx.send(Incoming::Bad(format!("a message over {} KiB", MAX_MESSAGE / 1024))).ok();
            return;
        }
        let line = String::from_utf8_lossy(&buf);
        if line.trim().is_empty() {
            continue;
        }
        let msg = match serde_json::from_str::<Value>(line.trim()) {
            Ok(v) if v.is_object() && v.get("id").is_none() => continue,
            Ok(v) if v.is_object() => Incoming::Message(v),
            _ => Incoming::Bad(format!("a line that is not a JSON-RPC message: {}", line.trim().chars().take(80).collect::<String>())),
        };
        let bad = matches!(msg, Incoming::Bad(_));
        if tx.send(msg).is_err() || bad {
            return;
        }
    }
}

/// SIGPIPE ignored while it lives, and put back as it was after. A write
/// to a server that exited then fails with an error, where the default
/// would end attend.
struct PipeQuiet {
    #[cfg(unix)]
    was: libc::sighandler_t,
}

impl PipeQuiet {
    fn new() -> PipeQuiet {
        PipeQuiet {
            // SAFETY: signal(2) takes no pointers; the previous disposition
            // is restored on drop.
            #[cfg(unix)]
            was: unsafe { libc::signal(libc::SIGPIPE, libc::SIG_IGN) },
        }
    }
}

impl Drop for PipeQuiet {
    fn drop(&mut self) {
        // SAFETY: as in new.
        #[cfg(unix)]
        unsafe {
            libc::signal(libc::SIGPIPE, self.was);
        }
    }
}
