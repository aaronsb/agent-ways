//! Firing-event log reader (ADR-151 §1).
//!
//! Reads the append-only JSONL log of way-firing events and aggregates it.
//! Shared engine: the compliance tooling cross-references claims against how
//! often a way actually fires, and the reader belongs in the library rather
//! than any one binary.

use serde_json::Value;
use std::collections::HashMap;

/// Load all firing events, one JSON object per line, from the events log.
///
/// A missing or unreadable log is not an error — it contributes nothing (a fresh
/// install simply has no firing history yet).
pub fn load_events() -> Vec<Value> {
    crate::paths::events_log_sources()
        .iter()
        .filter_map(|path| crate::event_archive::read_source(path))
        .flat_map(|content| {
            content
                .lines()
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect::<Vec<Value>>()
        })
        .collect()
}

/// Raw text of the events log, archives and all, oldest first.
///
/// The line-oriented timeline reconstruction of `ways session` works over raw
/// JSONL lines rather than parsed values, so it needs the text, not
/// [`load_events`]'s `Vec<Value>`. This reads all history: use it for the
/// global views (stats, tuning). A reader after one session wants
/// [`load_events_text_for_session`].
pub fn load_events_text() -> String {
    load_events_text_until(|_| false)
}

/// The text of the sources a session can be in, oldest first. Sources are read
/// newest first and reading stops after the first one that holds the session's
/// `session_start`: every older source predates the session. A session with no
/// `session_start` on record reads everything.
pub fn load_events_text_for_session(session_id: &str) -> String {
    load_events_text_until(|text| has_session_start(text, |v| v["session"].as_str() == Some(session_id)))
}

/// The parsed events of one session, read as [`load_events_text_for_session`]
/// reads them.
pub fn load_events_for_session(session_id: &str) -> Vec<Value> {
    events_for_session_from(&crate::paths::events_log_sources(), &mut |p| crate::event_archive::read_source(p), session_id)
}

fn events_for_session_from(sources: &[std::path::PathBuf], read: &mut dyn FnMut(&std::path::Path) -> Option<String>, session_id: &str) -> Vec<Value> {
    let text = newest_first_text(sources, read, &|t| has_session_start(t, |v| v["session"].as_str() == Some(session_id)));
    text.lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
}

/// The text a reader needs to find and show a session: the one named, or when
/// none is, the newest in `scope` (every project when `None`). Reading stops
/// after the newest source holding a `session_start` at the scope's own
/// project, since a session in a worktree under it only stands in when the
/// project has none.
pub fn load_events_text_for(session: Option<&str>, scope: Option<&str>) -> String {
    match session {
        Some(id) => load_events_text_for_session(id),
        None => load_events_text_until(|text| has_session_start(text, |v| scope.is_none_or(|sc| { let p = v["project"].as_str().unwrap_or(""); crate::util::in_project(p, sc) && crate::util::in_project(sc, p) }))),
    }
}

/// The text back to the newest `session_start` of `project`, oldest first.
pub fn load_events_text_for_project_sessions(project: &str) -> String {
    load_events_text_until(|text| has_session_start(text, |v| v["project"].as_str() == Some(project)))
}

/// Read the sources newest first, stopping after the first whose text
/// satisfies `stop`; return what was read oldest first.
pub fn load_events_text_until(stop: impl Fn(&str) -> bool) -> String {
    newest_first_text(&crate::paths::events_log_sources(), &mut |p| crate::event_archive::read_source(p), &stop)
}

fn newest_first_text(sources: &[std::path::PathBuf], read: &mut dyn FnMut(&std::path::Path) -> Option<String>, stop: &dyn Fn(&str) -> bool) -> String {
    let mut newest_first: Vec<String> = Vec::new();
    for path in sources.iter().rev() {
        let Some(text) = read(path) else { continue };
        let done = stop(&text);
        newest_first.push(text);
        if done {
            break;
        }
    }
    if newest_first.len() == 1 {
        return newest_first.pop().unwrap_or_default();
    }
    let mut out = String::with_capacity(newest_first.iter().map(|t| t.len() + 1).sum());
    for text in newest_first.iter().rev() {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(text);
    }
    out
}

/// Whether `text` has a `session_start` event that `matches`.
fn has_session_start(text: &str, matches: impl Fn(&Value) -> bool) -> bool {
    text.lines()
        .filter(|l| l.contains("session_start"))
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .any(|v| v["event"].as_str() == Some("session_start") && matches(&v))
}

