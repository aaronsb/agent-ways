//! Read-only reader for Claude-Code session transcripts (ADR-153 §3).
//!
//! Transcripts live at `~/.claude/projects/<slug>/<session>.jsonl`
//! (Claude-Code-owned; agent-ways is a read-only consumer). We use them for one
//! thing: recovering the **foreign key** the firing-event log lacks. Every
//! `UserPromptSubmit` hook injection is recorded as an `attachment` line whose
//! `parentUuid` chain walks back to the `user` message it augmented — so the
//! message a set of ways was injected into is recoverable post-hoc, without any
//! fire-time capture and even for historical sessions.
//!
//! This is deliberately *not* a general transcript parser: it extracts only
//! `(timestamp, user-message uuid)` per prompt injection. Absence and truncation
//! are normal (return empty), never an error.

use std::collections::HashMap;
use std::path::PathBuf;

/// One `UserPromptSubmit` injection resolved to the user message it augmented:
/// the injection's timestamp (for matching to a turn's fires) and the keyed
/// user-message uuid.
#[derive(Debug, Clone, PartialEq)]
pub struct PromptTurn {
    pub ts: String,
    pub user_uuid: String,
}

/// Locate a session's transcript. First tries the lossy `/`,`.`→`-` slug Claude
/// Code names project dirs with (matching `rethink::build_token_timeline` and
/// `session.rs`); on a miss — the recorded project can differ from the dir the
/// transcript actually lives in (subagent cwd, a `session_start` logged elsewhere)
/// — falls back to scanning every `projects/*` dir, since session ids are globally
/// unique. `None` if no matching transcript exists.
fn transcript_path(project: &str, session_id: &str) -> Option<PathBuf> {
    let root = crate::paths::transcripts_root();
    let file = format!("{session_id}.jsonl");

    let slug = project.replace(['/', '.'], "-");
    let direct = root.join(&slug).join(&file);
    if direct.is_file() {
        return Some(direct);
    }

    // Slug miss → the id is unique, so find it wherever it lives.
    std::fs::read_dir(&root)
        .ok()?
        .flatten()
        .map(|e| e.path().join(&file))
        .find(|p| p.is_file())
}

/// Resolve every `UserPromptSubmit` injection in a session's transcript to its
/// triggering user message. Empty when the transcript is absent/unreadable.
pub fn prompt_turns(project: &str, session_id: &str) -> Vec<PromptTurn> {
    match transcript_path(project, session_id).and_then(|p| std::fs::read_to_string(p).ok()) {
        Some(content) => prompt_turns_from_str(&content),
        None => Vec::new(),
    }
}

/// Pure core: parse a transcript's JSONL text into resolved prompt turns.
/// Factored out so the `parentUuid`-walk is testable against a fixture string.
pub fn prompt_turns_from_str(content: &str) -> Vec<PromptTurn> {
    // uuid -> (parentUuid, type). Only the fields the walk needs are retained, so
    // even a large transcript stays light.
    let mut parent_of: HashMap<String, (Option<String>, String)> = HashMap::new();
    // (ts, own uuid) for each UserPromptSubmit attachment, in file order.
    let mut injections: Vec<(String, String)> = Vec::new();

    for line in content.lines() {
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let Some(uuid) = v["uuid"].as_str() else {
            continue;
        };
        let ty = v["type"].as_str().unwrap_or("").to_string();
        let parent = v["parentUuid"].as_str().map(|s| s.to_string());
        if ty == "attachment" && v["attachment"]["hookEvent"].as_str() == Some("UserPromptSubmit")
        {
            let ts = v["timestamp"].as_str().unwrap_or("").to_string();
            injections.push((ts, uuid.to_string()));
        }
        parent_of.insert(uuid.to_string(), (parent, ty));
    }

    injections
        .into_iter()
        .filter_map(|(ts, uuid)| {
            walk_to_user(&uuid, &parent_of).map(|user_uuid| PromptTurn { ts, user_uuid })
        })
        .collect()
}

/// Follow `parentUuid` from `start` back to the first `type == "user"` entry.
/// Bounded so a malformed/cyclic chain can't loop forever.
fn walk_to_user(start: &str, parent_of: &HashMap<String, (Option<String>, String)>) -> Option<String> {
    let mut cur = start.to_string();
    for _ in 0..64 {
        let (parent, _) = parent_of.get(&cur)?;
        let parent = parent.as_ref()?;
        let (_, ty) = parent_of.get(parent)?;
        if ty == "user" {
            return Some(parent.clone());
        }
        cur = parent.clone();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_ups_injection_to_its_user_message() {
        // user message → an intervening attachment → the UserPromptSubmit
        // injection. The parentUuid chain must walk from the injection back to
        // the user message, skipping the intervening attachment.
        let jsonl = [
            r#"{"uuid":"u1","parentUuid":"a0","type":"user","timestamp":"2026-01-01T00:00:00.000Z"}"#,
            r#"{"uuid":"mid","parentUuid":"u1","type":"attachment","timestamp":"2026-01-01T00:00:01.000Z"}"#,
            r#"{"uuid":"ups","parentUuid":"mid","type":"attachment","timestamp":"2026-01-01T00:00:01.200Z","attachment":{"hookEvent":"UserPromptSubmit","content":"[ways]"}}"#,
            r#"{"uuid":"asst","parentUuid":"ups","type":"assistant","timestamp":"2026-01-01T00:00:02.000Z"}"#,
        ]
        .join("\n");

        let turns = prompt_turns_from_str(&jsonl);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].user_uuid, "u1");
        assert_eq!(turns[0].ts, "2026-01-01T00:00:01.200Z");
    }

    #[test]
    fn ignores_non_ups_and_unresolvable() {
        let jsonl = [
            // A PreToolUse hook attachment — not a prompt injection.
            r#"{"uuid":"x","parentUuid":"u1","type":"attachment","attachment":{"hookEvent":"PreToolUse"}}"#,
            // A UPS injection whose chain never reaches a user message.
            r#"{"uuid":"orphan","parentUuid":"missing","type":"attachment","timestamp":"t","attachment":{"hookEvent":"UserPromptSubmit"}}"#,
            r#"garbage-not-json"#,
        ]
        .join("\n");
        assert!(prompt_turns_from_str(&jsonl).is_empty());
    }
}
