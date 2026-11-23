// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The poll of an `mcp` sensor: one call of a read-only tool on a stdio MCP
//! server, its result handed to the file's triggers and declared state as
//! a command's JSON output is.
//!
//! The server is the file's inline `command`, `args` and `env`, or a server
//! of Claude Code's MCP configuration by name: the project's entry in
//! `~/.claude.json` first, then the project's `.mcp.json`, then the user's
//! entries in `~/.claude.json`. A `.mcp.json` server is code the
//! repository chose, so it starts only for a project in
//! `attend.project_sensors`. Only stdio servers run.
//!
//! A server lives across polls: one per distinct launch (program, args,
//! env, directory and environment rule) in a process, shared by every
//! sensor that names it, started at the first poll that needs it, and
//! ended when the last sensor holding it is dropped, so a sensor host
//! keeps its servers for the life of its session. A server that fails, by
//! exiting, timing out or breaking the protocol, is ended at once and the
//! poll records why. The next poll starts it again.
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

use sensor_spec::{McpBlock, Server};
use serde_json::{json, Value};

use super::command::{kill_group, leader_exited, sensor_env, Inherit};
use super::hosted::{Fetched, Source};

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
}

/// Where a named server is looked up.
#[derive(Debug, Clone)]
pub struct Lookup {
    /// Claude Code's `~/.claude.json`.
    pub user_json: PathBuf,
    /// Whether the project is in `attend.project_sensors`, which a
    /// `.mcp.json` server needs.
    pub trusted: bool,
}

impl Lookup {
    /// `~/.claude.json`, or `.claude.json` in `CLAUDE_CONFIG_DIR` when it is
    /// set, as Claude Code reads it.
    pub fn user_json() -> PathBuf {
        match std::env::var_os("CLAUDE_CONFIG_DIR").filter(|d| !d.is_empty()) {
            Some(d) => PathBuf::from(d).join(".claude.json"),
            None => claude_sessions::claude_json(),
        }
    }
}

/// The launch of `server` for the project at `project`, whose file's
/// environment rule is `inherit`. A named server's own definition decides
/// too: one from `.mcp.json` takes the project rule.
pub fn resolve(server: &Server, project: &Path, inherit: Inherit, lookup: &Lookup) -> Result<Launch, String> {
    let (label, def, inherit) = match server {
        Server::Inline { command, args, env } => {
            return Ok(Launch { label: command.clone(), program: command.clone(), args: args.clone(), env: env.clone(), cwd: project.to_path_buf(), inherit });
        }
        Server::Named(name) => {
            let user = read_json(&lookup.user_json)?;
            let key = project.to_string_lossy();
            let local = user.as_ref().and_then(|u| u.get("projects")?.get(key.as_ref())?.get("mcpServers")?.get(name)).cloned();
            let mcp_json = project.join(".mcp.json");
            let shared = read_json(&mcp_json)?.and_then(|p| p.get("mcpServers")?.get(name).cloned());
            let global = user.as_ref().and_then(|u| u.get("mcpServers")?.get(name)).cloned();
            match (local, shared, global) {
                (Some(d), _, _) => (name, d, inherit),
                (None, Some(_), _) if !lookup.trusted => {
                    return Err(format!(
                        "server {name} is defined in {}, code the repository chose, and the project is not in attend.project_sensors",
                        mcp_json.display()
                    ))
                }
                (None, Some(d), _) => (name, d, Inherit::Project),
                (None, None, Some(d)) => (name, d, inherit),
                (None, None, None) => {
                    return Err(format!("no MCP server named {name} in {} or {}", lookup.user_json.display(), mcp_json.display()));
                }
            }
        }
    };
    match def.get("type").and_then(Value::as_str) {
        None | Some("stdio") => {}
        Some(t) => return Err(format!("server {label} is a {t} server; attend runs stdio servers only")),
    }
    let Some(program) = def.get("command").and_then(Value::as_str).filter(|c| !c.is_empty()) else {
        return Err(format!("server {label} names no command"));
    };
    let args = def.get("args").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let env = def.get("env").and_then(Value::as_object).map(|m| m.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect()).unwrap_or_default();
    Ok(Launch { label: label.clone(), program: program.to_string(), args, env, cwd: project.to_path_buf(), inherit })
}

