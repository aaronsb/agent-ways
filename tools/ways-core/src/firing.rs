//! Firing-event log reader (ADR-151 §1).
//!
//! Reads the append-only JSONL log of way-firing events and aggregates it.
//! Shared engine: the compliance tooling cross-references claims against how
//! often a way actually fires, and the reader belongs in the library rather
//! than any one binary.

use serde_json::Value;
use std::collections::HashMap;

/// Load all firing events, one JSON object per line, unioned across every
/// existing events-log file (ADR-153 §1).
///
/// A missing or unreadable log is not an error — it contributes nothing (a fresh
/// install simply has no firing history yet). The union recovers `session_start`
/// lines that older shell hooks orphaned in the legacy `~/.claude/stats` file
/// after the XDG migration; see [`crate::paths::events_log_sources`] for why the
/// files never overlap.
pub fn load_events() -> Vec<Value> {
    crate::paths::events_log_sources()
        .iter()
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .flat_map(|content| {
            content
                .lines()
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect::<Vec<Value>>()
        })
        .collect()
}

/// Concatenated raw text of every existing events-log file (ADR-153 §1 union).
///
/// The line-oriented timeline reconstruction in `ways rethink` works over raw
/// JSONL lines rather than parsed values, so it needs the text, not
/// [`load_events`]'s `Vec<Value>`. Same union guarantee: recovers `session_start`
/// lines orphaned in the legacy log. Files are joined with a newline; the empty
/// line any trailing newline produces is inert to line parsers.
pub fn load_events_text() -> String {
    crate::paths::events_log_sources()
        .iter()
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .collect::<Vec<_>>()
        .join("\n")
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
