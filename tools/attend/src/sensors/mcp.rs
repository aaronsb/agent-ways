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
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use sensor_spec::McpBlock;
use serde_json::Value;

use super::command::{leader_exited, Inherit};
use super::hosted::{Fetched, Source};

pub mod ledger;
mod lookup;
mod session;
use session::{Grace, Session};
pub use lookup::{inline_binding, resolve, Lookup};

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
    pub fn with_ledger(mut self, dir: &std::path::Path) -> Mcp {
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
