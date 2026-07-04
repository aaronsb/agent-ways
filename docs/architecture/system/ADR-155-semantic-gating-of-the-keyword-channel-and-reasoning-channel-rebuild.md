---
status: Draft
date: 2026-07-04
deciders:
  - aaronsb
  - claude
related:
  - 108
  - 125
  - 130
  - 134
  - 153
---

# ADR-155: Semantic gating of the keyword channel and reasoning-channel rebuild

## Context

ADR-125 named the matching architecture: retrieval is embedding-only, the model's
cosine score is accepted as ground truth, and the `pattern:`/`commands:` regex
fields survive as *explicit deterministic triggers* — "fire this way exactly when
this pattern appears." In practice the keyword channel has drifted from that
role into fuzzy retrieval, and the code gives it veto-proof authority.

### How matching works today

In `match_prompt` (`tools/ways-cli/src/cmd/scan/mod.rs`), the regex pattern is
checked first. Any substring hit anywhere in the prompt fires the way
deterministically; the embedding is never consulted for that way. The embedding
is a fallback that can only *add* fires, never veto a lexical coincidence —
even though `batch_embed_score` has already scored **every** way against the
prompt before the candidate loop starts. The signal that would separate
incidental keywords from topical intent is computed, then discarded.

Two adjacent surfaces compound this:

- **PreToolUse** (`command()`/`file()`): ways match by regex only (`commands`
  against the command string, `pattern` against the tool description). The
  semantic lane exists on that surface but is wired only to *checks*. Where
  Claude is about to act — the moment course-correction is most valuable —
  ways have no semantic channel at all.
- **Stop hook** (`check-response.sh`): Claude's response is reduced to a grep
  against a hard-coded 24-word whitelist (`api|test|debug|…|way|pr|…`), and the
  surviving tokens are concatenated into the **next prompt's match query** —
  including the regex channel. The whitelist words are, by construction, way
  trigger words. If Claude's reply mentioned "way" or "pr", the next user
  prompt keyword-fires `meta/knowledge` or `softwaredev/delivery/github`
  regardless of what the user typed. The channel meant to give ways awareness
  of Claude's reasoning is simultaneously too blind to capture it (24 fixed
  words) and a noise injector into the keyword channel.

### Observed failure (session introspection, ADR-153)

A live session in a TUI-oscilloscope project fired five ways on one pasted
prompt. Rescoring that exact prompt against the corpus:

| Way | fired via | EN embed score | topically relevant |
|---|---|---:|---|
| softwaredev/visualization | semantic | 0.40 | yes |
| softwaredev/visualization/charts | keyword `graph` | 0.28 | yes |
| meta/knowledge | keyword ` ways ` ("btop's various ways to render…") | 0.22 | no |
| meta/memory | keyword `remember` ("I remember the tektronix…") | 0.15 | no |
| softwaredev/delivery/github | keyword `github` (a pasted URL) | 0.09 | no |

The inverse test — short prompts with genuine intent — scores high on the
target way: "adr" → 0.47 (documentation/adr), "remember this decision for
later sessions" → 0.35 (meta/memory), "let's look at the github pr checks" →
0.57 (delivery/github), "can you make a chart of the results" → 0.51
(visualization/charts). The embedding rank-orders relevance correctly in every
observed case; a floor near half the fire threshold cleanly separates
coincidence from intent. **The model already knows which keyword fires are
junk; the scan loop doesn't ask it.**

### Scale of the problem

- 42 of 136 way files carry a `pattern:`. Many alternations are bare common
  words (`commit`, `workflow`, `docs`, `remember`, `release`, `trade.?off` —
  the last in two different ways) or unanchored substrings (`graph` matches
  "photograph"). These are vocabulary words doing pattern work.
- Telemetry (`$XDG_STATE/agent-ways/events.jsonl`) records one keyword fire
  whose `matched_span` is a 120-character stretch of prompt — a greedy `.*`
  alternation matching essentially arbitrary text.
