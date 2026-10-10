---
description: the keyword lane of a way — adding a pattern regex beside semantic matching, the floor-gated fire rule, pattern_strict and pattern_keep, and pattern hygiene lint findings
vocabulary: pattern regex keyword lane floor gate pattern_strict pattern_keep hygiene anchor term-of-art wildcard unbounded common-word fail-open case-insensitive unconditional lowercase pattern-hygiene calibration
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# The Keyword Lane

**When to add `pattern:` alongside semantic:** As a supplementary trigger for exact terms you never want to miss on the strong-signal path. The two lanes are **not** an unconditional OR. A way fires when `g(s) ≥ τ_s ∨ (keyword_match ∧ g(s) ≥ τ_k)`, where `g(s) = σ(a·s + b)` is the calibrated relevance probability (ADR-156), `τ_s = 0.5` is the semantic bar, and `τ_k = 0.15` is the keyword floor. The keyword lane is **floor-gated**: a pattern hit only fires when the semantic probability already clears `τ_k`, so a keyword can't drag in an unrelated prompt — *as long as calibration is loaded.* With no calibrated signal (a non-embeddable way, the engine not run, or no calibration present) the keyword lane **fails open** and fires unconditionally, so the author's explicit trigger still stands. If you want a keyword to fire unconditionally *by design* — bypassing the gate even when calibrated — set `pattern_strict: true`. That is the only way to guarantee a keyword fires regardless of semantic signal.

**Keep `pattern:` clean — this is how future ways stay compatible.** Reserve the keyword lane for *specific, term-of-art* triggers (`erd`, `mttr`, exact command or file names). Suggestive or common words ("optimize", "review", "guidance", "knowledge") belong in `vocabulary:`, where the semantic lane weighs them in context. A bare common word in `pattern:` fires on unrelated prose and only avoids leaking because the `τ_k` floor happens to gate it — don't rely on that. `ways author lint` flags common-word, short-unanchored, and unbounded-`.*` alternations for exactly this reason (ADR-155 §5); fix them by moving the term to `vocabulary:`, anchoring a genuine term of art (`\berd\b`), or bounding the wildcard (`.{0,30}`). This keeps a way about a specific topic from firing during unrelated work — the semantic lane is the topic isolation. The keyword lane is **case-insensitive** (ADR-157), so write patterns in lowercase and mean the concept — `\bpr\b` matches the `PR` a user types, no `(?i)` needed.

## Fields

- `pattern:` - Regex matched against user prompts
- `pattern_strict:` - `true` makes a pattern hit fire **unconditionally**, bypassing the `τ_k` floor gate. The only way to guarantee a keyword fires regardless of semantic signal. Use sparingly.
- `pattern_keep:` - Space-separated list exempting a *measured* common-word keep from the pattern-hygiene lint (ADR-155 §5). Use only when the keyword is load-bearing and its off-sense noise is floor-gated by `τ_k` — measure before adding it.

## See Also

- knowledge/authoring(meta) — parent: way format and the semantic lane
- knowledge/optimization(meta) — measuring a way's matching before and after a change
