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
//!    `vocabulary:`. Applies regardless of anchoring — an anchored `\bcommit\b`
//!    is still the word "commit".
//! 2. **Short unanchored alternation** — a literal alternation shorter than
//!    [`SHORT_ALTERNATION_FLOOR`] with no word-boundary anchoring collides
//!    with substrings of larger words. Anchor it (term of art) or demote it.
//! 3. **Unbounded `.*` wildcard** — a `.*` (greedy or lazy) makes the trigger
//!    match almost anything between its literals.
//!
//! The analysis mirrors the regex the matcher actually compiles: the value is
//! read as the matcher's YAML loader sees it (inline `# comment` and a leading
//! `(?i)`-style flag group stripped), and the rules operate on top-level
//! alternations (split on `|` at paren depth 0), so a grouped inner `|` such as
//! `alert.?(response|triage)` is treated as one alternation, not three.

use std::collections::HashSet;
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

/// Parse a `pattern_keep:` value into the set of lowercased bare words the
/// author has measured and deliberately kept in `pattern:` despite the
/// common-word / short-token heuristics. Whitespace-separated; an inline
/// `# reason` comment (plain scalar) is stripped so the value carries only the
/// exempted tokens. Each entry documents a calibration-backed exception
/// (ADR-155 §5 sweep, ADR-156 ruler): the keyword is load-bearing and its noise
/// is floor-gated, so the heuristic flag is a known false alarm.
pub(super) fn parse_keep(raw: &str) -> HashSet<String> {
    let (body, was_quoted) = unquote(raw.trim());
    let val = if was_quoted {
        body
    } else {
        strip_yaml_comment(body)
    };
    val.split_whitespace().map(|w| w.to_lowercase()).collect()
}

