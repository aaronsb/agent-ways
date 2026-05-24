//! Sentence-salience input reduction for embed matching (ADR-130).
//!
//! When a hook payload exceeds the embedding model's working window (MiniLM's
//! 128 position embeddings), we'd previously SIGABRT inside way-embed. The
//! interim fix capped inputs by character count in the hook scripts (PRs
//! #94/#95/#96); this module replaces that with a structural fix that keeps
//! the *high-signal* prose instead of clamping at an arbitrary char index.
//!
//! Algorithm: split into sentences → softmax over token frequencies across
//! the whole input → score each sentence by average softmax weight of its
//! tokens → keep top-N sentences within a token budget → reassemble in
//! document order. Inputs that already fit pass through unchanged.
//!
//! Failure modes are deliberate:
//! - Non-prose input (one sentence detected) → fall back to bag-of-words
//!   top-K selection on tokens directly (the Shape A path from the design
//!   conversation).
//! - Any internal error → return the input unchanged. The truncation caps
//!   in the hook scripts are the belt-and-suspenders fallback.
//!
//! No corpus involvement here. This is input preparation only; ADR-125's
//! embedding-only matcher contract is untouched.

use std::collections::HashMap;

/// Approximate-tokens-per-char ratio used for sizing. MiniLM's WordPiece
/// segments ~4 chars/token on English prose; we round down for headroom.
const CHARS_PER_TOKEN: usize = 4;

/// Minimal scaffold-aware stopword list. Standard English fillers plus
/// the words that dominate agent dispatch prompts in observed crash
/// payloads (`agent`, `task`, `prompt`, `context`, etc.). Kept small;
/// grows empirically based on observed scaffold dominance.
const STOPWORDS: &[&str] = &[
    // Standard English
    "the", "a", "an", "and", "or", "but", "is", "are", "was", "were", "be",
    "been", "being", "have", "has", "had", "do", "does", "did", "will",
    "would", "should", "could", "may", "might", "must", "can", "to", "of",
    "in", "on", "at", "by", "for", "with", "as", "from", "into", "about",
    "this", "that", "these", "those", "it", "its", "if", "then", "than",
    "you", "your", "we", "our", "they", "their", "i", "me", "my", "not",
    "no", "yes", "so", "up", "down", "out", "all", "any", "some", "each",
    "such", "only", "own", "same", "other", "more", "most", "very", "just",
    // Agent dispatch / hook scaffold tokens — these dominate the
    // frequency tail in structured prompts and drown out topical signal.
    "agent", "task", "prompt", "context", "return", "description",
    "subagent", "tool", "tools", "use", "using", "user", "claude",
    "session", "project", "file", "files", "code", "please",
    // PR / git workflow scaffold tokens
    "pr", "branch", "diff", "merge", "review", "body", "title", "summary",
    "scope", "report", "final", "plan", "approved", "approve", "result",
    "verdict", "blocker", "blockers", "suggestion", "suggestions",
    "nit", "nits", "ship",
];

/// Reduce `input` to fit within `budget_tokens` approximate tokens, using
/// sentence-salience scoring when possible. Returns the input unchanged
/// when it already fits or when reduction can't be applied meaningfully.
pub fn reduce_for_embed(input: &str, budget_tokens: usize) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if approx_tokens(trimmed) <= budget_tokens {
        return trimmed.to_string();
    }

    let sentences = split_sentences(trimmed);

    // Non-prose fallback: degrade to bag-of-words token selection.
    if sentences.len() <= 1 {
        return reduce_by_tokens(trimmed, budget_tokens);
    }

    reduce_by_sentences(trimmed, &sentences, budget_tokens)
}

