// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The poll of an `mcp` sensor: one call of a read-only tool on a stdio MCP
//! server, its result handed to the file's triggers and declared state as
//! a command's JSON output is.
//!
//! The server is the file's inline `command`, `args` and `env`, or a server
//! of Claude Code's MCP configuration by name ([`lookup`]). Only stdio
//! servers run. A poll runs only when the sensor's probe record is bound to
//! the server's definition as it resolves now; a changed command, args or
//! env voids the record.
//!
//! A server lives across polls: one per distinct launch (program, args,
//! env, directory, environment rule and binding) in a process, shared by
//! every sensor that names it, started at the first poll that needs it,
//! and ended when the last sensor holding it is dropped, so a sensor host
//! keeps its servers for the life of its session. A server that fails, by
//! exiting, timing out or breaking the protocol, is ended at once and the
//! poll records why. The next poll starts it again.
//!
//! A server is ended in the order the MCP stdio transport gives: its input
//! is closed, then its process group is sent TERM, then KILL, each after a
//! wait for the leader to exit; a failed server gets shorter waits. Each
//! running server has an entry in the [`ledger`], so a host killed with
//! SIGKILL, which ends nothing, has its servers ended by the next host for
//! the project.
//!
//! Each step has a deadline: the start (spawn, `initialize` and the
//! `initialized` notice), the `tools/list` that follows it, and each
//! `tools/call`. A server runs in a process group of its own, and a missed
//! deadline ends the group, so nothing it started outlives it.
//!
//! The tool must declare `readOnlyHint: true` in `tools/list`. A file may
//! set `allow_unannotated: true` to call a tool that declares no hint; a
//! tool that declares `readOnlyHint: false` says it writes, and is refused
//! either way. A refusal is the poll's recorded failure.
//!
//! The server's environment is set by [`super::command::sensor_env`], the
//! rule every sensor child takes: a project's server, by a project file or
//! a `.mcp.json`, starts from the fixed project set.
//!
//! References:
//! - ADR-403 §4: the mcp kind, server lookup, the read-only check and
//!   stdio only.
//! - ADR-416 items 2 and 3: one host per session, each holding its own MCP
//!   sessions.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command as Process, Stdio};
use std::sync::{mpsc, Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use sensor_spec::McpBlock;
use serde_json::{json, Value};

use super::command::{kill_group, leader_exited, sensor_env, Inherit};
use super::hosted::{Fetched, Source};

pub mod ledger;
mod lookup;
pub use lookup::{inline_binding, resolve, Lookup};

/// How many messages from a server wait unread before its reader waits.
const BACKLOG: usize = 64;

/// The most one message from a server may hold, in bytes.
const MAX_MESSAGE: u64 = 1024 * 1024;

/// The protocol version attend asks for.
const PROTOCOL: &str = "2025-06-18";

/// The most `tools/list` pages read before the tool counts as missing.
const MAX_PAGES: usize = 50;

/// How long each step may take.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Spawning the server, `initialize` and the `initialized` notice.
    pub start: Duration,
    /// `tools/list`, all pages.
    pub list: Duration,
    /// One `tools/call`.
    pub call: Duration,
}

impl Default for Limits {
    /// Ten seconds a step, a command sensor's default timeout.
    fn default() -> Self {
        let step = Duration::from_secs(sensor_spec::bounds::COMMAND_TIMEOUT);
        Limits { start: step, list: step, call: step }
    }
}

/// How a server is started. Two sensors with the same launch share one
/// server.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Launch {
    /// The server's name in Claude Code's configuration, or its command.
    pub label: String,
    pub program: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: PathBuf,
    pub inherit: Inherit,
    /// Where the definition came from and a hash of it ([`lookup`]).
    pub binding: String,
}

