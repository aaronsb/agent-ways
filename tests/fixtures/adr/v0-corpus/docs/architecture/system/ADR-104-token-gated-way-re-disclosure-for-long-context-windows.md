---
status: Superseded
date: 2026-03-13
revised: 2026-04-14
deciders:
  - aaronsb
  - claude
related:
  - ADR-103
  - ADR-004
  - ADR-123
superseded_by: ADR-123
---

# ADR-104: Token-Gated Way Re-Disclosure for Long Context Windows

## Status: Superseded by ADR-123

**Superseded 2026-04-14.** The core insight of this ADR — that ways must re-fire on a token-distance axis rather than once-per-session, because retrieval degrades as context accumulates past a way's injection point — is still correct and load-bearing. The specific implementation described below (a global `REDISCLOSE_PCT: u64 = 25` constant, a per-model context-window lookup, a flat step-function that admits re-firing once distance crosses 25% of the window) has been replaced by the per-way curve engine introduced in [ADR-123](ADR-123-firing-dynamics-progression-axis-unification.md).

The current implementation: each way declares an explicit `curve:` block in its frontmatter, and the engine queries `current_salience(current_tick) < REFIRE_FLOOR` to decide when to re-fire. `REFIRE_FLOOR` defaults to `0.5`, so `Curve::Exponential { half_life: H }` re-fires at delta `H`, `Curve::Flat { suppression: N }` re-fires at delta `N`, and `Curve::ProgressiveStaircase` re-fires on each declared step. The 25% global default is gone; ways pick their own re-fire cadence.

The rest of this ADR is preserved as historical context — the empirical motivation (retrieval degradation benchmarks), the argument against epoch-based gating, and the alternatives-considered table are all still the reasoning that led to ADR-123's decision. The Decision section below describes *what was decided in 2026-03-13*; the ADR-123 successor describes *what actually runs now*.

## Context

Ways initially fired once per session, gated by a marker file (`/tmp/.claude-way-{name}-{session}`). This rule was designed for 200K context windows where the entire conversation fit within a single effective attention span.

With Opus 4.6's 1M context window, this assumption breaks. Empirical benchmarks show measurable degradation over long contexts:

- **Retrieval** (MRCR v2): Opus drops from 91.9% at 256K to 78.3% at 1M (~15% degradation)
- **Reasoning** (GraphWalks BFS): Opus drops from 72.8% at 256K to 68.4% at 1M (~6% degradation)
- **Sonnet 4.6** degrades much faster: retrieval 90.6% → 65.1%, reasoning 61.5% → 41.2%

(See `docs/reference/model-context-decay/` for benchmark charts and data tables.)

A way disclosed at token 50K is not gone at token 500K — but it's faded. The model can still retrieve the general concept but loses specificity. For guidance that depends on precise rules (security checks, commit conventions, architectural patterns), this degradation produces subtle failures: the model follows the spirit but misses the letter.

The epoch counter (ADR-103) tracks **event distance** — how many tool actions have occurred since a way fired. This is the right metric for check decay (is the model still thinking about this domain?). But it's the wrong metric for re-disclosure (has the way faded from retrievable memory?). A session can have 200 epoch events in 50K tokens, or 10 epoch events in 500K tokens. Token distance is the signal that correlates with measured retrieval degradation.

## Decision (as of 2026-03-13)

Replace the hard "once per session" marker with a **token-distance-gated re-eligibility window**. A way becomes eligible for re-disclosure when the token distance since its last disclosure exceeds a model-specific threshold.

### Token distance tracking

When a way fires, stamp the current token position alongside the existing epoch stamp. Token position is read from the transcript using the same method as `context-usage.sh` — sum of `cache_read_input_tokens + cache_creation_input_tokens + input_tokens` from the most recent API usage record.

### Re-disclosure thresholds

**Percentage-based, not fixed token counts.** Re-disclosure fires when a way has drifted 25% of the context window since its last disclosure. This scales automatically with the model's context size:

| Model | Context window | 25% interval | Max re-disclosures |
|-------|---------------|-------------|-------------------|
| Opus 4.6 | 1M | 250K tokens | ~3-4 per session |
| Sonnet 4.6 | 200K | 50K tokens | ~3 per session |
| Haiku 4.5 | 200K | 50K tokens | ~3 per session |

The 25% figure corresponds to the empirical degradation curves: retrieval accuracy drops ~10-15% per quarter-window.

Using percentages meant the system automatically adapted when Anthropic shipped new context tiers — no hardcoded token counts to update.

### Re-disclosure behavior

When a way re-discloses:

1. The way content is injected again (same as first disclosure)
2. The token stamp is updated to the current position
3. The epoch stamp is updated (checks reset their distance)
4. A `way_redisclosed` event is logged with the token distance that triggered it
5. The fire count is incremented (for stats, not for gating)

