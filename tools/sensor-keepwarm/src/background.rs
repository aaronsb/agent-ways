// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The background work still in flight, as the last Stop hook's
//! `background_tasks` listed it: shells, watchers, and agents. attend's
//! Stop hook pipes its payload to `attend message inbox --drain`, which
//! records it here at every Stop. While an exit is armed, `attend keepwarm
//! check` counts it as pending where the host's `--pending` names no count
//! of that kind, and `attend keepwarm brief` lists it.
//!
//! The record is the last Stop's. At an idle due point no turn has run
//! since, so it is the latest word there is. A shell that finishes after
//! the Stop stays counted until the next turn ends, which only keeps the
//! pings going longer, never ends the session early.
//!
//! A Stop that lists nothing in flight leaves no file, so a session's
//! record stands only while its work runs, and no record reads as a last
//! Stop with nothing in flight.
//!
//! Read and write it only through [`record`] and [`read`].
//!
//! References:
//! - ADR-418 item 5: agents, shells and watchers in flight are pending.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// What one Stop listed in flight: a one-line description of each shell,
/// each watcher and each agent, and when it was recorded, in epoch
/// seconds. An agent line names its kind, `subagent: <description>`,
/// `teammate: …`, `cloud session: …` or `workflow: <name>`; a workflow
/// orchestrates agents, so it counts as one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Background {
    #[serde(default)]
    pub shells: Vec<String>,
    #[serde(default)]
    pub watchers: Vec<String>,
    /// Read from `workflows` too, the name a record before the subagents,
    /// teammates and cloud sessions were counted gave it.
    #[serde(default, alias = "workflows")]
    pub agents: Vec<String>,
    #[serde(default)]
    pub at: u64,
}

impl Background {
    /// Nothing in flight.
    pub fn is_empty(&self) -> bool {
        self.shells.is_empty() && self.watchers.is_empty() && self.agents.is_empty()
    }
}

/// The longest description kept, in characters.
const LINE_MAX: usize = 200;

fn path(dir: &Path, session_id: &str) -> PathBuf {
    dir.join(format!("{session_id}.background.json"))
}

/// Records `b` as the session's background work in flight, replacing the
/// last record; one with nothing in flight removes it and writes none.
pub fn record(dir: &Path, session_id: &str, b: &Background) -> std::io::Result<()> {
    let path = path(dir, session_id);
    if b.is_empty() {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let text = serde_json::to_string(b).map_err(std::io::Error::other)?;
    crate::write_atomic(dir, &path, &text)
}

/// The session's last record, or `None` where none was written, the last
/// Stop listed nothing, or it does not read.
pub fn read(dir: &Path, session_id: &str) -> Option<Background> {
    let text = std::fs::read_to_string(path(dir, session_id)).ok()?;
    serde_json::from_str(&text).ok()
}

/// The background work a Stop hook payload lists, read leniently: `None`
/// where the payload is no JSON object or carries no `background_tasks`
/// array, so a payload of another shape records nothing. A task no longer
/// running is left out. The types are those Claude Code 2.1.332 writes:
///
/// - shells: `shell`, by its command line;
/// - watchers: `monitor` and `MCP task`, by their description;
/// - agents: `subagent`, `teammate` and `cloud session` by their
///   description, and `workflow` by its name, each line led by its type.
///
/// `dream`, `auto-mode scan` and `memory save` are the host's own
/// housekeeping, which no one waits on, and a type this does not know is
/// left out too.
pub fn from_stop_payload(payload: &str, now: u64) -> Option<Background> {
    let doc: serde_json::Value = serde_json::from_str(payload).ok()?;
    let tasks = doc.get("background_tasks")?.as_array()?;
    let mut b = Background { at: now, ..Background::default() };
    for t in tasks {
        let field = |k: &str| t.get(k).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty());
        if field("status").is_some_and(|s| matches!(s, "completed" | "failed" | "killed" | "stopped")) {
            continue;
        }
        let described = || field("description").or(field("name"));
        let (list, kind, text) = match field("type") {
            Some("shell") => (&mut b.shells, None, field("command").or(field("description"))),
            Some("monitor") => (&mut b.watchers, None, field("description").or(field("tool"))),
            Some("MCP task") => (&mut b.watchers, None, field("description").or(field("tool"))),
            Some(kind @ ("subagent" | "teammate" | "cloud session")) => (&mut b.agents, Some(kind), described()),
            Some("workflow") => (&mut b.agents, Some("workflow"), field("name").or(field("description"))),
            // The host's own work, which keeps nothing of the session's waiting.
            Some("dream" | "auto-mode scan" | "memory save") => continue,
            _ => continue,
        };
        let text = text.unwrap_or("(no description)");
        list.push(one_line(&match kind {
            Some(k) => format!("{k}: {text}"),
            None => text.to_string(),
        }));
    }
    Some(b)
}

fn one_line(s: &str) -> String {
    let first = s.lines().next().unwrap_or("").trim();
    if first.chars().count() <= LINE_MAX {
        return first.to_string();
    }
    let mut cut: String = first.chars().take(LINE_MAX - 1).collect();
    cut.push('…');
    cut
}