fn approx_tokens(s: &str) -> usize {
    // Cheap upper bound. Three estimators, take the max so we never
    // under-budget:
    //
    //   by_words  — whitespace-separated runs. Tight on English prose.
    //   by_chars  — chars / CHARS_PER_TOKEN. Catches space-less identifier
    //               soup (concatenated symbols, URLs, code).
    //   by_cjk    — CJK ideographs and kana count ~1 WordPiece token per
    //               char in the MiniLM tokenizer, so for CJK input
    //               by_words and by_chars/4 both *severely* under-count.
    //               Without this term, a 400-char Japanese input would
    //               estimate at ~100 tokens but produce ~400 real tokens,
    //               past the 128-position-embedding window → SIGABRT.
    //
    // The reducer's job is to never lie to the embedder about how big
    // the embed input is. Conservative estimation here is mandatory.
    let by_words = s.split_whitespace().count();
    let by_chars = s.chars().count() / CHARS_PER_TOKEN;
    let by_cjk = s.chars().filter(|c| is_cjk(*c)).count();
    by_words.max(by_chars).max(by_cjk)
}

/// True for code points the MiniLM tokenizer treats as one-token-per-
/// character: CJK Unified Ideographs and extensions, Hiragana, Katakana,
/// Hangul. Conservative (matches more than strictly necessary) by design.
fn is_cjk(c: char) -> bool {
    let cp = c as u32;
    (0x3040..=0x309F).contains(&cp)   // Hiragana
        || (0x30A0..=0x30FF).contains(&cp) // Katakana
        || (0x3400..=0x4DBF).contains(&cp) // CJK Unified Ideographs Extension A
        || (0x4E00..=0x9FFF).contains(&cp) // CJK Unified Ideographs
        || (0xAC00..=0xD7AF).contains(&cp) // Hangul Syllables
        || (0xF900..=0xFAFF).contains(&cp) // CJK Compatibility Ideographs
        || (0x20000..=0x2A6DF).contains(&cp) // CJK Unified Ideographs Extension B
}

fn tokenize(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
        .filter(|t| !t.is_empty())
        .map(|t| t.to_lowercase())
        .filter(|t| !STOPWORDS.contains(&t.as_str()))
        .collect()
}

/// Split on sentence-ending punctuation followed by whitespace, plus
/// double-newline boundaries. Cheap and good enough for prose; falls
/// through to single-element on code/JSON/heredoc inputs (which the
/// caller handles via the fallback path).
fn split_sentences(s: &str) -> Vec<&str> {
    let mut sentences = Vec::new();
    let bytes = s.as_bytes();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        let is_sentence_end = (c == b'.' || c == b'!' || c == b'?')
            && i + 1 < bytes.len()
            && bytes[i + 1].is_ascii_whitespace();
        let is_paragraph_break = c == b'\n'
            && i + 1 < bytes.len()
            && bytes[i + 1] == b'\n';

        if is_sentence_end {
            // Include the terminating punctuation in the sentence.
            if start <= i + 1 {
                let chunk = s[start..i + 1].trim();
                if !chunk.is_empty() {
                    sentences.push(chunk);
                }
            }
            // Advance past the punctuation AND the following whitespace
            // so the next iteration doesn't re-process the whitespace
            // and trigger a paragraph-break with start > end.
            start = i + 2;
            i += 2;
            continue;
        }
        if is_paragraph_break {
            if start <= i {
                let chunk = s[start..i].trim();
                if !chunk.is_empty() {
                    sentences.push(chunk);
                }
            }
            start = i + 2;
            i += 2;
            continue;
        }
        i += 1;
    }
    if start < bytes.len() {
        let tail = s[start..].trim();
        if !tail.is_empty() {
            sentences.push(tail);
        }
    }
    sentences
}

fn softmax(counts: &HashMap<String, f64>) -> HashMap<String, f64> {
    if counts.is_empty() {
        return HashMap::new();
    }
    // Subtract max for numerical stability — standard softmax trick.
    let max = counts.values().cloned().fold(f64::NEG_INFINITY, f64::max);
    let exps: HashMap<String, f64> = counts
        .iter()
        .map(|(k, v)| (k.clone(), (v - max).exp()))
        .collect();
    let sum: f64 = exps.values().sum();
    if sum == 0.0 {
        return HashMap::new();
    }
    exps.into_iter().map(|(k, v)| (k, v / sum)).collect()
}

fn count_frequencies(tokens: &[String]) -> HashMap<String, f64> {
    let mut counts: HashMap<String, f64> = HashMap::new();
    for t in tokens {
        *counts.entry(t.clone()).or_insert(0.0) += 1.0;
    }
    counts
}

