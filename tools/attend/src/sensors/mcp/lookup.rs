// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! Where an mcp sensor's server comes from: the file's inline definition,
//! or a server of Claude Code's MCP configuration by name.

use std::path::{Path, PathBuf};

use sensor_spec::Server;
use serde_json::Value;

use super::super::command::Inherit;
use super::Launch;

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
