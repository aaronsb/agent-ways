//! Positive-prose lint (ADR-160 §6): a way's **embedded alias** — the
//! `description:` and `vocabulary:` fields — is what the late-interaction
//! matcher ranks and gates on. Dense bi-encoders cannot represent negation, so
//! a contrast construction in the alias ("a skill *vs* a way", "orchestration
//! *versus* a single agent") pulls the embedding *toward* the very item it
//! means to exclude. The corpus-authoring corollary of ADR-160 stage 6: the
//! alias states only what the way is *for*, in positive terms; all exclusion
//! lives in the scope gate, not in negated text.
//!
//! **Scope is the alias only.** The way's *body* is deliberately not checked:
//! the body is the corpus for stage 5's winning-chunk confirm pass, where the
//! *best* body chunk corroborates the won surface chunk (max, not mean), so a
//! non-corroborating cross-reference chunk is simply inert. Progressive-
//! disclosure referrals ("see the subagents way", `[[links]]`, "defers to the
//! official docs") belong in the body and are encouraged there — this rule
//! never sees them.
//!
//! **One advisory rule, emitted as WARNING** (so `--check` in CI is not gated
//! on prose hygiene — the rework pass adjudicates per way):
//!
//! - **Contrast naming a sibling item** — a contrast marker ([`CONTRAST`])
//!   followed, within a short forward window in the *same field*, by a taxonomy
//!   noun ([`TAXONOMY`]: way / skill / agent / workflow / tool / MCP / CLI …).
//!   The forward window matters: "encoding operations as commands *rather than*
//!   manual shell sequences" contrasts a *behavior* and names its own topic
//!   ("commands") *before* the marker — not a violation. The window never
//!   crosses a field boundary, because a `description` contrast plus a
//!   `vocabulary` noun is not one construction.
//!
//! Deferral verbs ("defers to", "see", "pairs with", "builds on") are not
//! contrast markers, so progressive disclosure that happens to sit in the alias
//! is spared; only *contrastive* naming trips the rule.

use std::fmt::Display;

/// How far (in bytes) past a contrast marker to look for a taxonomy noun. Wide
/// enough to span an intervening article/adjective ("versus a single agent",
/// "rather than referencing specific tool names") but short enough that an
/// unrelated later clause in the same field does not pair spuriously.
const FORWARD_WINDOW: usize = 60;

/// Contrast / exclusion constructions. A marker here, followed by a sibling
/// noun, is the negation the embedder cannot represent. Deliberately excludes
/// bare `not` / `no` / `without` — those overwhelmingly appear as domain
/// vocabulary (`if not exists`, `no longer used`, `without erroring`) rather
/// than as inter-item contrast, and pairing them with the forward-noun gate
/// still over-fires. Contrastive phrases are the precise signal.
const CONTRAST: &[&str] = &[
    "vs",
    "versus",
    "rather than",
    "instead of",
    "unlike",
    "as opposed to",
    "distinct from",
    "differs from",
    "differ from",
];

/// Sibling corpus items and tool categories. Naming one of these inside a
/// contrast pulls the alias toward it. Matched as whole lowercased words (or the
/// two-word phrase "slash command"); a plural trailing `s` is tolerated.
const TAXONOMY: &[&str] = &[
    "way",
    "skill",
    "slash command",
    "subagent",
    "agent",
    "workflow",
    "macro",
    "tool",
    "mcp",
    "cli",
    "hook",
    "command",
];

/// Check one embedded-alias field (`description` or `vocabulary`) for a contrast
/// construction that names a sibling item. `field` is the field name (for the
/// message); `warnings` accumulates the project-wide counter owned by the
/// caller. Each distinct marker→noun pairing warns at most once.
pub(super) fn check_positive_prose(
    rel: &dyn Display,
    field: &str,
    value: &str,
    warnings: &mut u32,
) {
    let (body, was_quoted) = unquote(value.trim());
    let val = if was_quoted {
        body
    } else {
        strip_yaml_comment(body)
    };
    let lower = val.to_lowercase();

    for marker in CONTRAST {
        // Scan every whole-word occurrence of this marker.
        for start in whole_word_matches(&lower, marker) {
            let after = start + marker.len();
            let window_end = (after + FORWARD_WINDOW).min(lower.len());
            let window = &lower[after..window_end];
            if let Some(noun) = TAXONOMY.iter().find(|n| whole_word_contains(window, n)) {
                eprintln!(
                    "  WARNING: {rel} — {field} contrasts against '{noun}' ('{marker} … {noun}'); \
                     the embedded alias states only what the way is for, in positive terms — move \
                     the comparison to the body (ADR-160 §6)"
                );
                *warnings += 1;
            }
        }
    }
}

/// Byte offsets where `needle` occurs in `hay` as a whole word (alphanumeric
/// boundaries on both sides). `hay` is already lowercased; `needle` is lower.
fn whole_word_matches(hay: &str, needle: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = hay[from..].find(needle) {
        let at = from + rel;
        let end = at + needle.len();
        let left_ok = at == 0 || !is_word_byte(hay.as_bytes()[at - 1]);
        let right_ok = end == hay.len() || !is_word_byte(hay.as_bytes()[end]);
        if left_ok && right_ok {
            out.push(at);
        }
        from = at + 1;
    }
    out
}

