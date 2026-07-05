//! scan/lookbehind.rs — transcript lookbehind for the tool-use channel (ADR-160
//! stage 1: contextualize the surface).
//!
//! A bare shell command is an *action*, not a statement of intent, so it matches
//! ways poorly on its own. The reasoning that led to the tool call sits just
//! above it in the transcript — the assistant's prose since the last human turn.
//! This module extracts that prose so the command surface can be embedded as
//! intent, not just the literal artifact.
//!
//! **Fail-safe.** Every path returns `None` (the caller falls back to
//! command-only) when there is no transcript, no usable prose, or anything
//! unparseable — lookbehind can only *add* intent context, never break matching.
//!
//! **Cheap on the hot path.** Bash is the highest-volume scan surface, so this
//! reads only the transcript's tail (the current turn lives at the end) rather
//! than loading a multi-megabyte file on every command.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde_json::Value;

/// Only the tail is needed — the current turn is at the end of the transcript.
/// A turn with more than this much assistant prose + tool traffic before the
/// command is vanishingly rare; if the human boundary falls outside the tail we
/// simply return the prose we found (still useful) or None.
const TAIL_BYTES: u64 = 64 * 1024;

/// The assistant's prose since the last human turn, for `session_id`, or `None`.
pub(crate) fn intent(session_id: &str) -> Option<String> {
    let path = crate::cmd::context::find_transcript_by_session(session_id)?;
    let tail = read_tail(&path, TAIL_BYTES)?;
    assistant_prose_since_last_human(&tail)
}

/// Read up to `max_bytes` from the end of `path`, lossily decoded (a tail that
/// splits a UTF-8 boundary or a JSON line is fine — partial lines just fail to
/// parse and are skipped).
fn read_tail(path: &Path, max_bytes: u64) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(max_bytes);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Walk the transcript backward collecting assistant text blocks until a genuine
/// human message ends the walk. `tool_result` records (also role `user`) are
/// part of the current turn's tool loop and are skipped, so multi-tool turns
/// still resolve to the human turn that started them. Returns the collected
/// prose in chronological order, or `None` if there is none.
pub(crate) fn assistant_prose_since_last_human(transcript: &str) -> Option<String> {
    let mut prose: Vec<String> = Vec::new(); // reverse-chronological while building
    for line in transcript.lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue; // a partial first line (tail split) or non-JSON — skip
        };
        let msg = v.get("message");
        let role = msg.and_then(|m| m.get("role")).and_then(|r| r.as_str());
        let content = msg.and_then(|m| m.get("content"));
        match role {
            Some("assistant") => collect_text_blocks(content, &mut prose),
            Some("user") if is_human_message(content) => break,
            _ => {} // tool_result user records, summaries, meta — skip
        }
    }
    if prose.is_empty() {
        return None;
    }
    prose.reverse();
    let joined: String = prose.join(" ").split_whitespace().collect::<Vec<_>>().join(" ");
    (!joined.is_empty()).then_some(joined)
}

/// Push an assistant record's text blocks onto `out` (reverse order, so the
/// caller's final reverse yields chronological prose). Handles both a plain
/// string content and the block-array form.
fn collect_text_blocks(content: Option<&Value>, out: &mut Vec<String>) {
    match content {
        Some(Value::String(s)) => {
            if !s.trim().is_empty() {
                out.push(s.clone());
            }
        }
        Some(Value::Array(blocks)) => {
            for b in blocks.iter().rev() {
                if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                    if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                        if !t.trim().is_empty() {
                            out.push(t.to_string());
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

/// A genuine human turn vs a `tool_result` carrier (both are role `user`). A real
/// prompt is a plain string or has a text block and no `tool_result` block.
fn is_human_message(content: Option<&Value>) -> bool {
    match content {
        Some(Value::String(_)) => true,
        Some(Value::Array(blocks)) => {
            let block_type = |b: &Value| b.get("type").and_then(|t| t.as_str()).map(str::to_string);
            let has_tool_result = blocks.iter().any(|b| block_type(b).as_deref() == Some("tool_result"));
            let has_text = blocks.iter().any(|b| block_type(b).as_deref() == Some("text"));
            has_text && !has_tool_result
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Minimal transcript records in the Claude Code JSONL shape.
    fn user_text(t: &str) -> String {
        format!(r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"text","text":"{t}"}}]}}}}"#)
    }
    fn assistant_text(t: &str) -> String {
        format!(r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"{t}"}}]}}}}"#)
    }
    fn tool_result(t: &str) -> String {
        format!(r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","content":"{t}"}}]}}}}"#)
    }

    #[test]
    fn collects_assistant_prose_since_last_human() {
        let t = [
            user_text("older question"),
            assistant_text("older answer"),
            user_text("let us commit the fix"),
            assistant_text("I will stage the change and commit it."),
            tool_result("staged"),
            assistant_text("Now committing."),
        ]
        .join("\n");
        let prose = assistant_prose_since_last_human(&t).unwrap();
        // Spans the tool_result within the turn, stops at the human message,
        // excludes the earlier turn's "older answer".
        assert!(prose.contains("I will stage the change"), "{prose}");
        assert!(prose.contains("Now committing"), "{prose}");
        assert!(!prose.contains("older answer"), "{prose}");
        assert!(!prose.contains("let us commit"), "prose must not include the human turn: {prose}");
    }

    #[test]
    fn none_when_no_assistant_prose() {
        let t = user_text("just a question with no assistant turn yet");
        assert!(assistant_prose_since_last_human(&t).is_none());
    }

    #[test]
    fn partial_leading_line_is_skipped() {
        // A tail that starts mid-line (unparseable) must not poison the result.
        let t = format!("nk\":\"garbage partial line\"}}\n{}", assistant_text("real intent prose here"));
        let prose = assistant_prose_since_last_human(&t).unwrap();
        assert_eq!(prose, "real intent prose here");
    }
}