- 5,011 near-miss events show the semantic lane frequently lands just below
  threshold — the opposite imbalance: keyword over-fires while semantic
  under-fires.

## Decision

Give the already-computed embedding score veto power over lexical coincidence,
and rebuild the response-topics channel so Claude's reasoning feeds the
semantic lane instead of polluting the regex lane. Five parts:

### 1. Semantic plausibility gate on keyword fires

A `pattern:` hit on the prompt/task surface fires only if the way's embedding
score also clears a **gate floor**:

```
gate_floor = keyword_gate_fraction × effective_threshold(way)
```

with `keyword_gate_fraction` a global config value (default **0.5**), applied
per model lane the same way fire thresholds are (EN and multi each gate on
their own floor; clearing either lane passes). Parent-boost applies to the
effective threshold before the fraction, so a keyword hit under a fired parent
is gated more leniently — consistent with ADR-125's disclosure semantics.

With the defaults this yields floors of 0.14–0.20. Against the observed data:
the three false fires above (0.09, 0.15, 0.22 vs floors 0.20, 0.175, 0.20) are
suppressed or borderline; every observed true fire clears with margin.

The gate is a *veto on noise*, not a second retrieval tier: the pattern
remains the explicit trigger, and the score consulted is the one
`batch_embed_score` already produced for the fire path. Zero additional model
invocations; zero added latency.

**Escape hatch:** a way may declare `pattern_strict: true` in frontmatter to
opt out of gating — for patterns that genuinely mean "fire on this exact
token, always" (e.g. slash-command references like `/wrap`). The lint warns
when `pattern_strict` is combined with common-word alternations.

**Telemetry:** a gated-off hit logs a `way_keyword_gated` event carrying
`matched_span`, both scores, both floors — the same shape as `way_nearmiss`
(ADR-134). The tuning passes consume this stream to calibrate
`keyword_gate_fraction` empirically before any tightening. A gated fire is
also visible in `ways introspect` as a suppressed-candidate row, so a "why
didn't X fire" question is answerable from the session record.

### 2. Mask non-linguistic spans before regex matching

URLs (`https?://\S+`) and fenced code blocks are masked out of the query
**for the keyword channel only** before pattern matching. A pasted link
containing "github" is not GitHub-workflow intent. The embed query is
untouched — the ADR-130 salience reducer already handles long pasted content
for that lane.

### 3. Rebuild the response-topics channel

`check-response.sh`'s 24-word grep whitelist is removed. The Stop hook instead
passes the last assistant message through the ADR-130 sentence-salience
reducer (exposed via the `ways` binary so the hook stays a thin dispatcher)
and stores the reduced text. On the next prompt scan, that stored text feeds
**only the embed query, never the regex query**. The keyword channel matches
what the user actually typed; the semantic channel sees user intent *plus*
what Claude was just reasoning about.

This is the fix for under-firing on Claude's own reasoning: full-sentence
salient content replaces a fixed vocabulary, and it reaches the lane designed
to interpret it.

### 4. Semantic lane for ways at PreToolUse

`command()` already computes embedding scores for the reduced
`command + description` query (used by checks). Ways gain the same lane: a way
whose score clears its effective threshold fires on the bash surface, exactly
as it would on the prompt surface. The tool `description` field is Claude's
own natural-language statement of intent, which is the right embed input at
act-time. No new embedding work — the scores are already computed per
PreToolUse event. Existing re-disclosure suppression (decay curves) bounds
repeat injections; the near-miss/fire telemetry monitors this surface for
noise before any threshold adjustment.

### 5. Pattern hygiene: demote vocabulary words out of patterns

Doctrine, enforced by lint and applied by a rework pass over the 42
pattern-bearing ways:

- **`pattern:` is for exact, high-precision triggers** — command names, term
  -of-art tokens (`adr`, `diataxis`, `mermaid`), phrases with anchoring
  structure. Word-boundary anchoring required for short alternations.