### What re-disclosure is NOT

- **Not a timer.** It doesn't fire every N tokens regardless. The way must still be triggered by a matching prompt or tool action. Token distance only makes it *eligible* — it still needs a trigger to fire.
- **Not a check.** Checks (ADR-103) are pre-action verification sensors with decay curves. Re-disclosure is a periodic refresh of the full way guidance.
- **Not visible to the user.** The user sees the same way content; they don't know it's a re-disclosure vs first disclosure.

## What ADR-123 changed (2026-04-14)

- **`REDISCLOSE_PCT` constant is gone.** `tools/ways-cli/src/session.rs` no longer hard-codes a 25% threshold. Each way declares its own curve and the engine computes per-way re-fire distances from `Curve::refire_delta(REFIRE_FLOOR)`.
- **`token_distance_exceeded()` is gone.** Replaced by `session::way_fire_outcome(way_id, session_id, curve) -> FireOutcome` which returns `FirstFire | ReFire | Suppressed`. Same semantic role (gatekeeper for firing), different mechanics.
- **Model-specific window lookup is gone from the firing path.** The engine doesn't care what the context window is; it only knows ticks (token positions) supplied by the caller. Context-window detection still exists in `session.rs` for the visualization path (`ways list`, `ways rethink`), as a fallback when a way's frontmatter is missing or unparsable.
- **Marker file shape changed.** The old `.value` stamp files in `way-tokens/` are still written for legacy callers (tree-metrics, scan-time "has this way been seen") but the canonical firing state now lives at `{session_dir}/way-engagement/{way_id}.json` as a serialized `EngagementState`.
- **25% was the wrong unit of analysis.** It treated all ways the same — a one-size-fits-all cadence. ADR-123 lets each way express its own tempo: a quality way fires on a short half-life because file size grows quickly between fires; an architecture way fires on a long half-life because design decisions persist. Per-way curves capture this directly.

The empirical motivation (retrieval degradation, the MRCR v2 benchmarks, the argument against epoch gating) is **unchanged and still correct**. ADR-104's contribution is the insight that token distance is the right axis; ADR-123's contribution is making the curve shape on that axis a per-way decision instead of a global constant.

## Consequences

### Positive (preserved)

- Compensates for empirically measured retrieval degradation over long contexts
- Maintains the trigger requirement — ways only re-disclose when the domain is relevant
- Resets check distance — prevents stale checks from nagging when the way is freshly re-anchored
- Low token cost per re-disclosure
- Invisible to the model — no behavioral change needed from the model's perspective

### Positive (added by ADR-123)

- Each way declares its own re-fire cadence — no single-heuristic calibration
- The same engine drives attend's inward-gate refractory, eliminating two divergent implementations
- The curve shape is a first-class parameter, enabling progressive-disclosure staircases and other non-exponential shapes

### Negative (resolved by ADR-123)

- ~~Adds token position reading to the hot path (one jq call per way evaluation)~~ — still present, but the cost has been minimal in practice
- ~~Model detection adds complexity to show-way.sh~~ — the shell dispatchers are gone; all firing logic is in the Rust `ways` binary
- ~~Thresholds are empirically derived but not session-specific~~ — resolved by per-way curves

## Alternatives Considered (2026-03-13)

- **Fixed epoch-based re-disclosure (every N events)** — Rejected because epoch count doesn't correlate with retrieval degradation. 100 quick edits in the same file consume fewer tokens than 10 complex prompts with tool chains. Token distance is the right signal.
- **Percentage-of-window triggers (at 25%, 50%, 75% absolute positions)** — Simpler but less nuanced. Doesn't account for when the way was first disclosed.
- **Always re-disclose (remove the once-per-session gate entirely)** — Wasteful; re-disclosing the same way 50 times in 10 minutes adds noise.
- **Decay the existing check system to handle re-anchoring** — Checks inject a short re-anchor (1-2 lines); re-disclosure injects full way content (~200-500 tokens). Using checks for re-disclosure would require making them much longer, defeating their "light sensor" design.
- **Let the user decide (manual re-disclosure command)** — Users shouldn't have to manage context decay. If the user has to remember "my security way has probably faded," the system has failed.

## References

- **[ADR-123](ADR-123-firing-dynamics-progression-axis-unification.md)** — the progression-axis unification that made the per-way curve shape first-class.
- **ADR-103** — check scoring via epoch distance; unchanged.
- **ADR-004** — the original once-per-session marker design that ADR-104 replaced.
- `docs/reference/model-context-decay/` — empirical retention benchmarks.
- `docs/hooks-and-ways/context-decay.md` — the presentation-economics model that explains why token-distance gating works.