/// Run all three pattern-hygiene rules against one way's `pattern:` value.
/// `keep` lists bare words exempted from the common-word / short rules (see
/// [`parse_keep`]); the structural `.*` rule is never exempted. `warnings`
/// accumulates the project-wide counter owned by the caller.
pub(super) fn check_pattern(
    rel: &dyn Display,
    pattern: &str,
    keep: &HashSet<String>,
    warnings: &mut u32,
) {
    // Read the value the way the matcher's YAML loader does: strip a plain
    // scalar's inline `# comment` (only for unquoted values — quotes make `#`
    // literal) and a leading whole-pattern inline flag group like `(?i)`.
    let (body_val, was_quoted) = unquote(pattern.trim());
    let val = if was_quoted {
        body_val
    } else {
        strip_yaml_comment(body_val)
    };
    let val = strip_inline_flags(val);

    for raw in split_top_level(val) {
        let alt = raw.trim();
        if alt.is_empty() {
            continue;
        }

        // Rule 3: unbounded wildcard (greedy `.*` or lazy `.*?`).
        if alt.contains(".*") {
            eprintln!(
                "  WARNING: {rel} — pattern alternation '{alt}' uses an unbounded '.*' wildcard; \
                 prompt patterns should bound or restructure it (ADR-155 §5)"
            );
            *warnings += 1;
        }

        // Classify against the untrimmed alternation so leading/trailing space
        // anchoring (` erd `) is detected before it is trimmed away.
        let info = classify(raw);

        // Rules 1 and 2 only apply to bare literal words; structured
        // alternations (groups, character classes, multi-word phrases joined
        // by `.?`/`.*`) are outside the vocabulary-demotion doctrine.
        if !info.is_bare_word {
            continue;
        }

        // Author-acknowledged exception: a measured, calibration-backed keep
        // (`pattern_keep:`) exempts this bare word from the two heuristic
        // rules. The structural `.*` rule above is intentionally not exemptable
        // — a wildcard span is a defect, not a judgement call.
        if keep.contains(&info.word) {
            continue;
        }

        // Rule 1: common word (length-independent, anchoring-independent).
        // Takes precedence over the short-token rule so an alternation earns
        // at most one of these two.
        if COMMON_WORDS.contains(&info.word.as_str()) {
            eprintln!(
                "  WARNING: {rel} — pattern alternation '{alt}' is a common word; \
                 move it to vocabulary: (semantic lane) and keep pattern: for exact triggers (ADR-155 §5)"
            );
            *warnings += 1;
            continue;
        }

        // Rule 2: short and unanchored.
        if !info.is_anchored && info.char_len < SHORT_ALTERNATION_FLOOR {
            let n = info.char_len;
            let word = &info.word;
            eprintln!(
                "  WARNING: {rel} — pattern alternation '{alt}' is short ({n} chars) and unanchored; \
                 anchor it (\\b{word}\\b) if it's a term of art, or move it to vocabulary: (ADR-155 §5)"
            );
            *warnings += 1;
        }
    }
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

/// Drop a leading whole-pattern inline flag group (`(?i)`, `(?ims)`, `(?i-u)`)
/// so it is not misread as part of the first alternation. A non-capturing
/// `(?:…)` or named `(?P<…>…)` group is left intact (its inner content is real
/// pattern text).
fn strip_inline_flags(s: &str) -> &str {
    if let Some(rest) = s.strip_prefix("(?") {
        if let Some(close) = rest.find(')') {
            let flags = &rest[..close];
            if !flags.is_empty() && flags.chars().all(|c| c.is_ascii_alphabetic() || c == '-') {
                return &rest[close + 1..];
            }
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
    /// The literal word after anchor decoration is stripped, lowercased. Empty
    /// when the alternation is not a bare word.
    word: String,
    /// Character count of the literal word (Unicode scalar values, so accented
    /// and non-Latin tokens count correctly).
    char_len: usize,
    /// The alternation is a single literal word (only alphanumeric chars,
    /// optionally with anchor decoration and an optional trailing `?`) — not a
    /// phrase or structured regex.
    is_bare_word: bool,
    /// Carries word-boundary anchoring (`\b`, `^`/`$`, `(^| )`/`( |$)`, or a
    /// leading/trailing literal or escaped space).
    is_anchored: bool,
}

fn classify(alt: &str) -> AltInfo {
    // Detect anchoring on the untrimmed alternation so surrounding spaces
    // count as boundaries before they are trimmed off below.
    let is_anchored = detect_anchor(alt);

    // Reduce to the literal word: strip anchor decoration, then a single
    // trailing `?` (optional last letter, e.g. `ways?`). `\b`, `^`, `$`,
    // `(^| )`, `( |$)`, and boundary spaces are removed; anything else (`.`,
    // `*`, `+`, groups, classes, interior `?`) means the alternation is not a
    // bare word. Word chars are tested with Unicode `is_alphanumeric` so
    // localized patterns are classified, not silently skipped.
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
    let word_src = s.strip_suffix('?').unwrap_or(s);
    let is_bare_word = !word_src.is_empty() && word_src.chars().all(|c| c.is_alphanumeric());
    let (word, char_len) = if is_bare_word {
        (word_src.to_lowercase(), word_src.chars().count())
    } else {
        (String::new(), 0)
    };

    AltInfo {
        word,
        char_len,
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
        check_pattern(&"way", pattern, &HashSet::new(), &mut n);
        n
    }

    fn warnings_with_keep(pattern: &str, keep: &str) -> u32 {
        let mut n = 0;
        check_pattern(&"way", pattern, &parse_keep(keep), &mut n);
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
    fn anchored_common_word_still_flagged() {
        // \bcommit\b — the `\b` must not leak into the word; "commit" is still
        // a common word and should be flagged (regression for the core/body
        // corruption bug).
        assert_eq!(warnings_for(r"\bcommit\b"), 1);
        assert_eq!(warnings_for(r"\bdocs\b|diataxis"), 1);
    }

    #[test]
    fn inline_flag_group_stripped() {
        // (?i) applies to the whole pattern; the first alternation is "commit".
        assert_eq!(warnings_for("(?i)commit|remember"), 2);
        // Non-capturing groups are left intact (not a flag group): only the
        // bare `baz` warns. If `(?:…)` were wrongly stripped as flags, the
        // pattern would become `foo|bar|baz` and warn three times.
        assert_eq!(warnings_for("(?:foo|bar)|baz"), 1);
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
    fn space_anchored_middle_token_is_clean() {
        // A middle alternation anchored by surrounding spaces must not warn
        // (regression for anchor detection running before the trim).
        assert_eq!(warnings_for("longword| erd |otherword"), 0);
    }

    #[test]
    fn non_ascii_short_word_is_classified() {
        // Accented/non-Latin bare words must be seen, not silently skipped.
        assert_eq!(warnings_for("añ|entityname"), 1); // "añ" short + unanchored
    }

    #[test]
    fn inline_comment_stripped() {
        // serde_yaml drops ` # trigger`; lint must too, so "erd" is still seen.
        assert_eq!(warnings_for("pr|erd # trigger"), 2); // pr + erd, both short
    }

    #[test]
    fn lazy_wildcard_flagged_without_greedy_label() {
        // `.*?` is unbounded too; still flagged, but not mislabeled "greedy".
        let mut n = 0;
        // capture is not straightforward; just assert it fires exactly once.
        check_pattern(&"way", "alert.*?ack", &HashSet::new(), &mut n);
        assert_eq!(n, 1);
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

    #[test]
    fn pattern_keep_exempts_measured_common_word() {
        // 'slow' is a common word but a measured keep: pattern_keep silences
        // rule 1 for it, while other alternations stay checked.
        assert_eq!(warnings_for("slow|optimi|latency"), 1);
        assert_eq!(warnings_with_keep("slow|optimi|latency", "slow"), 0);
        // Multiple kept words (deps: package + library), whitespace-separated.
        assert_eq!(warnings_for("package|library|upgrade"), 3);
        assert_eq!(
            warnings_with_keep("package|library|upgrade", "package library"),
            1 // only 'upgrade' remains
        );
    }

    #[test]
    fn pattern_keep_exempts_short_token() {
        // A short unanchored term-of-art can be kept as-is if measured safe.
        assert_eq!(warnings_for("erd|entity"), 1);
        assert_eq!(warnings_with_keep("erd|entity", "erd"), 0);
    }

    #[test]
    fn pattern_keep_does_not_exempt_wildcard() {
        // The structural `.*` rule is never silenced, even if the word is kept.
        assert_eq!(warnings_with_keep("slow|save.*memory", "slow save"), 1);
    }

    #[test]
    fn pattern_keep_strips_inline_reason_comment() {
        // Authors annotate the keep with a reason; the comment is not a token.
        assert_eq!(
            warnings_with_keep("slow|optimi", "slow # measured: floor-gated"),
            0
        );
    }

    #[test]
    fn pattern_keep_is_case_insensitive_and_anchor_aware() {
        // Keep matches the lowercased bare word, so an anchored/cased keyword
        // is still exempted by its plain form.
        assert_eq!(warnings_with_keep(r"\bCommit\b", "commit"), 0);
    }
}
