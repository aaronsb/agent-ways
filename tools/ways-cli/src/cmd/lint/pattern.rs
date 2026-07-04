//! Pattern-hygiene lint (ADR-155 §5): the `pattern:` field is the
//! high-precision keyword lane. Suggestive common words belong in
//! `vocabulary:` where the embedding lane weighs them contextually; the
//! keyword lane should carry only exact, anchored, term-of-art triggers.
//!
//! Three advisory rules, all emitted as WARNING (so `--check` in CI is not
//! gated on hygiene — the rework pass and telemetry adjudicate per way):
//!
//! 1. **Bare common word** — an alternation that is a plain dictionary word
//!    (from [`COMMON_WORDS`]) fires on unrelated prose. Move it to
//!    `vocabulary:`.
//! 2. **Short unanchored alternation** — a literal alternation shorter than
//!    [`SHORT_ALTERNATION_FLOOR`] with no word-boundary anchoring collides
//!    with substrings of larger words. Anchor it (term of art) or demote it.
//! 3. **`.*` in a prompt pattern** — an unbounded greedy wildcard makes the
//!    trigger match almost anything between its literals.
//!
//! The rules operate on top-level alternations (split on `|` at paren depth
//! 0), so a grouped inner `|` such as `alert.?(response|triage)` is treated
//! as one alternation, not three.

use std::fmt::Display;

/// Literal alternations at or above this length are common enough in normal
/// English that unanchored short-token collisions are the dominant risk; below
/// it, a bare token matched anywhere in the prompt fires on unrelated
/// substrings. Chosen to flag 2–4 char tokens (`pr`, `erd`, `dbml`, `docs`)
/// while leaving 5+ char tokens to the common-word rule. Heuristic, not a law.
const SHORT_ALTERNATION_FLOOR: usize = 5;

/// Suggestive words that read as intent in prose and therefore belong in the
/// contextual (embedding) lane, not the exact keyword lane. Kept deliberately
/// narrow: unambiguously generic words plus the four the ADR names
/// (`remember`, `commit`, `workflow`, `docs`). Domain terms of art (`adr`,
/// `diataxis`, `schema`, `migrate`, `mermaid`) are intentionally absent — the
/// rework pass keeps-and-anchors those rather than demoting them. Extend this
/// list from `way_keyword_gated` telemetry as live offenders surface.
const COMMON_WORDS: &[&str] = &[
    // ADR-155 §5 names these four explicitly.
    "remember",
    "commit",
    "workflow",
    "docs",
    // Generic verbs/nouns that fire on unrelated prose.
    "note",
    "review",
    "error",
    "exception",
    "slow",
    "speed",
    "upgrade",
    "release",
    "audit",
    "automate",
    "tooling",
    "skill",
    // Generic doc/data nouns better served by the embedding lane.
    "documentation",
    "package",
    "library",
    "chart",
    "graph",
    "plot",
    "performance",
];

/// Run all three pattern-hygiene rules against one way's `pattern:` value.
/// `warnings` accumulates the project-wide counter owned by the caller.
pub(super) fn check_pattern(rel: &dyn Display, pattern: &str, warnings: &mut u32) {
    let pattern = unquote(pattern.trim());
    for alt in split_top_level(pattern) {
        let alt = alt.trim();
        if alt.is_empty() {
            continue;
        }

        // Rule 3: unbounded greedy wildcard.
        if alt.contains(".*") {
            eprintln!(
                "  WARNING: {rel} — pattern alternation '{alt}' uses '.*' (greedy); \
                 prompt patterns should avoid unbounded wildcards — bound or restructure it (ADR-155 §5)"
            );
            *warnings += 1;
        }

        let info = classify(alt);

        // Rules 1 and 2 only apply to bare literal words; structured
        // alternations (groups, character classes, multi-word phrases joined
        // by `.?`/`.*`) are outside the vocabulary-demotion doctrine.
        if !info.is_bare_word {
            continue;
        }
        let core = info.core.to_ascii_lowercase();

        // Rule 2: common word (length-independent). Takes precedence over the
        // short-token rule so an alternation earns at most one of these two.
        if COMMON_WORDS.contains(&core.as_str()) {
            eprintln!(
                "  WARNING: {rel} — pattern alternation '{alt}' is a common word; \
                 move it to vocabulary: (semantic lane) and keep pattern: for exact triggers (ADR-155 §5)"
            );
            *warnings += 1;
            continue;
        }

        // Rule 1: short and unanchored.
        if !info.is_anchored && info.core.len() < SHORT_ALTERNATION_FLOOR {
            let n = info.core.len();
            eprintln!(
                "  WARNING: {rel} — pattern alternation '{alt}' is short ({n} chars) and unanchored; \
                 anchor it (\\b{core}\\b) if it's a term of art, or move it to vocabulary: (ADR-155 §5)"
            );
            *warnings += 1;
        }
    }
}

/// Strip one layer of matching YAML quotes, if present.
fn unquote(s: &str) -> &str {
    for q in ['"', '\''] {
        if s.len() >= 2 && s.starts_with(q) && s.ends_with(q) {
            return &s[1..s.len() - 1];
        }
    }
    s
}