/// Count `way_fired` events per way ID across the given events.
pub fn count_fires(events: &[Value]) -> HashMap<String, u64> {
    let mut counts: HashMap<String, u64> = HashMap::new();
    for event in events {
        if event["event"].as_str() == Some("way_fired") {
            if let Some(way) = event["way"].as_str() {
                *counts.entry(way.to_string()).or_default() += 1;
            }
        }
    }
    counts
}

#[cfg(test)]
mod reader_tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn start(session: &str) -> String {
        format!("{{\"ts\":\"2026-01-01T00:00:00Z\",\"event\":\"session_start\",\"session\":\"{session}\",\"project\":\"/p\"}}\n")
    }

    /// Three sources, oldest to newest, and a reader that records what it opened.
    fn run(stop: &dyn Fn(&str) -> bool) -> (String, Vec<String>) {
        let sources: Vec<PathBuf> = ["old.gz", "mid.gz", "events.jsonl"].iter().map(PathBuf::from).collect();
        let texts = [("old.gz", start("s1")), ("mid.gz", format!("{}mid line\n", start("s2"))), ("events.jsonl", "live line\n".to_string())];
        let mut opened = Vec::new();
        let mut read = |p: &Path| {
            let name = p.to_string_lossy().into_owned();
            opened.push(name.clone());
            texts.iter().find(|(n, _)| *n == name).map(|(_, t)| t.clone())
        };
        let text = newest_first_text(&sources, &mut read, stop);
        (text, opened)
    }

    #[test]
    fn a_session_lookup_does_not_open_archives_older_than_the_session() {
        let (text, opened) = run(&|t| has_session_start(t, |v| v["session"] == "s2"));
        assert_eq!(opened, ["events.jsonl", "mid.gz"], "old.gz predates s2's start and is never opened");
        assert_eq!(text, format!("{}mid line\nlive line\n", start("s2")), "read back oldest first");
    }

    #[test]
    fn parsed_session_events_do_not_open_archives_older_than_the_session() {
        let sources: Vec<PathBuf> = ["old.gz", "mid.gz", "events.jsonl"].iter().map(PathBuf::from).collect();
        let fire = |s: &str, way: &str| format!("{{\"event\":\"way_fired\",\"session\":\"{s}\",\"way\":\"{way}\"}}\n");
        let texts = [
            ("old.gz", format!("{}{}", start("s2"), fire("s2", "ghost"))),
            ("mid.gz", format!("{}{}", start("s2"), fire("s2", "a"))),
            ("events.jsonl", fire("s2", "b")),
        ];
        let mut opened = Vec::new();
        let mut read = |p: &Path| {
            opened.push(p.to_string_lossy().into_owned());
            texts.iter().find(|(n, _)| Path::new(n) == p).map(|(_, t)| t.clone())
        };
        let events = events_for_session_from(&sources, &mut read, "s2");
        assert_eq!(opened, ["events.jsonl", "mid.gz"]);
        let ways: Vec<&str> = events.iter().filter_map(|e| e["way"].as_str()).collect();
        assert_eq!(ways, ["a", "b"]);
    }

    #[test]
    fn a_session_with_no_start_on_record_reads_all_history() {
        let (text, opened) = run(&|t| has_session_start(t, |v| v["session"] == "nope"));
        assert_eq!(opened.len(), 3);
        assert!(text.starts_with(&start("s1")) && text.ends_with("live line\n"));
    }

    #[test]
    fn the_full_reader_joins_every_source_in_order() {
        let (text, opened) = run(&|_| false);
        assert_eq!(opened.len(), 3);
        assert_eq!(text, format!("{}{}mid line\nlive line\n", start("s1"), start("s2")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn counts_only_way_fired_events() {
        let events = vec![
            json!({"event": "way_fired", "way": "softwaredev/commits"}),
            json!({"event": "way_fired", "way": "softwaredev/commits"}),
            json!({"event": "way_fired", "way": "meta/todos"}),
            json!({"event": "session_start", "way": "softwaredev/commits"}),
            json!({"event": "way_fired"}), // no way field → skipped
        ];
        let counts = count_fires(&events);
        assert_eq!(counts.get("softwaredev/commits"), Some(&2));
        assert_eq!(counts.get("meta/todos"), Some(&1));
        assert_eq!(counts.len(), 2);
    }

    #[test]
    fn empty_events_yield_empty_counts() {
        assert!(count_fires(&[]).is_empty());
    }
}
