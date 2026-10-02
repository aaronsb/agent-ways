//! Reading token usage and the model from a transcript's text.

use serde_json::Value;

/// The model Claude Code records on turns it writes itself (interrupts, API
/// errors). It names no model.
pub const SYNTHETIC_MODEL: &str = "<synthetic>";

/// Context tokens of one `message.usage` object: input plus cache reads plus
/// cache writes. Output tokens are not context until the next turn reads them.
pub fn usage_total(usage: &Value) -> u64 {
    let n = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
    n("input_tokens") + n("cache_read_input_tokens") + n("cache_creation_input_tokens")
}

/// The parsed record, when `line` is an assistant turn.
fn assistant(line: &str) -> Option<Value> {
    let v: Value = serde_json::from_str(line).ok()?;
    (v.get("type").and_then(Value::as_str) == Some("assistant")).then_some(v)
}

/// The context size at the newest assistant turn whose usage is non-zero.
/// `None` when no turn reports usage.
pub fn last_context_tokens(content: &str) -> Option<u64> {
    content
        .lines()
        .rev()
        .filter(|l| l.contains("\"usage\""))
        .filter_map(assistant)
        .filter_map(|v| v.get("message")?.get("usage").map(usage_total))
        .find(|&total| total > 0)
}

/// The model of the newest assistant turn that names a real one: turns with
/// [`SYNTHETIC_MODEL`] or an empty model are skipped. `None` before the first
/// assistant turn is written.
pub fn last_model(content: &str) -> Option<String> {
    content
        .lines()
        .rev()
        .filter(|l| l.contains("\"model\""))
        .filter_map(assistant)
        .filter_map(|v| v.get("message")?.get("model")?.as_str().map(str::to_string))
        .find(|m| !m.trim().is_empty() && m.trim() != SYNTHETIC_MODEL)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(model: &str, input: u64, read: u64, create: u64) -> String {
        format!(
            r#"{{"type":"assistant","message":{{"model":"{model}","usage":{{"input_tokens":{input},"cache_read_input_tokens":{read},"cache_creation_input_tokens":{create},"output_tokens":9}}}}}}"#
        )
    }

    #[test]
    fn sums_input_and_both_cache_counts() {
        let content = [turn("m1", 1, 2, 3), turn("m2", 10, 20, 30)].join("\n");
        assert_eq!(last_context_tokens(&content), Some(60));
    }

    #[test]
    fn skips_a_trailing_zero_usage_turn() {
        // A synthetic interrupt turn reports zero usage; the context did not
        // shrink to nothing.
        let content = [turn("m1", 1, 100, 0), turn(SYNTHETIC_MODEL, 0, 0, 0)].join("\n");
        assert_eq!(last_context_tokens(&content), Some(101));
        assert_eq!(last_model(&content).as_deref(), Some("m1"));
    }

    #[test]
    fn ignores_user_lines_and_garbage() {
        let content = [
            turn("m1", 5, 0, 0),
            r#"{"type":"user","message":{"model":"fake","usage":{"input_tokens":999}}}"#.to_string(),
            "not json \"usage\" \"model\"".to_string(),
        ]
        .join("\n");
        assert_eq!(last_context_tokens(&content), Some(5));
        assert_eq!(last_model(&content).as_deref(), Some("m1"));
    }

    #[test]
    fn empty_transcript_has_no_usage_or_model() {
        assert_eq!(last_context_tokens(""), None);
        assert_eq!(last_model(""), None);
    }
}
