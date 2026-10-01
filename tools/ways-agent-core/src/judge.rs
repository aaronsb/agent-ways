//! The judge's question, independent of engine (ADR-196 §2-3).
//!
//! One request per prompt carries the recent turns and every candidate. Each
//! candidate is a way's path through the ways tree followed by its
//! description. The engine answers, per candidate, relevant yes or no with a
//! confidence; P(yes) is the confidence of a yes, or one minus that of a no.
//! The wording is the probe's (ADR-195), extended to several candidates and
//! measured in that form: AUC 0.936 against 0.924 for one call per way.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const SYSTEM: &str = "You are a relevance gate for a coding assistant's guidance system. You are given \
several pieces of guidance and the most recent turns of a conversation. Judge each piece of guidance on its own. \
Call the record_judgements tool exactly once, with one entry per piece of guidance. `relevant` is your yes/no \
answer; `confidence` is your probability, from 0 to 1, that this answer is correct.";

const INSTRUCTION: &str =
    "Decide whether each piece of guidance is relevant to what the conversation is doing in its most recent turns.";

pub const TOOL_NAME: &str = "record_judgements";
pub const TOOL_DESCRIPTION: &str =
    "Record, for each piece of guidance, whether it is relevant to the recent conversation.";

/// The JSON schema of the tool's input: one judgement per candidate.
pub fn tool_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["judgements"],
        "properties": {
            "judgements": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["id", "relevant", "confidence"],
                    "properties": {
                        "id": {"type": "string"},
                        "relevant": {"type": "boolean"},
                        "confidence": {"type": "number"}
                    }
                }
            }
        }
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// One conversation turn: what the user typed or the assistant's prose, with
/// injected guidance, tool calls and results already removed by the caller.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    pub role: Role,
    pub text: String,
}

/// A way the matcher picked, as the judge sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    /// The way's id, e.g. `softwaredev/code/quality`.
    pub id: String,
    /// The text judged: see [`way_text`].
    pub text: String,
}

/// A way's text for the judge: its path through the tree, then its description
/// (input B in ADR-195, the best for both judges).
pub fn way_text(way_id: &str, description: &str) -> String {
    let route = way_id.split('/').filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" › ");
    format!("{route}\n{}", description.trim())
}

/// The last `n` turns as `User: …` / `Assistant: …` lines, whitespace
/// collapsed, each cut to its last `max_chars` characters.
pub fn render_turns(turns: &[Turn], n: usize, max_chars: usize) -> String {
    let start = turns.len().saturating_sub(n);
    turns[start..]
        .iter()
        .map(|t| {
            let text = t.text.split_whitespace().collect::<Vec<_>>().join(" ");
            let chars: Vec<char> = text.chars().collect();
            let text = if chars.len() > max_chars {
                format!("…{}", chars[chars.len() - max_chars + 1..].iter().collect::<String>())
            } else {
                text
            };
            let speaker = match t.role {
                Role::User => "User",
                Role::Assistant => "Assistant",
            };
            format!("{speaker}: {text}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The user message: the instruction, each candidate under id `g1`…`gN`, then
/// the conversation.
pub fn render_prompt(turns: &str, candidates: &[Candidate]) -> String {
    let guidance = candidates
        .iter()
        .enumerate()
        .map(|(i, c)| format!("<guidance id=\"g{}\">\n{}\n</guidance>", i + 1, c.text))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("{INSTRUCTION}\n\n{guidance}\n\n<conversation>\n{turns}\n</conversation>")
}

/// P(yes) from one judgement.
pub fn p_yes(relevant: bool, confidence: f64) -> f64 {
    let c = if confidence.is_finite() { confidence.clamp(0.0, 1.0) } else { 0.5 };
    if relevant {
        c
    } else {
        1.0 - c
    }
}

/// P(yes) per candidate, in candidate order, from the tool's input. A missing
/// or malformed judgement fails the whole call, which then fails open.
pub fn parse_judgements(input: &Value, n: usize) -> Result<Vec<f64>> {
    let list = input
        .get("judgements")
        .and_then(Value::as_array)
        .context("the answer has no judgements array")?;
    let mut out = vec![None; n];
    for j in list {
        let id = j.get("id").and_then(Value::as_str).unwrap_or_default();
        let Some(i) = id.strip_prefix('g').and_then(|s| s.parse::<usize>().ok()).filter(|i| (1..=n).contains(i)) else {
            continue;
        };
        let relevant = j.get("relevant").and_then(Value::as_bool).context("a judgement has no relevant flag")?;
        let confidence = j.get("confidence").and_then(Value::as_f64).context("a judgement has no confidence")?;
        out[i - 1] = Some(p_yes(relevant, confidence));
    }
    let missing = out.iter().filter(|p| p.is_none()).count();
    if missing > 0 {
        bail!("the answer judged {} of {n} candidates", n - missing);
    }
    Ok(out.into_iter().flatten().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(role: Role, text: &str) -> Turn {
        Turn { role, text: text.to_string() }
    }

    #[test]
    fn way_text_is_route_then_description() {
        assert_eq!(way_text("softwaredev/code/quality", " Quality flags. "), "softwaredev › code › quality\nQuality flags.");
    }

    #[test]
    fn render_turns_keeps_the_last_n_and_the_tail_of_each() {
        let turns = vec![turn(Role::User, "first"), turn(Role::Assistant, "a  b\n c"), turn(Role::User, "abcdefghij")];
        assert_eq!(render_turns(&turns, 2, 100), "Assistant: a b c\nUser: abcdefghij");
        assert_eq!(render_turns(&turns, 1, 5), "User: …ghij");
        assert_eq!(render_turns(&turns, 9, 100).lines().count(), 3);
    }

    #[test]
    fn prompt_numbers_candidates_from_one() {
        let c = vec![Candidate { id: "a/b".into(), text: "a › b\nx".into() }, Candidate { id: "c".into(), text: "c\ny".into() }];
        let p = render_prompt("User: hi", &c);
        assert!(p.contains("<guidance id=\"g1\">\na › b\nx\n</guidance>"));
        assert!(p.contains("<guidance id=\"g2\">"));
        assert!(p.ends_with("<conversation>\nUser: hi\n</conversation>"));
    }

    #[test]
    fn judgements_map_to_p_yes_in_candidate_order() {
        let input = json!({"judgements": [
            {"id": "g2", "relevant": false, "confidence": 0.9},
            {"id": "g1", "relevant": true, "confidence": 0.8},
            {"id": "g9", "relevant": true, "confidence": 1.0}
        ]});
        let p = parse_judgements(&input, 2).unwrap();
        assert!((p[0] - 0.8).abs() < 1e-9 && (p[1] - 0.1).abs() < 1e-9);
    }

    #[test]
    fn a_missing_judgement_fails_the_call() {
        let input = json!({"judgements": [{"id": "g1", "relevant": true, "confidence": 0.8}]});
        assert!(parse_judgements(&input, 2).is_err());
        assert!(parse_judgements(&json!({}), 1).is_err());
    }

    #[test]
    fn p_yes_clamps_and_survives_nan() {
        assert_eq!(p_yes(true, 1.7), 1.0);
        assert_eq!(p_yes(false, -1.0), 1.0);
        assert_eq!(p_yes(true, f64::NAN), 0.5);
    }
}