/// How long an ending server is given to exit at each step: after its
/// input closes, and after TERM.
#[derive(Debug, Clone, Copy)]
enum Grace {
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
struct Session {
    launch: Launch,
    child: Child,
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
    ended: bool,
}

/// The servers of this process, by launch. Each is held by the sensors
/// using it; the list keeps none alive.
static SERVERS: Mutex<Vec<(Launch, Weak<Mutex<Session>>)>> = Mutex::new(Vec::new());

/// End every MCP server this process started, as a re-exec does before it
/// replaces the process: an exec runs no destructor, and a server that
/// ignores the end of its input would outlive the host.
pub fn end_servers() {
    let held: Vec<_> = SERVERS.lock().map(|mut l| l.drain(..).filter_map(|(_, w)| w.upgrade()).collect()).unwrap_or_default();
    for s in held {
        if let Ok(mut s) = s.lock() {
            s.end(Grace::Short);
        }
    }
}

impl Session {
    /// Spawn the server, then `initialize` and `tools/list` it, each step
    /// within its limit.
    fn start(launch: &Launch, limits: Limits, ledger: &Path) -> Result<Session, String> {
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
    fn gone(&mut self) -> String {
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
    fn end(&mut self, grace: Grace) -> Option<std::process::ExitStatus> {
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
    fn refuses(&self, tool: &str, allow_unannotated: bool) -> Option<String> {
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
    fn call(&mut self, tool: &str, args: &Value, limit: Duration) -> Result<Value, String> {
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

/// An mcp sensor's poll, for the project at `project`.
pub struct Mcp {
    /// The sensor's name.
    name: String,
    block: McpBlock,
    project: PathBuf,
    inherit: Inherit,
    lookup: Lookup,
    limits: Limits,
    /// Where the servers' entries are noted ([`ledger`]).
    ledger: PathBuf,
    /// The server this sensor last polled through.
    held: Option<Arc<Mutex<Session>>>,
}

impl Mcp {
    pub fn new(name: &str, block: McpBlock, project: &str, inherit: Inherit, lookup: Lookup) -> Mcp {
        Mcp { name: name.to_string(), block, project: PathBuf::from(project), inherit, lookup, limits: Limits::default(), ledger: ledger::dir(), held: None }
    }

    /// The same, with each step's limit set.
    #[cfg(test)]
    pub fn with_limits(mut self, limits: Limits) -> Mcp {
        self.limits = limits;
        self
    }

    /// The same, noting its servers under `dir`.
    #[cfg(test)]
    pub fn with_ledger(mut self, dir: &Path) -> Mcp {
        self.ledger = dir.to_path_buf();
        self
    }

    /// The running server for `launch`: the one this sensor holds, another
    /// sensor's, or a new one.
    fn server(&mut self, launch: &Launch) -> Result<Arc<Mutex<Session>>, String> {
        let live = |s: &Arc<Mutex<Session>>| s.lock().is_ok_and(|s| !s.ended && s.launch == *launch);
        if let Some(s) = self.held.as_ref().filter(|s| live(s)) {
            return Ok(Arc::clone(s));
        }
        self.held = None;
        let mut all = SERVERS.lock().map_err(|_| "the server list is poisoned".to_string())?;
        all.retain(|(_, w)| w.strong_count() > 0);
        if let Some(s) = all.iter().filter(|(l, _)| l == launch).find_map(|(_, w)| w.upgrade()).filter(live) {
            self.held = Some(Arc::clone(&s));
            return Ok(s);
        }
        let s = Arc::new(Mutex::new(Session::start(launch, self.limits, &self.ledger)?));
        all.push((launch.clone(), Arc::downgrade(&s)));
        self.held = Some(Arc::clone(&s));
        Ok(s)
    }

    /// One call of the tool, as a JSON result. Refused unless the probe
    /// record is bound to the server's definition as it resolves now.
    pub fn call(&mut self) -> Result<Value, String> {
        let launch = resolve(&self.block.server, &self.project, self.inherit, &self.lookup)?;
        if self.block.probe.as_ref().and_then(|p| p.binding.as_deref()) != Some(launch.binding.as_str()) {
            return Err(format!(
                "the probe record was not taken with server {} as it is defined now; run `attend sensor probe {}`",
                launch.label, self.name
            ));
        }
        self.call_on(&launch)
    }

    /// One call of the tool for `attend sensor probe`, which takes the
    /// record, so no record binds it: the result, and the binding of the
    /// server it ran on.
    pub fn probe_call(&mut self) -> Result<(Value, String), String> {
        let launch = resolve(&self.block.server, &self.project, self.inherit, &self.lookup)?;
        Ok((self.call_on(&launch)?, launch.binding))
    }

    fn call_on(&mut self, launch: &Launch) -> Result<Value, String> {
        let args = serde_json::to_value(&self.block.args).map_err(|e| format!("its args are not JSON: {e}"))?;
        let server = self.server(launch)?;
        let mut s = server.lock().map_err(|_| format!("server {} is poisoned", launch.label))?;
        if let Some(why) = s.refuses(&self.block.tool, self.block.allow_unannotated) {
            return Err(why);
        }
        if matches!(leader_exited(&mut s.child), Ok(true)) {
            return Err(s.gone());
        }
        s.call(&self.block.tool, &args, self.limits.call)
    }
}

impl Source for Mcp {
    fn fetch(&mut self) -> Result<Fetched, String> {
        self.call().map(Fetched::Json)
    }
}

#[cfg(all(test, unix))]
mod tests;