/// Whole-word (phrase) containment: true if `needle` occurs in `hay` on word
/// boundaries. A trailing plural `s` on the last token is tolerated so "agents"
/// matches "agent" and "tool names" matches "tool".
fn whole_word_contains(hay: &str, needle: &str) -> bool {
    for at in byte_matches(hay, needle) {
        let end = at + needle.len();
        let left_ok = at == 0 || !is_word_byte(hay.as_bytes()[at - 1]);
        // Right boundary: end of string, a non-word byte, or a lone trailing
        // 's' (plural) that is itself followed by a non-word byte.
        let bytes = hay.as_bytes();
        let right_ok = end == hay.len()
            || !is_word_byte(bytes[end])
            || (bytes[end] == b's' && (end + 1 == hay.len() || !is_word_byte(bytes[end + 1])));
        if left_ok && right_ok {
            return true;
        }
    }
    false
}

fn byte_matches(hay: &str, needle: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = hay[from..].find(needle) {
        let at = from + rel;
        out.push(at);
        from = at + 1;
    }
    out
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Strip one layer of matching YAML quotes. Returns the inner text and whether
/// a quote layer was removed (quoted values do not honor `#` comments).
fn unquote(s: &str) -> (&str, bool) {
    for q in ['"', '\''] {
        if s.len() >= 2 && s.starts_with(q) && s.ends_with(q) {
            return (&s[1..s.len() - 1], true);
        }
    }
    (s, false)
}

/// Drop a plain-scalar inline comment (` #…`), matching serde_yaml. A `#` not
/// preceded by whitespace is literal and kept.
fn strip_yaml_comment(s: &str) -> &str {
    match s.find(" #") {
        Some(i) => s[..i].trim_end(),
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn warnings_for(field: &str, value: &str) -> u32 {
        let mut n = 0;
        check_positive_prose(&"way", field, value, &mut n);
        n
    }

    #[test]
    fn skill_vs_way_flagged() {
        // The meta/skills violation: two contrastive namings of siblings.
        let n = warnings_for(
            "description",
            "How we write skills — when a skill vs a way vs a slash command, naming conventions",
        );
        assert_eq!(n, 2); // vs→way, vs→slash command
    }

    #[test]
    fn versus_agent_flagged() {
        // meta/workflows: "orchestration, versus a single agent, a skill, …".
        assert_eq!(
            warnings_for(
                "description",
                "When to reach for the Workflow tool — orchestration, versus a single agent, a skill"
            ),
            1 // versus→agent (first taxonomy noun in window)
        );
    }

    #[test]
    fn rather_than_tool_flagged() {
        // tool-agnostic: "patterns rather than referencing specific tool names".
        assert_eq!(
            warnings_for(
                "description",
                "writing ways that describe patterns rather than referencing specific tool names, MCP servers"
            ),
            1
        );
    }

    #[test]
    fn contrast_naming_a_behavior_is_clean() {
        // meta/choices: contrasts a behavior, no sibling noun forward.
        assert_eq!(
            warnings_for(
                "description",
                "presenting decisions as explicit choices rather than burying options in prose"
            ),
            0
        );
    }

    #[test]
    fn own_topic_before_marker_is_clean() {
        // tooling: "commands" is its own topic, *before* the marker; forward
        // from "rather than" is "manual shell sequences" — no sibling noun.
        assert_eq!(
            warnings_for(
                "description",
                "encoding repeated operations as commands rather than manual shell sequences"
            ),
            0
        );
    }

    #[test]
    fn deferral_verb_is_not_contrast() {
        // Progressive disclosure in the alias is spared: "defers … to" is not a
        // contrast marker even though it names a doc/way.
        assert_eq!(
            warnings_for(
                "description",
                "how we write skills; defers SKILL.md mechanics to the official docs"
            ),
            0
        );
    }

    #[test]
    fn domain_vs_without_sibling_noun_is_clean() {
        // groundtruth vocabulary: "docs vs code spec vs implementation" — 'vs'
        // present, but code/spec/implementation are not taxonomy nouns.
        assert_eq!(
            warnings_for("vocabulary", "source of truth docs vs code spec vs implementation"),
            0
        );
    }

    #[test]
    fn bare_negation_is_not_a_contrast_marker() {
        // 'not' / 'without' are excluded: domain vocabulary, not inter-item
        // contrast.
        assert_eq!(
            warnings_for(
                "description",
                "idempotent migrations that survive a retry without erroring; guard with if not exists"
            ),
            0
        );
    }

    #[test]
    fn plural_taxonomy_noun_matches() {
        assert_eq!(
            warnings_for("description", "one procedure rather than several workflows"),
            1
        );
    }

    #[test]
    fn noun_outside_window_is_clean() {
        // A sibling noun far past the marker (beyond FORWARD_WINDOW) does not
        // pair with it.
        let far = format!(
            "orchestration rather than {}the agent lane",
            " padded".repeat(12)
        );
        assert_eq!(warnings_for("description", &far), 0);
    }

    #[test]
    fn positive_alias_is_clean() {
        // A fully positive alias never warns.
        assert_eq!(
            warnings_for(
                "description",
                "deterministic multi-agent orchestration: fan-out, staged pipelines, verification and synthesis"
            ),
            0
        );
    }

    #[test]
    fn quoted_value_and_inline_comment_handled() {
        // Quoted scalar keeps '#'; unquoted drops the trailing comment. Neither
        // should change the contrast verdict here (clean either way).
        assert_eq!(warnings_for("vocabulary", "\"portable vendor neutral abstract\""), 0);
        assert_eq!(warnings_for("vocabulary", "portable abstract # note"), 0);
    }
}