/// The JSON document at `path`; none when there is no file.
fn read_json(path: &Path) -> Result<Option<Value>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map(Some).map_err(|e| format!("{} is not JSON: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("cannot read {}: {e}", path.display())),
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
    stdin: ChildStdin,
    rx: mpsc::Receiver<Incoming>,
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
            s.end();
        }
    }
}

impl Session {
    /// Spawn the server, then `initialize` and `tools/list` it, each step
    /// within its limit.
    fn start(launch: &Launch, limits: Limits) -> Result<Session, String> {
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
        let (tx, rx) = mpsc::channel();
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
        let mut s = Session { launch: launch.clone(), child, stdin, rx, said, said_all, next_id: 0, tools: BTreeMap::new(), ended: false };
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
        if self.stdin.write_all(line.as_bytes()).and_then(|_| self.stdin.flush()).is_err() {
            return Err(self.gone());
        }
        Ok(())
    }

    /// Send `method` and wait until `deadline` for its answer. A request the
    /// server makes meanwhile is answered: `ping` with an empty result, any
    /// other with method-not-found. Notifications are read and dropped.
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
        let status = self.end();
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

    /// End the server and return `why`.
    fn fail(&mut self, why: String) -> String {
        self.end();
        why
    }

    /// End the server's process group and reap it, once. The leader is
    /// reaped only here, after the group kill, so its group id stays
    /// reserved until then.
    fn end(&mut self) -> Option<std::process::ExitStatus> {
        if self.ended {
            return None;
        }
        self.ended = true;
        kill_group(&mut self.child);
        self.child.wait().ok()
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
        self.end();
    }
}

/// Pass each line of a server's stdout on as a message, until it closes
/// or sends what is not one.
fn read_messages(out: impl Read, tx: mpsc::Sender<Incoming>) {
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
    block: McpBlock,
    project: PathBuf,
    inherit: Inherit,
    lookup: Lookup,
    limits: Limits,
    /// The server this sensor last polled through.
    held: Option<Arc<Mutex<Session>>>,
}

impl Mcp {
    pub fn new(block: McpBlock, project: &str, inherit: Inherit, lookup: Lookup) -> Mcp {
        Mcp { block, project: PathBuf::from(project), inherit, lookup, limits: Limits::default(), held: None }
    }

    /// The same, with each step's limit set.
    #[cfg(test)]
    pub fn with_limits(mut self, limits: Limits) -> Mcp {
        self.limits = limits;
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
        let s = Arc::new(Mutex::new(Session::start(launch, self.limits)?));
        all.push((launch.clone(), Arc::downgrade(&s)));
        self.held = Some(Arc::clone(&s));
        Ok(s)
    }

