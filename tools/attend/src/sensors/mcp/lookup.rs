// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! Where an mcp sensor's server comes from: the file's inline definition,
//! or a server of Claude Code's MCP configuration by name.
//!
//! A named server is the project's entry in `~/.claude.json` first, then
//! the project's `.mcp.json`, then the user's entry in `~/.claude.json`,
//! as Claude Code orders them. `.mcp.json` is read only when the project's
//! entry misses, and a malformed one fails the lookup only when the server
//! could come from nowhere else.
//!
//! `${NAME}` and `${NAME:-default}` in a `~/.claude.json` entry's command,
//! args and env expand from attend's environment, as Claude Code expands
//! them; a reference to an unset name with no default fails the lookup and
//! names it. A repository's `.mcp.json` entry is code and configuration
//! the repository chose, so attend expands nothing in it, and an entry
//! with a reference fails the lookup naming it.
//!
//! Each launch carries a binding: where its definition came from and a
//! hash of the definition as written, before expansion. A probe record
//! names the binding it was taken with, so a changed command, args or env
//! voids the record, and the record holds no value of the definition.
//!
//! References:
//! - ADR-403 §4: server lookup, stdio only, and the probe record.

use std::path::{Path, PathBuf};

use sensor_spec::Server;
use serde_json::{json, Value};

use super::super::command::Inherit;
use super::Launch;

/// How a variable is read: its value, or none when it is unset.
pub type Vars = fn(&str) -> Option<String>;

/// Where a named server is looked up.
#[derive(Debug, Clone)]
pub struct Lookup {
    /// Claude Code's `~/.claude.json`.
    pub user_json: PathBuf,
    /// Whether the project is in `attend.project_sensors`, which a
    /// `.mcp.json` server needs.
    pub trusted: bool,
    /// Where `${NAME}` in a `~/.claude.json` entry reads from.
    pub vars: Vars,
}

impl Lookup {
    /// The lookup for a project, trusted or not: Claude Code's user file,
    /// with variables read from attend's environment.
    pub fn new(trusted: bool) -> Lookup {
        Lookup { user_json: Lookup::user_json(), trusted, vars: |k| std::env::var(k).ok() }
    }

    /// `~/.claude.json`, or `.claude.json` in `CLAUDE_CONFIG_DIR` when it is
    /// set, as Claude Code reads it.
    pub fn user_json() -> PathBuf {
        match std::env::var_os("CLAUDE_CONFIG_DIR").filter(|d| !d.is_empty()) {
            Some(d) => PathBuf::from(d).join(".claude.json"),
            None => claude_sessions::claude_json(),
        }
    }
}

/// Where a named server's definition was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    /// The project's entry in `~/.claude.json`.
    Local,
    /// The project's `.mcp.json`.
    Project,
    /// The user's entry in `~/.claude.json`.
    User,
}

impl Scope {
    fn word(self) -> &'static str {
        match self {
            Scope::Local => "local",
            Scope::Project => "project",
            Scope::User => "user",
        }
    }
}

/// The binding of an inline server: its command, args and env, hashed.
/// A named server's binding needs its definition, which only
/// [`resolve`] reads.
pub fn inline_binding(server: &Server) -> Option<String> {
    match server {
        Server::Inline { command, args, env } => Some(binding("inline", &json!({ "command": command, "args": args, "env": env }))),
        Server::Named(_) => None,
    }
}

/// `scope:` and a hash of `def` as written.
fn binding(scope: &str, def: &Value) -> String {
    format!("{scope}:{:016x}", agent_identity::identity::fnv1a_64(def.to_string().as_bytes()))
}

