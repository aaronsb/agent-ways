// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The background shells and watchers still in flight, as the last Stop
//! hook's `background_tasks` listed them. attend's Stop hook pipes its
//! payload to `attend message inbox --drain`, which records them here at
//! every Stop. While an exit is armed, `attend keepwarm check` counts them
//! as pending where the host's `--pending` names no shells or watchers.
//!
//! The record is the last Stop's. At an idle due point no turn has run
//! since, so it is the latest word there is. A shell that finishes after
//! the Stop stays counted until the next turn ends, which only keeps the
//! pings going longer, never ends the session early.
//!
//! Read and write it only through [`record`] and [`read`].
//!
//! References:
//! - ADR-418 item 5: agents, shells and watchers in flight are pending.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// What one Stop listed in flight: a one-line description of each shell
/// and each watcher, and when it was recorded, in epoch seconds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Background {
    #[serde(default)]
    pub shells: Vec<String>,
    #[serde(default)]
    pub watchers: Vec<String>,
    #[serde(default)]
    pub at: u64,
}

/// The longest description kept, in characters.
const LINE_MAX: usize = 200;

fn path(dir: &Path, session_id: &str) -> PathBuf {
    dir.join(format!("{session_id}.background.json"))
}

/// Records `b` as the session's background work in flight, replacing the
/// last record.
pub fn record(dir: &Path, session_id: &str, b: &Background) -> std::io::Result<()> {
    let text = serde_json::to_string(b).map_err(std::io::Error::other)?;
    crate::write_atomic(dir, &path(dir, session_id), &text)
}

/// The session's last record, or `None` where none was written or it does
/// not read.
pub fn read(dir: &Path, session_id: &str) -> Option<Background> {
    let text = std::fs::read_to_string(path(dir, session_id)).ok()?;
    serde_json::from_str(&text).ok()
}

/// The background work a Stop hook payload lists, read leniently: `None`
/// where the payload is no JSON object or carries no `background_tasks`
/// array, so a payload of another shape records nothing. A task of an
/// unknown type or one no longer running is left out. Shells are the
/// `shell` tasks, by their command line; watchers are the `monitor` tasks,
/// by their description.
pub fn from_stop_payload(payload: &str, now: u64) -> Option<Background> {
    let doc: serde_json::Value = serde_json::from_str(payload).ok()?;
    let tasks = doc.get("background_tasks")?.as_array()?;
    let mut b = Background { at: now, ..Background::default() };
    for t in tasks {
        let field = |k: &str| t.get(k).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty());
        if field("status").is_some_and(|s| matches!(s, "completed" | "failed" | "killed" | "stopped")) {
            continue;
        }
        let (list, text) = match field("type") {
            Some("shell") => (&mut b.shells, field("command").or(field("description"))),
            Some("monitor") => (&mut b.watchers, field("description").or(field("tool"))),
            _ => continue,
        };
        list.push(one_line(text.unwrap_or("(no description)")));
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

    #[test]
    fn a_running_shell_and_a_monitor_are_recorded_and_read_back() {
        let payload = r#"{"hook_event_name":"Stop","stop_hook_active":false,"background_tasks":[
            {"id":"b1","type":"shell","status":"running","description":"run the suite","command":"cargo test\n--all"},
            {"id":"m1","type":"monitor","status":"running","description":"watch CI on #1403","server":"gh","tool":"watch"},
            {"id":"a1","type":"subagent","status":"running","description":"review","agent_type":"Explore"},
            {"id":"b2","type":"shell","status":"completed","description":"done","command":"true"}]}"#;
        let b = from_stop_payload(payload, 42).unwrap();
        assert_eq!(b.shells, vec!["cargo test".to_string()]);
        assert_eq!(b.watchers, vec!["watch CI on #1403".to_string()]);
        assert_eq!(b.at, 42);
        let dir = std::env::temp_dir().join(format!("kw-bg-{}", std::process::id()));
        record(&dir, "s1", &b).unwrap();
        assert_eq!(read(&dir, "s1"), Some(b));
        assert_eq!(read(&dir, "s2"), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_empty_list_records_nothing_in_flight() {
        let b = from_stop_payload(r#"{"background_tasks":[]}"#, 7).unwrap();
        assert_eq!(b, Background { shells: vec![], watchers: vec![], at: 7 });
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