    /// One call of the tool, as a JSON result.
    pub fn call(&mut self) -> Result<Value, String> {
        let launch = resolve(&self.block.server, &self.project, self.inherit, &self.lookup)?;
        let args = serde_json::to_value(&self.block.args).map_err(|e| format!("its args are not JSON: {e}"))?;
        let server = self.server(&launch)?;
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
mod tests {
    use super::*;

    fn fixture() -> String {
        format!("{}/tests/fixtures/mcp-server.sh", env!("CARGO_MANIFEST_DIR"))
    }

    /// A scratch directory for one test.
    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("attend-mcp-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// An mcp sensor on the fixture server in `mode`, calling `tool` on the
    /// result file `items` holds, its pids noted in `dir`.
    fn sensor(dir: &Path, mode: &str, tool: &str, allow_unannotated: bool) -> Mcp {
        let items = dir.join("items.json");
        if !items.exists() {
            std::fs::write(&items, r#"{"items":[{"id":"a"}],"count":1}"#).unwrap();
        }
        let server = Server::Inline {
            command: "sh".into(),
            args: vec![fixture(), mode.into()],
            env: BTreeMap::from([("FIXTURE_PIDS".to_string(), dir.join("pids").display().to_string())]),
        };
        let mut args = serde_yaml::Mapping::new();
        args.insert("file".into(), items.display().to_string().into());
        let block = McpBlock { server, tool: tool.into(), args, allow_unannotated, probe: None };
        let lookup = Lookup { user_json: dir.join("no-claude.json"), trusted: false };
        Mcp::new(block, dir.to_str().unwrap(), Inherit::Stripped, lookup)
    }

    fn quick(m: Mcp) -> Mcp {
        let step = Duration::from_secs(1);
        m.with_limits(Limits { start: step, list: step, call: step })
    }

    fn pids(dir: &Path) -> Vec<String> {
        std::fs::read_to_string(dir.join("pids")).unwrap_or_default().lines().map(str::to_string).collect()
    }

    fn alive(pid: &str) -> bool {
        Process::new("kill").args(["-0", pid]).stderr(Stdio::null()).status().unwrap().success()
    }

    /// Wait up to two seconds for every pid in `dir` to be gone, and name
    /// any that is not.
    fn left_behind(dir: &Path) -> Vec<String> {
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            let live: Vec<String> = pids(dir).into_iter().filter(|p| alive(p)).collect();
            if live.is_empty() || Instant::now() > until {
                return live;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn a_read_only_tool_s_structured_result_is_the_poll_result() {
        let dir = scratch("structured");
        let mut m = sensor(&dir, "serve", "list_items", false);
        let Ok(Fetched::Json(v)) = m.fetch() else { panic!("{:?}", m.call()) };
        assert_eq!(v, json!({"items": [{"id": "a"}], "count": 1}));
        std::fs::write(dir.join("items.json"), r#"{"items":[],"count":0}"#).unwrap();
        assert_eq!(m.call().unwrap(), json!({"items": [], "count": 0}), "the next poll calls again");
        assert_eq!(pids(&dir).len(), 1, "one server across polls");
        drop(m);
        assert_eq!(left_behind(&dir), Vec::<String>::new(), "dropping the last sensor ends its server");
    }

    #[test]
    fn a_text_result_that_is_json_is_read_as_json() {
        let dir = scratch("text");
        let mut m = sensor(&dir, "serve", "text_items", false);
        assert_eq!(m.call().unwrap(), json!({"items": [{"id": "a"}], "count": 1}));
    }

    /// A tool with no `readOnlyHint` is refused unless the file allows it,
    /// and one that declares `readOnlyHint: false` is refused either way.
    #[test]
    fn a_tool_not_declared_read_only_is_refused() {
        let dir = scratch("readonly");
        let refused = sensor(&dir, "serve", "bare", false).call().unwrap_err();
        assert_eq!(refused, "tool bare does not declare readOnlyHint: true, so attend does not call it; allow_unannotated: true calls it anyway");
        assert!(sensor(&dir, "serve", "bare", true).call().is_ok(), "allow_unannotated calls it");
        for allow in [false, true] {
            assert_eq!(sensor(&dir, "serve", "mark_read", allow).call().unwrap_err(), "tool mark_read declares readOnlyHint: false, so attend does not call it");
        }
        assert_eq!(sensor(&dir, "serve", "nope", true).call().unwrap_err(), "server sh has no tool nope");
    }

    /// A server that answers nothing fails the poll at its start limit,
    /// and neither it nor what it started is left running.
    #[test]
    fn a_server_that_hangs_times_out_and_leaves_no_process() {
        let dir = scratch("hang");
        let started = Instant::now();
        let why = quick(sensor(&dir, "hang", "list_items", false)).call().unwrap_err();
        assert_eq!(why, "server sh timed out after 1s at initialize");
        assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
        assert_eq!(pids(&dir).len(), 2, "the server and its child started");
        assert_eq!(left_behind(&dir), Vec::<String>::new());
    }

    /// A call that never returns fails the poll at the call limit and ends
    /// the server's group; the next poll starts it again.
    #[test]
    fn a_call_that_hangs_times_out_and_ends_the_server() {
        let dir = scratch("hang-call");
        let mut m = quick(sensor(&dir, "hang-call", "list_items", false));
        assert_eq!(m.call().unwrap_err(), "server sh timed out after 1s at tools/call");
        assert_eq!(left_behind(&dir), Vec::<String>::new());
        let before = pids(&dir).len();
        assert!(m.call().is_err());
        assert!(pids(&dir).len() > before, "the next poll started a new server");
        assert_eq!(left_behind(&dir), Vec::<String>::new());
    }

    #[test]
    fn a_server_that_crashes_says_how() {
        let dir = scratch("crash");
        assert_eq!(sensor(&dir, "crash", "list_items", false).call().unwrap_err(), "server sh exited with status 3: fixture crashed on purpose");
        assert_eq!(left_behind(&dir), Vec::<String>::new());
    }

    /// A server that exits between polls fails the next poll, which says
    /// so, and the poll after starts it again.
    #[test]
    fn a_server_that_exits_is_restarted_on_the_next_poll() {
        let dir = scratch("once");
        let mut m = sensor(&dir, "once", "list_items", false);
        assert!(m.call().is_ok());
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(m.call().unwrap_err(), "server sh exited with status 0");
        assert!(m.call().is_ok(), "restarted");
        assert_eq!(pids(&dir).len(), 2);
    }

    /// Two sensors on one launch share its server.
    #[test]
    fn sensors_on_one_server_share_it() {
        let dir = scratch("shared");
        let (mut a, mut b) = (sensor(&dir, "serve", "list_items", false), sensor(&dir, "serve", "text_items", false));
        assert!(a.call().is_ok() && b.call().is_ok());
        assert_eq!(pids(&dir).len(), 1);
        drop(a);
        assert!(b.call().is_ok(), "the other sensor keeps it");
        assert_eq!(pids(&dir).len(), 1);
        drop(b);
        assert_eq!(left_behind(&dir), Vec::<String>::new(), "the last sensor's drop ends it");
    }

    #[test]
    fn a_missing_program_cannot_start() {
        let dir = scratch("missing");
        let mut m = sensor(&dir, "serve", "list_items", false);
        m.block.server = Server::Inline { command: "/nonexistent/mcp-server".into(), args: vec![], env: BTreeMap::new() };
        assert!(m.call().unwrap_err().starts_with("cannot start server /nonexistent/mcp-server: "));
    }

    /// A named server is the project's entry in `~/.claude.json`, then the
    /// project's `.mcp.json` when the project is trusted, then the user's
    /// entry; only a stdio server runs.
    #[test]
    fn a_named_server_is_looked_up_as_claude_code_does() {
        let dir = scratch("lookup");
        let project = dir.join("proj");
        std::fs::create_dir_all(&project).unwrap();
        let user_json = dir.join("claude.json");
        let p = project.display().to_string();
        std::fs::write(
            &user_json,
            json!({
                "mcpServers": { "u": { "command": "user-u" }, "both": { "command": "user-both" }, "web": { "type": "http", "url": "https://example.invalid" } },
                "projects": { p.clone(): { "mcpServers": { "local": { "type": "stdio", "command": "local-cmd", "args": ["-x"], "env": { "K": "v" } } } } }
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(project.join(".mcp.json"), json!({ "mcpServers": { "both": { "command": "repo-both" } } }).to_string()).unwrap();
        let at = |name: &str, trusted: bool| resolve(&Server::Named(name.into()), &project, Inherit::Stripped, &Lookup { user_json: user_json.clone(), trusted });
        let local = at("local", false).unwrap();
        assert_eq!((local.program.as_str(), local.args.clone(), local.env.get("K").map(String::as_str), local.inherit), ("local-cmd", vec!["-x".to_string()], Some("v"), Inherit::Stripped));
        assert_eq!(at("u", false).unwrap().program, "user-u");
        let repo = at("both", true).unwrap();
        assert_eq!((repo.program.as_str(), repo.inherit), ("repo-both", Inherit::Project), "a .mcp.json server takes the project rule");
        assert!(at("both", false).unwrap_err().contains("the project is not in attend.project_sensors"));
        assert_eq!(at("web", false).unwrap_err(), "server web is a http server; attend runs stdio servers only");
        assert!(at("none", false).unwrap_err().starts_with("no MCP server named none in "));
    }
}
