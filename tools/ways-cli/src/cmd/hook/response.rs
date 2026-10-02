//! The Stop/prompt round trip (ADR-155 §3). At Stop, Claude's last response
//! is stored raw for the session; the next UserPromptSubmit hands it to the
//! prompt scan's embed lane, never its keyword lane. The scan-time reducer
//! (ADR-130) selects what matters, so nothing is extracted here.

use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::session;

/// Transcript lines searched from the end for the last assistant message.
const TAIL_LINES: usize = 100;
/// Bytes of the response kept: the reducer's budget is ~110 tokens, and the
/// response competes with the prompt on salience anyway.
const MAX_RESPONSE_BYTES: usize = 2000;

#[derive(Serialize, Deserialize)]
struct ResponseState {
    timestamp: String,
    context: String,
    response_length: usize,
}

/// Stop: store the last assistant message, or clear the record when this turn
/// has none (a tool-use-only stop). A record left from an earlier turn would be
/// embedded as if current, steering matching toward a topic turns old.
pub fn record(session_id: &str, transcript: &Path) -> std::io::Result<()> {
    let path = session::response_context_path(session_id);
    let Some(context) = last_response(transcript) else {
        let _ = std::fs::remove_file(&path);
        return Ok(());
    };
    let state = ResponseState {
        timestamp: agent_fmt::when::now_utc_iso(),
        response_length: context.chars().count(),
        context,
    };
    agent_settings::writer::write_atomic(&path, serde_json::to_vec(&state)?)
}

/// UserPromptSubmit: the response the last Stop recorded, if any.
pub fn read(session_id: &str) -> Option<String> {
    let text = std::fs::read_to_string(session::response_context_path(session_id)).ok()?;
    let state: ResponseState = serde_json::from_str(&text).ok()?;
    Some(state.context).filter(|c| !c.is_empty())
}

/// The text of the last assistant message in the transcript's tail, its text
/// blocks joined by newlines, cut to [`MAX_RESPONSE_BYTES`] on a character
/// boundary. `None` when the tail has no assistant text.
fn last_response(transcript: &Path) -> Option<String> {
    let tail = tail_lines(transcript, TAIL_LINES).ok()?;
    let entry = tail
        .lines()
        .rev()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v.get("type").and_then(|t| t.as_str()) == Some("assistant"))?;
    let text = entry
        .pointer("/message/content")
        .and_then(|c| c.as_array())
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    let mut cut = text.len().min(MAX_RESPONSE_BYTES);
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    let text = text[..cut].trim_end_matches('\n');
    (!text.is_empty()).then(|| text.to_string())
}

/// The last `n` lines of a file, read backwards in blocks so a long
/// transcript is not read whole.
fn tail_lines(path: &Path, n: usize) -> std::io::Result<String> {
    const BLOCK: u64 = 64 * 1024;
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let mut start = len;
    let mut buf: Vec<u8> = Vec::new();
    while start > 0 {
        let step = BLOCK.min(start);
        start -= step;
        f.seek(SeekFrom::Start(start))?;
        let mut block = vec![0; step as usize];
        f.read_exact(&mut block)?;
        block.extend_from_slice(&buf);
        buf = block;
        // n lines need n newlines before them, plus the one ending the file.
        if buf.iter().filter(|&&b| b == b'\n').count() > n {
            break;
        }
    }
    let text = String::from_utf8_lossy(&buf);
    let lines: Vec<&str> = text.lines().collect();
    Ok(lines[lines.len().saturating_sub(n)..].join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transcript(name: &str, lines: &[String]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("ways-response-{}-{name}.jsonl", std::process::id()));
        std::fs::write(&p, lines.join("\n") + "\n").unwrap();
        p
    }

    fn assistant(texts: &[&str]) -> String {
        let blocks: Vec<_> = texts.iter().map(|t| serde_json::json!({"type":"text","text":t})).collect();
        serde_json::json!({"type":"assistant","message":{"content":blocks}}).to_string()
    }

    #[test]
    fn last_response_joins_the_last_assistant_texts() {
        let user = serde_json::json!({"type":"user","message":{"content":"q"}}).to_string();
        let tool = serde_json::json!({"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash"}]}}).to_string();
        let p = transcript("join", &[assistant(&["old"]), user.clone(), assistant(&["first", "second"]), user]);
        assert_eq!(last_response(&p).as_deref(), Some("first\nsecond"));
        // A tool-use-only turn has no text.
        let p2 = transcript("tool", &[assistant(&["old"]), tool]);
        assert_eq!(last_response(&p2), None);
        std::fs::remove_file(p).ok();
        std::fs::remove_file(p2).ok();
    }

    #[test]
    fn last_response_cuts_on_a_char_boundary() {
        let long = "é".repeat(1500); // 3000 bytes
        let p = transcript("cut", &[assistant(&[&long])]);
        let got = last_response(&p).unwrap();
        assert_eq!(got.len(), 2000);
        assert!(got.chars().all(|c| c == 'é'));
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn tail_lines_reads_only_the_end() {
        let lines: Vec<String> = (0..50_000).map(|i| format!("line {i}")).collect();
        let p = transcript("tail", &lines);
        let tail = tail_lines(&p, 3).unwrap();
        assert_eq!(tail, "line 49997\nline 49998\nline 49999");
        std::fs::remove_file(p).ok();
    }
}
