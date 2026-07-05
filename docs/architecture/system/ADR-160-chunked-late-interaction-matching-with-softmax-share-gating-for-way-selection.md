---
status: Accepted
date: 2026-07-05
deciders:
  - aaronsb
  - claude
related:
  - ADR-107
  - ADR-108
  - ADR-125
  - ADR-155
  - ADR-156
---

# ADR-160: Chunked late-interaction matching with softmax-share gating for way selection

## Context

Way selection embeds a surface (a prompt, a tool-use command, a task) into one dense vector and matches it against each way's one-line alias (`description` + `vocabulary`) by cosine similarity, thresholded in probability space by the calibrated fire gate (ADR-156, `g(s) ≥ τ_s`). This single-vector approach has two structural weaknesses. Dense sentence embeddings are **anisotropic** — unrelated text still scores 0.2–0.4, a high similarity floor — so an absolute threshold separates signal from noise across a narrow band. And a **sparse or action-shaped surface** (a shell command is an action, not a statement of intent) collides on shared tokens with unrelated aliases, so a way fires on lexical overlap rather than meaning; the fire carries no recoverable reason, because a cosine between two dense vectors has no term-level attribution.

The felt symptom is *poorly-matched* ways surfacing; the objective is **precision** (fewer poorly-matched fires), with lower fire count as the emergent byproduct rather than a directly-tuned target. The mechanism and the measured forces behind this decision were established by prototyping — see the design note *The Tool-Use Channel is a Signal Problem* (`docs/design-notes/tool-use-channel-lookbehind-chunk-matching.md`), which grounds each stage below in established information-retrieval practice (conversational query reformulation, ColBERT-style late interaction, score normalization against anisotropy, two-stage cascade reranking).

## Decision

Adopt a multi-stage evidence pipeline for way selection, replacing single-vector thresholding. The pipeline is channel-agnostic (prompt, tool-use, task) and uses only the existing embedder (ADR-108/125) and calibrated fire gate (ADR-156) — no new model, no reasoning tier, no resident daemon.

1. **Contextualize the surface.** A sparse surface is enriched with adjacent intent context before embedding, rather than embedding the bare surface. Intent, not the literal artifact, is what should be matched.
2. **Chunk and match.** Split the surface into sub-units, embed each, and match each against the corpus — multi-vector late interaction instead of one vector per surface.
3. **Rank by peak.** A way's ranking score is the maximum over its per-chunk similarities. Peak preserves specificity; it deliberately discards how many chunks agreed.
4. **Gate by softmax-share, with a peak co-gate.** Within each chunk, take a softmax over the candidate ways (competition-normalized and zero-sum, which defeats the anisotropic floor: a way must *win* the chunk, not merely clear an absolute score). Sum this mass across chunks. A way is admitted into confirmation on **either** a sufficient summed share **or** a decisive peak cosine — because `share = Σmass / n_chunks` caps a way that owns one of N topics at ≈1/N, so on a topic-diverse surface (the common case for a real prompt) a specific single-chunk match is diluted below any fixed share gate. Admitting on the peak, and letting the strict body-confirm (stage 5) carry precision, recovers that match.
5. **Confirm the winner.** Cross-compare the winning way's body chunks against the *chunk it won* (its peak chunk) and require the best of those to clear a bar. Corroborating the winning evidence — rather than averaging over every surface chunk — still rejects single-token collisions (a collided chunk finds no support in the way's own body) and covers the softmax gate's zero-sum blind spot (it always hands the winner mass, even when nothing is truly relevant), without diluting a way that legitimately matched only part of a multi-topic surface.
6. **Exclude structurally, never with negated text.** Exclusion (scope, domain, project) is expressed as a filter or rule, because dense bi-encoders cannot represent negation — negated text in an alias moves it *toward* the negated topic. The corpus-authoring corollary: a way's embedded prose (`description` + `vocabulary`) states only what the way is *for*, in positive terms, using its own distinctive vocabulary; it never names, contrasts with, or excludes another item — all exclusion lives in the scope gate. Naming or negating another item only pulls the alias *toward* it.

The pipeline is lenient where it ranks (peak + softmax-share admit a way on its strongest evidence) and strict where it confirms (the body must corroborate that winning evidence). An earlier design averaged the confirm over *every* surface chunk; a live trial showed that over-prunes multi-topic surfaces, so confirmation is scoped to the winning chunk. This lenient-rank / strict-confirm split is the load-bearing design choice.

**Required primitive.** The pipeline embeds many chunks per surface, so the embedder must embed a **batch per model load**, and ideally **multiple batches per load** (all chunks across all surfaces a hook needs in one invocation). Per-chunk model reloads are not viable. This is a hard prerequisite, not an optimization.