/// The launch of `server` for the project at `project`, whose file's
/// environment rule is `inherit`. A named server's own definition decides
/// too: one from `.mcp.json` takes the project rule.
pub fn resolve(server: &Server, project: &Path, inherit: Inherit, lookup: &Lookup) -> Result<Launch, String> {
    let label = match server {
        Server::Inline { command, args, env } => {
            let binding = inline_binding(server).unwrap_or_default();
            return Ok(Launch { label: command.clone(), program: command.clone(), args: args.clone(), env: env.clone(), cwd: project.to_path_buf(), inherit, binding });
        }
        Server::Named(name) => name,
    };
    let (scope, def) = find(label, project, lookup)?;
    let inherit = if scope == Scope::Project { Inherit::Project } else { inherit };
    match def.get("type").and_then(Value::as_str) {
        None | Some("stdio") => {}
        Some(t) => return Err(format!("server {label} is a {t} server; attend runs stdio servers only")),
    }
    let text = |v: &Value| -> Result<String, String> {
        let s = v.as_str().unwrap_or_default();
        match scope {
            Scope::Project => match first_reference(s) {
                Some(whole) => Err(format!("server {label} in the project's .mcp.json uses {whole}; attend does not expand variables for a repository's server")),
                None => Ok(s.to_string()),
            },
            _ => expand(s, lookup.vars).map_err(|name| format!("server {label} uses ${{{name}}}, which is not set in attend's environment, so it is left unexpanded")),
        }
    };
    let Some(command) = def.get("command").filter(|c| c.as_str().is_some_and(|c| !c.is_empty())) else {
        return Err(format!("server {label} names no command"));
    };
    let program = text(command)?;
    let args = def.get("args").and_then(Value::as_array).into_iter().flatten().filter(|v| v.is_string()).map(&text).collect::<Result<Vec<_>, _>>()?;
    let env = def.get("env").and_then(Value::as_object).into_iter().flatten().filter(|(_, v)| v.is_string()).map(|(k, v)| Ok((k.clone(), text(v)?))).collect::<Result<_, String>>()?;
    Ok(Launch { label: label.clone(), program, args, env, cwd: project.to_path_buf(), inherit, binding: binding(scope.word(), &def) })
}

/// The definition of the server `name` and where it was found.
fn find(name: &str, project: &Path, lookup: &Lookup) -> Result<(Scope, Value), String> {
    let user = read_json(&lookup.user_json)?;
    let key = project.to_string_lossy();
    if let Some(d) = user.as_ref().and_then(|u| u.get("projects")?.get(key.as_ref())?.get("mcpServers")?.get(name)) {
        return Ok((Scope::Local, d.clone()));
    }
    let mcp_json = project.join(".mcp.json");
    let global = user.as_ref().and_then(|u| u.get("mcpServers")?.get(name)).cloned();
    match read_json(&mcp_json) {
        Ok(shared) => match shared.as_ref().and_then(|p| p.get("mcpServers")?.get(name)) {
            Some(_) if !lookup.trusted => Err(format!(
                "server {name} is defined in {}, code the repository chose, and the project is not in attend.project_sensors",
                mcp_json.display()
            )),
            Some(d) => Ok((Scope::Project, d.clone())),
            None => global.map(|d| (Scope::User, d)).ok_or_else(|| format!("no MCP server named {name} in {} or {}", lookup.user_json.display(), mcp_json.display())),
        },
        // As Claude Code does, a malformed .mcp.json leaves the user's
        // servers usable; it fails only a server nothing else defines.
        Err(why) => global.map(|d| (Scope::User, d)).ok_or_else(|| format!("no MCP server named {name} in {}, and {why}", lookup.user_json.display())),
    }
}

/// The first `${...}` reference in `s`, as written.
fn first_reference(s: &str) -> Option<&str> {
    let at = s.find("${")?;
    let len = s[at..].find('}')?;
    Some(&s[at..at + len + 1])
}

/// `s` with each `${NAME}` replaced by the variable's value, and each
/// `${NAME:-default}` by its value or, when it is unset, the default; the
/// name of the first reference that has neither.
fn expand(s: &str, vars: Vars) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = s;
    while let Some(at) = rest.find("${") {
        let Some(len) = rest[at..].find('}') else { break };
        out.push_str(&rest[..at]);
        let inner = &rest[at + 2..at + len];
        let (name, default) = match inner.split_once(":-") {
            Some((n, d)) => (n, Some(d)),
            None => (inner, None),
        };
        out.push_str(&vars(name).or(default.map(str::to_string)).ok_or_else(|| name.to_string())?);
        rest = &rest[at + len + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// The JSON document at `path`; none when there is no file.
fn read_json(path: &Path) -> Result<Option<Value>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map(Some).map_err(|e| format!("{} is not JSON: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("cannot read {}: {e}", path.display())),
    }
}