- **Suggestive common words belong in `vocabulary:`** — they shape the
  embedding coordinate, which is the lane built to weigh them contextually.
  `remember`, `commit`, `workflow`, `docs`-alone move out of patterns.
- New lint rules: flag unanchored alternations under a length floor, bare
  dictionary-common words, and `.*` in prompt patterns.
- `ways tune --precision` gains per-alternation attribution: `matched_span`
  (ADR-153) joined with the off-class session heuristic (ADR-134) identifies
  which alternation produces off-domain fires, producing a ranked demotion
  worklist instead of hand-auditing 42 files.

### Sequencing

Parts 1–2 land first (one file, `scan/mod.rs`, plus config/frontmatter
plumbing) — they make the system safe against the *existing* noisy patterns.
Part 3 and 4 follow. Part 5 (frontmatter rework + corpus rebuild + lint) runs
last, validated by embed-scoring regression prompts, so pattern demotion
happens with the gate already protecting the transition.

## Consequences

### Positive

- False keyword fires on incidental words, pasted URLs, and echoed response
  topics are suppressed using signal that is already computed — no latency or
  model cost. Context stops being spent on off-topic way bodies, and ways stop
  training the agent to ignore them.
- Ways gain a semantic channel at PreToolUse — guidance can reach the moment
  before action, which keyword-only matching mostly missed.
- Claude's reasoning enters matching as reduced salient sentences in the
  semantic lane, replacing a 24-word whitelist that leaked trigger tokens into
  the regex lane.
- Every suppression is observable (`way_keyword_gated`, introspect rows) and
  the gate fraction is tunable from telemetry before any further tightening —
  the ADR-134 pattern applied to a new stream.

### Negative

- A way with an exact pattern but weak `description`/`vocabulary` text can be
  wrongly gated: its corpus coordinate under-represents what the pattern
  targets. Mitigations: `pattern_strict: true`, and the part-5 rework
  explicitly checks that pattern terms appear in the way's embeddable text.
- The keyword channel is no longer fully deterministic from the way file
  alone; "why didn't my pattern fire" now has a second cause. Introspect
  surfacing the gated row is the answer path.
- Behavior change for existing installs: some previously-firing ways go
  quiet. The gate defaults are deliberately loose (0.5 × threshold) and
  telemetry-adjustable.

### Neutral

- `meta/knowledge`'s ` ways ` alternation lands near the default floor
  (0.22 vs 0.20) — the borderline case that telemetry, not this ADR, should
  settle.
- Threshold rebalance (lowering semantic defaults where near-misses cluster
  on-domain) becomes attractive once keyword noise is gated, but is out of
  scope here — it stays with the ADR-134 tuning passes.
- The state channel (session-start fires such as `freshness`) has its own
  value question, untouched by this ADR — different trigger class, different
  economics.

## Alternatives Considered

- **A language-model classifier over candidate fires.** Rejected on cost and
  latency: every prompt and tool call would pay an LM round-trip, and the
  evidence shows MiniLM cosine already carries the discriminating signal —
  every observed false fire scored below 0.22, every observed true fire above
  0.28.
- **Weighted lexical scoring (BM25-like multi-term evidence).** Rejected as
  re-litigating ADR-125, which removed BM25 and named the embedding the single
  retrieval mechanism. The gate keeps that shape: lexical stays binary and
  explicit; the embedding stays the only scorer.
- **Authoring-only fix (rework the 42 patterns, change no code).** Necessary
  but not sufficient: authoring regresses (this drift happened under the
  current lint), project-local ways repeat the same mistakes, and no pattern
  hygiene fixes the Stop-hook whitelist or the missing PreToolUse lane.
- **Raising semantic thresholds to compensate.** Backwards: semantic is the
  under-firing channel (5,011 near-misses). The noise source is lexical.
- **Removing the keyword channel entirely.** Rejected: ADR-125's case for
  explicit deterministic triggers stands — slash commands, terms of art, and
  `commands:` regexes at PreToolUse are legitimately exact. The problem is
  authority without corroboration, not existence.