**The late-interaction pipeline is the semantic matcher, not an opt-in alternative.** It replaces the single-vector calibrated gate on the prompt and task surfaces; the single-vector path (ADR-156) is retained only as the **fail-safe fallback** for surfaces too sparse to chunk (fewer than two chunks) or when the embedder is unavailable — it is neither a user-selectable mode nor the default. There is deliberately no config flag to A/B the two: committing to one matcher is the non-clever choice.

**Status: Accepted — shipped with provisional operating points.** The operating points (softmax temperature, share gate, peak co-gate, confirm gate) are hand-set. The matcher ships as *the* semantic matcher on `main`. Two refinements have landed since first ship, each from evidence the shipped instruments surfaced:

- **Winning-chunk confirmation (stage 5).** A live trial showed a mean-of-max confirm over *every* surface chunk over-prunes multi-topic surfaces; scoping confirmation to the winning chunk resolves it.
- **Peak co-gate + confirm 0.35 (stage 4).** The read-side precision instrument and the `ways match` diagnostic — run once a `--batch`-capable embedder was actually deployed (a deployment defect had silently routed every scan to the single-vector fail-safe, so the points had never been exercised on real late-interaction surfaces) — showed the share gate alone fires *nothing* on topic-diverse prompts: a strong, specific match (e.g. `documentation/adr`, peak 0.54) is diluted below the share gate. Admitting on the peak (`PEAK_GATE = 0.50`) recovers it; that shifted the binding constraint to the confirm gate, which at 0.40 still rejected a clear true positive (body-confirm 0.363), so the confirm gate moved to 0.35 — which fired the true positives without admitting false positives on the sampled surfaces.

The points remain **provisional, not finally calibrated**: the follow-up is fitting `PEAK_GATE` / `CONFIRM_GATE` against a judged eval set (the `introspect fires` instrument plus labeled prompts) rather than a handful of surfaces. A known gap remains: this stage-4 text specifies routing the summed mass through ADR-156's *calibrated* fire gate, but the implementation thresholds a *hand-set* `SHARE_GATE`; calibration must reconcile the two. That is refinement of a shipped matcher, not a gate on adoption; a regression is handled by tuning or, in the limit, superseding this ADR.

## Consequences

### Positive

- **Precision.** Rejects the token-collision and cross-domain false positives that single-cosine admits; fewer poorly-matched fires, with lower total count as an emergent effect rather than a suppressed one.
- **Attribution.** Every fire carries a recoverable reason — the winning chunks and the confirming body spans — closing the "no recoverable term" gap in the fire drill-down.
- **Reuse.** Uses the existing embedder and calibrated fire gate; adds no model, no LLM/reasoning tier, and no resident daemon.

### Negative

- **A calibration surface.** The operating points must be fit against a metric, not hand-set — a real tuning burden and the explicit gate on adoption.
- **More embedding work per surface** (N chunks plus a winner-body cross-similarity), viable only with the batched-embedding primitive; without it the model-load cost multiplies.
- **Degrades on context-free sparse surfaces** (nothing to chunk); requires a fail-safe fallback to the single-vector path rather than a hard dependency.

### Neutral

- Requires the **batched-embedding primitive** (single- and multi-batch per model load) as a prerequisite deliverable.
- Requires **forward telemetry** (`fire_score` on *every* semantic fire, not just first-fires) plus a **read-side replay instrument** — replaying each fire's surface from the transcript at its logged token position, so relevance (and derived signals like self-reference and productivity) can be judged — to measure precision; the prerequisite for calibration. A per-fire query *hash* was considered and dropped: a hash can only be deduplicated, never judged for relevance.
- Channel-agnostic; roll-out is sequenced by measured fire-volume per channel, not by channel identity.

## Alternatives Considered

- **Single-vector cosine threshold (status quo, ADR-156).** The problem this ADR addresses: an anisotropic floor thresholded with no attribution. Retained only as the fail-safe fallback for surfaces too sparse to chunk — not as the default and not as an opt-in alternative to the late-interaction matcher.
- **Generative LLM reranker (local or remote).** Rejected as the primary mechanism. A probe kept the very false positive it was meant to reject when given only the thin alias as evidence; it adds cost and nondeterminism, and the leverage proved to be *evidence quality*, not model capability. Retained only as a possible last resort for residual within-domain ambiguity, behind the deterministic stages.
- **Resident model daemon.** Deferred. Unnecessary for the embedding tier — batched embedding suffices for the load. Relevant only if a larger reasoning model is later introduced as a reranker.
- **Negated alias text ("not for X").** Rejected. Dense bi-encoders move *toward* a negated topic; measured directly. Exclusion must be structural.
- **Sum / noisy-OR aggregation for ranking.** Rejected for ranking. Rewards breadth over specificity and lets a generic near-miss outrank the specific way; peak is used for ranking and mean only for winner confirmation.