fn reduce_by_sentences(input: &str, sentences: &[&str], budget_tokens: usize) -> String {
    let all_tokens = tokenize(input);
    let counts = count_frequencies(&all_tokens);
    let weights = softmax(&counts);

    let mut scored: Vec<(usize, &str, f64)> = sentences
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let tokens = tokenize(s);
            let n = tokens.len().max(1) as f64;
            let salience: f64 = tokens.iter().filter_map(|t| weights.get(t)).sum::<f64>() / n;
            (i, *s, salience)
        })
        .collect();

    // Descending salience for selection.
    scored.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

    let mut keep: Vec<(usize, &str)> = Vec::new();
    let mut accumulated = 0usize;
    for (i, s, _) in &scored {
        let s_tokens = approx_tokens(s);
        if accumulated + s_tokens > budget_tokens {
            // `continue` not `break`: a sentence further down the
            // salience-sorted list might be short enough to still fit
            // even if this one didn't. Skipping preserves total budget
            // utilization at the cost of one extra comparison per skip.
            continue;
        }
        keep.push((*i, *s));
        accumulated += s_tokens;
    }

    // If nothing fit (single sentence longer than budget), degrade to token
    // selection on the highest-scored sentence rather than emitting empty.
    if keep.is_empty() {
        let best = scored.first().map(|(_, s, _)| *s).unwrap_or(input);
        return reduce_by_tokens(best, budget_tokens);
    }

    // Document order restores prose coherence for the embedder.
    keep.sort_by_key(|(i, _)| *i);
    keep.into_iter()
        .map(|(_, s)| s)
        .collect::<Vec<_>>()
        .join(" ")
}