#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kw-bg-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        d
    }

    #[test]
    fn a_running_shell_and_a_monitor_are_recorded_and_read_back() {
        let payload = r#"{"hook_event_name":"Stop","stop_hook_active":false,"background_tasks":[
            {"id":"b1","type":"shell","status":"running","description":"run the suite","command":"cargo test\n--all"},
            {"id":"m1","type":"monitor","status":"running","description":"watch CI on #1403","server":"gh","tool":"watch"},
            {"id":"a1","type":"subagent","status":"running","description":"review","agent_type":"Explore"},
            {"id":"w1","type":"workflow","status":"running","description":"build the wave","name":"wave3"},
            {"id":"b2","type":"shell","status":"completed","description":"done","command":"true"}]}"#;
        let b = from_stop_payload(payload, 42).unwrap();
        assert_eq!(b.shells, vec!["cargo test".to_string()]);
        assert_eq!(b.watchers, vec!["watch CI on #1403".to_string()]);
        assert_eq!(b.agents, vec!["subagent: review".to_string(), "workflow: wave3".to_string()]);
        assert_eq!(b.at, 42);
        let dir = dir("roundtrip");
        record(&dir, "s1", &b).unwrap();
        assert_eq!(read(&dir, "s1"), Some(b));
        assert_eq!(read(&dir, "s2"), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// One task of each type the 2.1.332 serializer writes: shells and
    /// monitors, MCP tasks as watchers, subagents, teammates, cloud
    /// sessions and workflows as agents, and the host's own housekeeping
    /// left out, as is a type this does not know.
    #[test]
    fn each_task_type_lands_in_its_bucket() {
        let payload = r#"{"background_tasks":[
            {"type":"subagent","status":"running","description":"review the diff","agent_type":"Explore"},
            {"type":"workflow","status":"running","name":"wave3"},
            {"type":"shell","status":"running","command":"cargo build"},
            {"type":"monitor","status":"running","description":"watch CI"},
            {"type":"MCP task","status":"running","description":"index the docs","server":"kg","tool":"ingest"},
            {"type":"teammate","status":"running","description":"scout@crew"},
            {"type":"cloud session","status":"running","description":"port the parser"},
            {"type":"dream","status":"running","description":"consolidate"},
            {"type":"auto-mode scan","status":"running","description":"classify"},
            {"type":"memory save","status":"running","description":"save"},
            {"type":"something new","status":"running","description":"unknown"}]}"#;
        let b = from_stop_payload(payload, 1).unwrap();
        assert_eq!(b.shells, vec!["cargo build".to_string()]);
        assert_eq!(b.watchers, vec!["watch CI".to_string(), "index the docs".to_string()]);
        assert_eq!(
            b.agents,
            vec![
                "subagent: review the diff".to_string(),
                "workflow: wave3".to_string(),
                "teammate: scout@crew".to_string(),
                "cloud session: port the parser".to_string(),
            ]
        );
    }

    #[test]
    fn an_empty_list_records_nothing_in_flight() {
        let b = from_stop_payload(r#"{"background_tasks":[]}"#, 7).unwrap();
        assert_eq!(b, Background { at: 7, ..Background::default() });
        assert!(b.is_empty());
    }

    /// A Stop that lists nothing in flight removes the last record and
    /// writes none, so a session's file stands only while work runs, and no
    /// file reads as an empty last Stop.
    #[test]
    fn an_empty_record_removes_the_file_and_writes_none() {
        let dir = dir("empty");
        let running = from_stop_payload(r#"{"background_tasks":[{"type":"shell","status":"running","command":"sleep 9"}]}"#, 1).unwrap();
        record(&dir, "s1", &running).unwrap();
        assert!(path(&dir, "s1").exists());
        record(&dir, "s1", &from_stop_payload(r#"{"background_tasks":[]}"#, 2).unwrap()).unwrap();
        assert!(!path(&dir, "s1").exists(), "the empty Stop removes the record");
        assert_eq!(read(&dir, "s1"), None);
        // With no record on file, an empty Stop writes none and is no error.
        record(&dir, "s2", &Background { at: 3, ..Background::default() }).unwrap();
        assert!(!path(&dir, "s2").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A record written before the bucket was named `agents` reads its
    /// workflows as agents.
    #[test]
    fn an_older_record_s_workflows_read_as_agents() {
        let b: Background = serde_json::from_str(r#"{"shells":[],"watchers":[],"workflows":["wave3"],"at":1}"#).unwrap();
        assert_eq!(b.agents, vec!["wave3".to_string()]);
    }

    #[test]
    fn a_payload_of_another_shape_records_nothing() {
        for p in ["", "not json", "[]", r#"{"stop_hook_active":true}"#, r#"{"background_tasks":"x"}"#, r#"{"background_tasks":null}"#] {
            assert_eq!(from_stop_payload(p, 1), None, "{p}");
        }
        // Items of an unknown shape are left out, not fatal.
        let b = from_stop_payload(r#"{"background_tasks":[1,"x",{"type":7},{"type":"shell"}]}"#, 1).unwrap();
        assert_eq!(b.shells, vec!["(no description)".to_string()]);
    }

    #[test]
    fn a_long_line_is_cut() {
        let long = "x".repeat(500);
        assert_eq!(one_line(&long).chars().count(), LINE_MAX);
    }
}