/// Split a regex on `|` at parenthesis depth 0, so grouped inner alternations
/// (`(response|triage)`) and escaped pipes (`\|`) stay intact.
fn split_top_level(pattern: &str) -> Vec<&str> {
    let bytes = pattern.as_bytes();
    let mut parts = Vec::new();
    let mut depth: i32 = 0;
    let mut start = 0usize;
    let mut i = 0usize;
    let mut in_class = false; // inside a [...] character class
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 2; // skip escaped char
                continue;
            }
            b'[' if !in_class => in_class = true,
            b']' if in_class => in_class = false,
            b'(' if !in_class => depth += 1,
            b')' if !in_class => depth -= 1,
            b'|' if !in_class && depth == 0 => {
                parts.push(&pattern[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(&pattern[start..]);
    parts
}

struct AltInfo {
    /// Alphanumeric characters of the alternation, in order.
    core: String,
    /// The alternation is a single literal word (only word chars, optionally
    /// with anchor decoration and an optional trailing `?`) — not a phrase or
    /// structured regex.
    is_bare_word: bool,
    /// Carries word-boundary anchoring (`\b`, `^`/`$`, `(^| )`/`( |$)`, or a
    /// leading/trailing literal or escaped space).
    is_anchored: bool,
}

fn classify(alt: &str) -> AltInfo {
    let core: String = alt.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let is_anchored = detect_anchor(alt);

    // Strip anchor decoration, then test whether the remainder is a single
    // literal word. `\b`, `^`, `$`, `(^| )`, `( |$)`, and boundary spaces are
    // removed; a single trailing `?` (optional last letter, e.g. `ways?`) is
    // allowed. Anything else (`.`, `*`, `+`, groups, classes, interior `?`)
    // means the alternation is not a bare word.
    let mut s = alt.trim();
    s = s
        .trim_start_matches("(^| )")
        .trim_start_matches("(^|")
        .trim_end_matches("( |$)")
        .trim_end_matches("|$)");
    let s = s.replace("\\b", "");
    let mut s = s.trim();
    s = s.trim_start_matches('^').trim_end_matches('$');
    s = s.trim_start_matches("\\ ").trim_end_matches("\\ ");
    let s = s.trim();
    let body = s.strip_suffix('?').unwrap_or(s);
    let is_bare_word = !body.is_empty() && body.chars().all(|c| c.is_ascii_alphanumeric());

    AltInfo {
        core,
        is_bare_word,
        is_anchored,
    }
}

fn detect_anchor(alt: &str) -> bool {
    alt.contains("\\b")
        || alt.starts_with('^')
        || alt.ends_with('$')
        || alt.starts_with("(^|")
        || alt.ends_with("|$)")
        || alt.starts_with(' ')
        || alt.ends_with(' ')
        || alt.starts_with("\\ ")
        || alt.ends_with("\\ ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn warnings_for(pattern: &str) -> u32 {
        let mut n = 0;
        check_pattern(&"way", pattern, &mut n);
        n
    }

    #[test]
    fn split_respects_group_depth() {
        assert_eq!(
            split_top_level("alert.?(response|triage)|remediat"),
            vec!["alert.?(response|triage)", "remediat"]
        );
    }

    #[test]
    fn split_respects_escaped_pipe_and_class() {
        assert_eq!(split_top_level(r"a\|b|c"), vec![r"a\|b", "c"]);
        assert_eq!(split_top_level("[a|b]|c"), vec!["[a|b]", "c"]);
    }

    #[test]
    fn anchored_term_of_art_is_clean() {
        // (^| )adr( |$) — anchored, short: no warning.
        assert_eq!(warnings_for("(^| )adr( |$)|architect|decision"), 0);
    }

    #[test]
    fn common_word_flagged() {
        assert_eq!(warnings_for("remember|save.*memory"), 2); // remember + .*
        assert_eq!(warnings_for("commit"), 1);
        assert_eq!(warnings_for("workflow|orchestrat|pipeline"), 1); // only workflow
    }

    #[test]
    fn short_unanchored_flagged() {
        assert_eq!(warnings_for("erd|entity.?relationship"), 1); // erd short
        assert_eq!(warnings_for("dbml"), 1);
    }

    #[test]
    fn short_but_anchored_is_clean() {
        assert_eq!(warnings_for(r"\berd\b|entity"), 0);
    }

    #[test]
    fn greedy_wildcard_flagged() {
        assert_eq!(warnings_for("create.*pr|pr.*create"), 2);
    }

    #[test]
    fn phrase_not_treated_as_bare_word() {
        // design.?pattern is a joined phrase, not a bare word: no common/short
        // flag, and no `.*`.
        assert_eq!(warnings_for("design.?pattern|technical.?choice"), 0);
    }

    #[test]
    fn optional_trailing_letter_is_bare() {
        // ways? — bare word "ways"; not in COMMON_WORDS (project term of art),
        // long enough to escape the short rule: clean.
        assert_eq!(warnings_for("(^| )ways?( |$)|knowledge"), 0);
    }

    #[test]
    fn longer_uncommon_bare_word_is_clean() {
        assert_eq!(warnings_for("orchestrat|multiagent"), 0);
    }
}