fn reduce_by_tokens(input: &str, budget_tokens: usize) -> String {
    // Defensive: caller contract says budget should be ≥1, but a 0 budget
    // would silently produce empty output and break the "non-empty input
    // never returns empty" invariant. Treat as "no reduction needed."
    if budget_tokens == 0 {
        return input.to_string();
    }
    let tokens = tokenize(input);
    if tokens.is_empty() {
        // Pathological: nothing tokenizable left after stopwords. Fall
        // back to a hard char-budget slice so we never return empty on
        // non-empty input.
        let max_chars = budget_tokens * CHARS_PER_TOKEN;
        return input.chars().take(max_chars).collect();
    }
    let counts = count_frequencies(&tokens);
    let weights = softmax(&counts);

    // Walk tokens in document order, keep the highest-weight ones up to
    // budget. This preserves local ordering (not perfect — duplicates of
    // a high-weight token all survive — but it keeps the embed input
    // looking like a phrase rather than a sorted bag).
    let mut indexed: Vec<(usize, &String)> = tokens.iter().enumerate().collect();
    indexed.sort_by(|a, b| {
        let wa = weights.get(a.1).copied().unwrap_or(0.0);
        let wb = weights.get(b.1).copied().unwrap_or(0.0);
        wb.partial_cmp(&wa).unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut keep_indices: Vec<usize> = indexed
        .into_iter()
        .take(budget_tokens)
        .map(|(i, _)| i)
        .collect();
    keep_indices.sort_unstable();

    keep_indices
        .into_iter()
        .map(|i| tokens[i].as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_when_under_budget() {
        let input = "short user question about the codebase";
        let out = reduce_for_embed(input, 100);
        assert_eq!(out, input);
    }

    #[test]
    fn empty_input_returns_empty() {
        assert_eq!(reduce_for_embed("", 100), "");
        assert_eq!(reduce_for_embed("   \n\t  ", 100), "");
    }

    #[test]
    fn prose_reduces_to_high_salience_sentences() {
        // Repeated reference to "deploy" should make the deployment
        // sentences more salient than the unrelated ones.
        let input = "We need to deploy the new build. \
                     My cat is asleep on the keyboard. \
                     The deploy pipeline runs on staging first. \
                     Coffee is good in the morning. \
                     Each deploy step is logged. \
                     There is a thunderstorm tonight.";
        let out = reduce_for_embed(input, 20);
        // Output must be shorter than input
        assert!(out.len() < input.len());
        // Salient term must survive
        assert!(out.to_lowercase().contains("deploy"));
        // At least one unrelated sentence should be dropped
        let unrelated_count = ["cat", "coffee", "thunderstorm"]
            .iter()
            .filter(|w| out.contains(*w))
            .count();
        assert!(unrelated_count < 3, "expected some unrelated sentences dropped; got: {out}");
    }

    #[test]
    fn document_order_preserved() {
        let input = "Alpha sentence about deploy. \
                     Beta sentence is unrelated. \
                     Gamma sentence also about deploy and build. \
                     Delta sentence is unrelated. \
                     Epsilon sentence about deploy pipeline.";
        let out = reduce_for_embed(input, 30);
        let alpha = out.find("Alpha");
        let gamma = out.find("Gamma");
        let epsilon = out.find("Epsilon");
        if let (Some(a), Some(g)) = (alpha, gamma) {
            assert!(a < g, "Alpha must come before Gamma in output");
        }
        if let (Some(g), Some(e)) = (gamma, epsilon) {
            assert!(g < e, "Gamma must come before Epsilon in output");
        }
    }

    #[test]
    fn non_prose_input_falls_back_to_token_selection() {
        // A bash heredoc-style payload — no sentence boundaries. The
        // fallback's contract is "bound the size, provide some signal" —
        // not "extract topicality from a structureless blob," which
        // bag-of-words cannot promise without repetition of the
        // topical terms themselves.
        let input = "gh pr create --title \"fix(scope): description\" \
                     --body \"$(cat <<EOF this is a very long body \
                     without any sentence punctuation at all just words \
                     and more words about the deploy pipeline staging \
                     production rollout EOF)\"";
        let out = reduce_for_embed(input, 15);
        assert!(out.len() < input.len(), "should be reduced");
        assert!(!out.is_empty(), "fallback must not return empty");
        assert!(approx_tokens(&out) <= 30, "should respect budget approximately");
    }

    #[test]
    fn non_prose_with_repeated_topical_terms_keeps_them() {
        // When topical terms ARE the repeated content (which is what
        // happens in real bash commands — `gh pr create` repeated across
        // a session, or `deploy` appearing in multiple flags), the
        // bag-of-words fallback does the right thing.
        let input = "deploy deploy deploy deploy build build build build \
                     scaffold scaffold scaffold scaffold scaffold";
        let out = reduce_for_embed(input, 5);
        assert!(out.contains("deploy") || out.contains("build") || out.contains("scaffold"),
                "expected repeated term to survive: {out}");
    }

    #[test]
    fn budget_is_respected() {
        let input = "Repeating the deploy. Repeating the deploy. \
                     Repeating the deploy. Repeating the deploy. \
                     Repeating the deploy. Repeating the deploy.";
        let out = reduce_for_embed(input, 5);
        assert!(approx_tokens(&out) <= 10, "expected ≤10 tokens, got {} from: {out}", approx_tokens(&out));
    }

    #[test]
    fn single_long_sentence_does_not_return_empty() {
        // One sentence longer than budget — must degrade to token
        // selection on it rather than returning empty.
        let input = "this is a single very long sentence about deploying the pipeline to staging and then to production with all the various steps that happen along the way to make sure everything goes smoothly without any incidents";
        let out = reduce_for_embed(input, 10);
        assert!(!out.is_empty());
        assert!(approx_tokens(&out) <= 20);
    }

    #[test]
    fn stopwords_filtered_from_tokenization() {
        let tokens = tokenize("the agent will return the prompt context");
        // All of those are in STOPWORDS — should be empty after filter.
        assert!(tokens.is_empty(), "expected all stopwords filtered, got: {tokens:?}");
    }

    #[test]
    fn tokenize_handles_punctuation_and_case() {
        let tokens = tokenize("Deploy-Pipeline, staging/production: rollout!");
        // Stopwords removed; hyphen and underscore preserved inside identifiers.
        assert!(tokens.contains(&"deploy-pipeline".to_string()));
        assert!(tokens.contains(&"staging".to_string()));
        assert!(tokens.contains(&"production".to_string()));
        assert!(tokens.contains(&"rollout".to_string()));
    }

    #[test]
    fn split_sentences_basic() {
        let sentences = split_sentences("First sentence. Second sentence! Third one?");
        assert_eq!(sentences.len(), 3);
    }

    #[test]
    fn split_sentences_paragraph_break() {
        let sentences = split_sentences("First paragraph here\n\nSecond paragraph now");
        assert_eq!(sentences.len(), 2);
    }

    #[test]
    fn split_sentences_returns_one_on_non_prose() {
        let sentences = split_sentences("no-punctuation-and-no-paragraph-breaks");
        assert_eq!(sentences.len(), 1);
    }

    #[test]
    fn cjk_input_estimates_one_token_per_char() {
        // Japanese: each kanji/kana is ~1 WordPiece token in MiniLM.
        // ~50 chars of Japanese should NOT estimate as ~12 tokens
        // (chars/4) — it must estimate at least ~50.
        let jp = "成果物の鮮度について何かを記述または派生しているが、それに遅れをとっているファイルを浮かび上がらせる。";
        let est = approx_tokens(jp);
        assert!(est >= jp.chars().count() / 2,
                "Japanese should estimate near char-count, got {est} for {} chars",
                jp.chars().count());

        // Chinese: same property.
        let zh = "制品新鲜度发现那些描述或派生自其他事物但已落后于它的文件";
        let est_zh = approx_tokens(zh);
        assert!(est_zh >= zh.chars().count() / 2,
                "Chinese should estimate near char-count, got {est_zh}");

        // English: estimate stays close to the word-count (no CJK
        // inflation). Exact value is max(words, chars/4) — for this
        // sentence that's max(9, 10) = 10, well under the 35× inflation
        // CJK would have caused if treated naively.
        let en = "the quick brown fox jumps over the lazy dog";
        let en_est = approx_tokens(en);
        assert!(en_est <= 11, "English estimate should stay near word count, got {en_est}");
    }

    #[test]
    fn cjk_long_input_triggers_reduction() {
        // ~400 chars of Japanese. Pre-fix, this would estimate as ~100
        // tokens via chars/4 and bypass the reducer at budget=110, then
        // explode the embedder. Post-fix, it estimates near 400 and
        // properly triggers reduction.
        let jp = "成果物の鮮度について何かを記述または派生しているが、それに遅れをとっているファイルを浮かび上がらせる。".repeat(4);
        assert!(approx_tokens(&jp) > 110, "must exceed budget to trigger reduction");
        let out = reduce_for_embed(&jp, 110);
        assert!(out.chars().count() < jp.chars().count(), "must actually reduce");
    }

    #[test]
    fn budget_zero_does_not_return_empty() {
        // Defensive contract: never produce empty output from non-empty
        // input, even at the pathological budget = 0.
        let out = reduce_by_tokens("nonempty input here", 0);
        assert!(!out.is_empty());
    }

    #[test]
    fn softmax_distribution_sums_to_one() {
        let mut counts = HashMap::new();
        counts.insert("a".to_string(), 3.0);
        counts.insert("b".to_string(), 1.0);
        counts.insert("c".to_string(), 1.0);
        let weights = softmax(&counts);
        let sum: f64 = weights.values().sum();
        assert!((sum - 1.0).abs() < 1e-9, "softmax should sum to 1.0, got {sum}");
        // High count gets high weight.
        assert!(weights["a"] > weights["b"]);
    }

    // ── Behavioral tests against actual crash repros ──────────────

    /// Mimics PID 128657 / 156817 — vue-expert agent dispatch.
    fn crash_repro_vue_expert_prompt() -> &'static str {
        "you're standing in for the project's vue-expert agent — the project's \
         claude.md mandates vue 3 work be done through vue-expert (defined at \
         /home/foo/projects/inventory/.claude/agents/vue-expert.md). \
         Read that agent file first to align with its rules (script setup \
         patterns, scope, recipes), then proceed. \
         \n\nTask: augment client/src/views/orders.vue to add a submitted \
         orders section showing restocking orders submitted via the new \
         /api/restocking/orders endpoint. This is the still-owed half of \
         step 6 (budget-based restocking) from a workshop the user is going \
         through. \
         \n\nContext: the restocking feature was built earlier this session. \
         Backend endpoints exist: GET /api/restocking/orders returns the \
         list of submitted orders. Each submitted order has shape: \
         {id, submitted_at, budget, total, lead_time_days, status, items}. \
         The new section should sit above the existing all-orders card. \
         Match the existing orders.vue style with card / card-header / table. \
         \n\nFiles you'll touch: client/src/views/orders.vue (add template \
         section, add loader, add ref, expose to template), \
         client/src/locales/en.js (add orders.submitted block), \
         client/src/locales/ja.js (mirror with Japanese strings). \
         \n\nFinal report: return a concise summary (under 250 words) \
         covering the exact list of files changed, the new refs and methods \
         you added in orders.vue setup, the new i18n keys, whether the smoke \
         test passed, and anything you noticed the user should know."
    }

    /// Mimics PID 88563 / 197729 — gh pr create with heredoc body.
    fn crash_repro_gh_pr_heredoc() -> &'static str {
        r#"gh pr create --title "fix(scan): cap bash embed query at 256 chars" --body "$(cat <<'EOF'
## Summary

Bash tool inputs can carry kilobyte-scale heredoc bodies, JSON payloads,
and multi-line PR descriptions. All of those overrun the MiniLM embedding
model's position-embedding table and abort way-embed.

## Diagnosis

The signal that distinguishes one bash command from another lives in the
program name and first few args. Heredoc bodies carry no signal for
'what kind of command is this'. Truncate the command at 256 chars before
passing to scan command.

## Test plan

- ls -la (short cmd) tested
- 4845-char heredoc command tested with zero crashes
- bash -n clean
- All existing commands regex patterns under 106 chars
EOF
)""#
    }

    /// Mimics PID 197729 — task-completion notification through prompt hook.
    fn crash_repro_task_notification() -> &'static str {
        "<task-notification>\
         <task-id>ad51ec1d3fe1b72df</task-id>\
         <status>completed</status>\
         <summary>Agent 'Review PR #94 (skip embed for custom agents)' completed</summary>\
         <result>Review posted to PR #94. Approved with two non-blocking notes \
         about subagent_type input validation. The discriminator correctly skips \
         custom-agent dispatches; built-in subagent types still spawn way-embed \
         as designed. Recommended a regex guard so subagent_type cannot \
         glob-expand or path-traverse when used as a path component. Compgen \
         alternative to shopt-toggle was suggested for the plugin path glob. \
         Verdict: approve, ship it.</result></task-notification>"
    }

    #[test]
    fn behavior_vue_expert_dispatch_reduces_safely() {
        let input = crash_repro_vue_expert_prompt();
        let out = reduce_for_embed(input, BUDGET_TASK_TEST);
        assert!(!out.is_empty());
        assert!(approx_tokens(&out) <= BUDGET_TASK_TEST + 20, "got {} tokens", approx_tokens(&out));
        // Domain terms must survive — multiple sentences mention vue, agent,
        // orders, restocking, so at least one should appear.
        let has_vue = out.to_lowercase().contains("vue");
        let has_orders = out.to_lowercase().contains("orders");
        let has_restocking = out.to_lowercase().contains("restocking");
        assert!(has_vue || has_orders || has_restocking,
                "expected a domain term to survive, got: {out}");
    }

    #[test]
    fn behavior_gh_pr_heredoc_does_not_crash() {
        let input = crash_repro_gh_pr_heredoc();
        let out = reduce_for_embed(input, BUDGET_COMMAND_TEST);
        assert!(!out.is_empty());
        assert!(approx_tokens(&out) <= BUDGET_COMMAND_TEST + 20);
    }

    #[test]
    fn behavior_task_notification_reduces_safely() {
        let input = crash_repro_task_notification();
        let out = reduce_for_embed(input, BUDGET_PROMPT_TEST);
        assert!(!out.is_empty());
        assert!(approx_tokens(&out) <= BUDGET_PROMPT_TEST + 20);
    }

    // Budget constants for tests — mirror the module-level budgets in
    // scan/mod.rs but kept here to avoid coupling test code to that file's
    // private namespace.
    const BUDGET_PROMPT_TEST: usize = 110;
    const BUDGET_TASK_TEST: usize = 110;
    const BUDGET_COMMAND_TEST: usize = 75;

    // ── Manual recall validation (ADR-130 merge gate) ────────────
    //
    // Ignored by default. Run with:
    //   cargo test -p ways --release -- --ignored validate_recall_against_live_corpus --nocapture
    //
    // Requires: ~/.cache/claude-ways/user/ways-corpus-multi.jsonl,
    //           ~/.cache/claude-ways/user/multilingual-minilm-l12-v2-q8.gguf,
    //           ~/.claude/bin/way-embed.
    //
    // Spawns way-embed match against the live corpus for each query
    // twice — once with the full input, once with the reducer's output.
    // Reports top-1 agreement %. Merge gate: ≥ 90%.

    /// Production-faithful top-1: returns the highest-scoring way whose
    /// score clears the production threshold for its model. Returns
    /// None when nothing clears — which is the *correct* outcome when
    /// the corpus has no good match for the query.
    fn top1_fires(query: &str) -> Option<String> {
        let home = std::env::var("HOME").ok()?;
        let bin = format!("{home}/.claude/bin/way-embed");

        // EN model: threshold 0.40 (production default_embed_threshold).
        let en_corpus = format!("{home}/.cache/claude-ways/user/ways-corpus-en.jsonl");
        let en_model = format!("{home}/.cache/claude-ways/user/minilm-l6-v2.gguf");
        let en_best = best_match(&bin, &en_corpus, &en_model, query, 0.40);

        // Multi model: threshold 0.55 (production default_multi_embed_threshold).
        let multi_corpus = format!("{home}/.cache/claude-ways/user/ways-corpus-multi.jsonl");
        let multi_model = format!("{home}/.cache/claude-ways/user/multilingual-minilm-l12-v2-q8.gguf");
        let multi_best = best_match(&bin, &multi_corpus, &multi_model, query, 0.55);

        // match_prompt in production fires EN first if EN clears, else multi.
        en_best.or(multi_best)
    }

    fn best_match(
        bin: &str,
        corpus: &str,
        model: &str,
        query: &str,
        threshold: f64,
    ) -> Option<String> {
        use std::process::Command;
        let out = Command::new(bin)
            .args(["match", "--corpus", corpus, "--model", model,
                   "--query", query, "--threshold", "0.0"])
            .output().ok()?;
        if !out.status.success() { return None; }
        let stdout = String::from_utf8_lossy(&out.stdout);
        let mut best: Option<(String, f64)> = None;
        for line in stdout.lines() {
            let mut parts = line.split('\t');
            let id = parts.next()?.to_string();
            let score: f64 = parts.next()?.parse().ok()?;
            if best.as_ref().is_none_or(|(_, s)| score > *s) {
                best = Some((id, score));
            }
        }
        best.and_then(|(id, s)| if s >= threshold { Some(id) } else { None })
    }

    #[test]
    #[ignore]
    fn validate_recall_against_live_corpus() {
        let queries: Vec<(&str, &str, usize)> = vec![
            ("vue agent dispatch",
             "you're standing in for the project's vue-expert agent — the project's claude.md mandates vue 3 work be done through vue-expert. Read that agent file first to align with its rules (script setup patterns, scope, recipes), then proceed. Task: augment client/src/views/orders.vue to add a submitted orders section showing restocking orders submitted via the new api/restocking/orders endpoint. The new section should sit above the existing all-orders card. Match the existing orders.vue style with card / card-header / table. Files you'll touch: client/src/views/orders.vue, client/src/locales/en.js, client/src/locales/ja.js. Final report: return a concise summary covering the exact list of files changed.",
             BUDGET_TASK_TEST),
            ("gh pr create heredoc",
             "gh pr create --title \"fix(scan): cap bash embed query at 256 chars\" --body \"$(cat <<EOF Summary: Bash tool inputs can carry kilobyte-scale heredoc bodies and JSON payloads. All overrun the embedding model and abort way-embed. Diagnosis: signal lives in program name and first few args. Heredoc bodies carry no signal. Test plan: tested with zero crashes, bash -n clean, regex patterns under 106 chars. EOF)\"",
             BUDGET_COMMAND_TEST),
            ("task notification",
             "<task-notification><task-id>abc</task-id><status>completed</status><summary>Agent Review PR 94 (skip embed for custom agents) completed</summary><result>Review posted to PR 94. Approved with two non-blocking notes about subagent_type input validation. Discriminator correctly skips custom-agent dispatches. Recommended a regex guard so subagent_type cannot glob-expand or path-traverse when used as a path component. Verdict: approve, ship it.</result></task-notification>",
             BUDGET_PROMPT_TEST),
            ("debugging question (short)",
             "I'm trying to understand why this Rust program is panicking on slice bounds. It happens when processing user input that contains both sentence-ending punctuation followed by a paragraph break. Should I add a defensive check or restructure the loop?",
             BUDGET_PROMPT_TEST),
            ("schema migration question",
             "What's the safest way to roll out a database schema migration to production when the table has 50 million rows? I want to avoid locking the table for long.",
             BUDGET_PROMPT_TEST),
            ("test structure question",
             "How should I structure unit tests for a tokenizer that has fallback paths for non-prose input? I want to test both the happy path and the fallback without coupling the test code to internal helper functions.",
             BUDGET_PROMPT_TEST),
            ("kubectl long args",
             "kubectl apply -f manifests/staging/deployment.yaml --validate=strict --field-manager=ci --server-side --force-conflicts --field-validation=Strict --dry-run=server --output=yaml",
             BUDGET_COMMAND_TEST),
            ("code review request",
             "Review PR 95 on aaronsb/agent-ways. Branch is fix/cap-bash-embed-query. Diff against main. What this fix does: modifies hooks/ways/check-bash-pre.sh to truncate the bash command at 256 chars before passing to ways scan command. The truncation happens before both the regex commands matcher and the embed-based check matcher. Look for: is 256 the right cap? UTF-8 mid-codepoint concern? Bash parameter expansion correctness? Docstring honesty about the trade-off? Be specific about file:line for any concern.",
             BUDGET_TASK_TEST),
        ];

        let mut agree = 0;
        let n = queries.len();
        eprintln!("\n── ADR-130 recall validation (production-threshold semantics) ──\n");
        eprintln!("{:32}  {:28}  {:28}  agree", "case", "full fires", "reduced fires");
        for (label, full, budget) in &queries {
            let reduced = reduce_for_embed(full, *budget);
            let full_fires = top1_fires(full).unwrap_or_else(|| "(none)".to_string());
            let reduced_fires = top1_fires(&reduced).unwrap_or_else(|| "(none)".to_string());
            let ok = full_fires == reduced_fires;
            if ok { agree += 1; }
            eprintln!("{label:32}  {full_fires:28}  {reduced_fires:28}  {}",
                      if ok { "✓" } else { "✗" });
        }
        let pct = agree * 100 / n;
        eprintln!("\nFiring-set agreement: {agree}/{n} ({pct}%)");
        eprintln!("  (None == None counts as agreement — production semantics.)");
        eprintln!("Merge gate: ≥ 90%\n");
        assert!(pct >= 90, "recall {}% below 90% gate", pct);
    }

    /// Recall property: the high-frequency content terms from the input
    /// must appear in the reduced output. This is the in-process proxy
    /// for top-1 match recall — if the reducer keeps the terms the embed
    /// would weight highest, the embed match downstream is unlikely to
    /// drift to a different way.
    #[test]
    fn high_frequency_terms_survive_reduction() {
        let input = "deploy the new build to staging. \
                     The deploy pipeline checks must pass. \
                     My cat is asleep. \
                     Each deploy step writes to the build log. \
                     Coffee is good. \
                     Build artifacts get tagged in the registry. \
                     Thunderstorm tonight. \
                     The deploy completes when the build hash matches.";
        let out = reduce_for_embed(input, 30);
        // "deploy" and "build" appear 4 and 3 times respectively — they
        // MUST survive. The unrelated terms (cat, coffee, thunderstorm)
        // appear once each and may or may not survive depending on
        // sentence-level salience.
        assert!(out.to_lowercase().contains("deploy"), "deploy must survive: {out}");
        assert!(out.to_lowercase().contains("build"), "build must survive: {out}");
    }
}
