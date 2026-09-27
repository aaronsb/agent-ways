---
status: Accepted
date: 2026-06-09
deciders:
  - aaronsb
  - claude
related:
  - ADR-123
  - ADR-125
  - ADR-130
  - ADR-135
---

# ADR-134: Empirical auto-tuning from fire and near-miss telemetry

## Context

The firing engine is tuned by hand. Every threshold, half-life, and vocabulary was set by authorial judgment, and the system collects no evidence that could revise them. Three gaps compound:

1. **ADR-123 Phase E was never built.** The planned `ways tune` cadence calibration (derive per-way `half_life` from observed fire deltas in `~/.claude/stats/events.jsonl`) remains open; `ways tune` today audits locale alias fidelity only (per ADR-125). *(Correction, 2026-06-12: this claim was already false when drafted — Phase E shipped as the `ways tune-curves` subcommand in PR #49. See the Amendment below.)*
2. **Fires are logged; near-misses are discarded.** The matcher computes a score for every way on every prompt, then throws away everything below threshold. False silences — the way that *should* have fired — are structurally invisible, so the precision-first discipline (0 false positives as hard constraint) has no recall measurement to trade against.
3. **Fire relevance is unmeasured.** A 2026-06-09 session readout showed ~17 of 47 fires landing in a session whose work never touched their domain (`itops/incident`, `delivery/migrations`, `testing/mocking` firing into a docs-only session). Each costs an injection; collectively they erode the trust the emission discipline exists to protect. Nothing currently distinguishes a fire that shaped an action from one that was scrolled past.

Hand-tuning cannot close these gaps because the evidence doesn't exist to hand-tune *from*. A nervous system that cannot adjust its own sensitivities from experience is a reflex arc.

## Decision

Extend the telemetry surface and the `ways tune` subcommand into an empirical tuning loop with three measurements and a gated apply step.

1. **Near-miss logging.** The matcher logs, per prompt, the top-scoring below-threshold candidates (score within a margin of their threshold, e.g. 0.05) to `events.jsonl` as `way_nearmiss` events. The scores are already computed; this is persistence, not new computation. Volume is bounded by the margin.
2. **Cadence calibration (ADR-123 Phase E, as planned).** `ways tune --cadence` groups `way_fired` / `way_redisclosed` events by way, computes token-delta distributions between fires, and suggests `half_life` per the existing Phase E worksheet (rule of thumb: half_life ≈ median delta). *(Already shipped — as the `ways tune-curves` command; no `tune --cadence` mode exists or is needed under that name. See the Amendment.)*
3. **Relevance signal.** `ways tune --precision` correlates each way's fires with the session's subsequent activity class, derived from data already in the event log (trigger channel, tool mix, domains of other fires). A way whose fires repeatedly land in sessions that never touch its domain is flagged with its observed irrelevance rate and suggested remedies: threshold raise, vocabulary narrowing, or trigger-channel change. This is a heuristic flag, not a verdict — the same contract as `ways tune`'s fidelity audit.
4. **Gated apply.** All three report by default; `--apply` rewrites frontmatter (`half_life`, `embed_threshold`) in place. Vocabulary changes are never auto-applied — they re-shape the embedding neighborhood and stay authorial. Applied changes are ordinary git diffs, reviewable and revertible. *(The `half_life` apply already ships in `tune-curves --apply`, which rewrites the `curve:` block; only `embed_threshold` apply remains to build. See the Amendment.)*

The recall counterpart falls out of (1): near-miss data plus the existing fixture workflow lets `ways tune` report likely false silences (ways that consistently score just under threshold on prompts whose sessions then did that way's kind of work), giving the 0-FP discipline its first recall estimate.

## Consequences

### Positive

- Closes the open loop: telemetry that exists only as a record becomes input to calibration. Half-lives can be retuned per model generation (the maintenance posture ADR-301 documented).
- False silences become measurable for the first time; precision-first stops being precision-only.
- Per-session precision audits (`ways list` plus irrelevance rates) turn anecdotes like the 2026-06-09 readout into a tracked metric.

### Negative

- `events.jsonl` grows faster; near-miss margin needs a cap and the log needs rotation.
- The relevance signal is a proxy — activity-class correlation can mislabel legitimately cross-cutting ways (e.g. `meta/tracking`). Flags must stay diagnostic, never auto-applied to vocabulary.
- `--apply` writing frontmatter from observed behavior risks codifying one user's work distribution; suggested values should show the sample size they derive from.

### Neutral

- ADR-123 remains Accepted and unedited; this ADR absorbs and extends its open Phase E. Phase F (A/B validation) is unaffected.
- Fine-tuning the embedding model (deferred in ADR-108) becomes more attractive once near-miss data accumulates as training signal — out of scope here.

## Alternatives Considered

- **Edit ADR-123 to widen Phase E** — rejected: ADR-123 is an Accepted historical record; widening its scope post-acceptance hides when the precision dimension entered the design.
- **Model-graded relevance (LLM judges whether each fire influenced the session)** — rejected for now: highest-fidelity signal but adds inference cost to a system whose value is being cheap and ambient. Revisit if the activity-class proxy proves too coarse. Note that an *external* instance of this signal already arrives for free: the periodic Claude Code usage report is model-graded analysis of where a session actually went wrong, generated outside this loop at no inference cost to it. It does not measure way-fire relevance directly, but it measures the downstream thing ways exist to prevent — and a 2026-06 report (write-time friction: Buggy Code 40, Wrong Approach 37, Excessive Changes 7) is what empirically justified the content-level trigger in ADR-135, this design's first pattern-level consumer.
- **Manual periodic audits of `ways list` output** — rejected as the only mechanism: it found today's signal, but it doesn't scale and never sees near-misses.

## Amendment — 2026-06-12: Phase E reconciliation

Implementation grounding for Decision 1 surfaced that this ADR's own Context was wrong on a load-bearing point. Context gap #1 and Decision 2 both assert that ADR-123 Phase E — cadence-derived `half_life` calibration — was never built. **It was.** It ships as the `ways tune-curves` subcommand (commit `28527e8`, PR #49; with its input field `token_position` added to `way_fired` in commit `8b20782`), wired at `main.rs`, predating this ADR's 2026-06-09 draft. Run today it processes the real event log (hundreds of fires per high-traffic way), groups `way_fired`/`way_redisclosed` by `(way, session)`, computes token-position deltas, and suggests `half_life ≈ median delta` — verbatim Decision 2. There is no separate cadence work to do.

That a *draft about empirical self-correction* shipped a confident-but-wrong claim about its own installed components is the failure mode the 2026-06 usage report named; recording the correction here rather than silently editing it over is the point.

The reconciliation, decision by decision:

- **Decision 1 (near-miss logging)** — genuinely new; implemented (`way_nearmiss`, the matcher's 3-state `match_prompt`, `near_miss_margin` config). Stands.
- **Decision 2 (cadence calibration)** — **already satisfied by `tune-curves`.** No new code. The `--cadence` *spelling* in the Decision is aspirational; the *capability* exists under a sibling command name. Building a `tune --cadence` alias was considered and rejected as cosmetic duplication (it would add CLI surface for naming fidelity alone) — the over-build the firing engine's own discipline (ADR-135) exists to prevent.
- **Decision 3 (relevance / `--precision`)** — genuinely new and **not built**. This is the substantive remaining contribution: correlating each way's fires with session activity class to flag the irrelevance the 2026-06-09 readout measured (17/47 off-domain fires).
- **Decision 4 (gated apply)** — **partly already shipped.** `tune-curves --apply` performs the `half_life` rewrite (on the `curve:` block) with sample-size reporting. Only `embed_threshold` apply — driven by the recall/precision signals — remains to build.

Net: this ADR's open work is Decision 3 (precision) plus the `embed_threshold` slice of Decision 4. Decisions 1 and 2 and the `half_life` half of 4 are done. The ADR is retained whole — including the now-corrected Context — because the empirical-tuning frame and the precision signal it still authorizes remain valid; the value is in narrowing the build to what does not already exist.

## Implementation status — 2026-06-12 (Accepted)

The design is adopted; the empirical-tuning loop it authorizes is built and exercised against the real event log. Status of each piece:

- **Decision 1 — near-miss logging** — shipped (PR #117). `way_nearmiss` events emit for below-threshold candidates within `near_miss_margin` (default 0.05); the matcher's `match_prompt` is a 3-state `Fired | NearMiss | NoMatch`. Verified live.
- **Decision 2 — cadence calibration** — pre-existing as `ways tune-curves` (reconciled above, PR #118). No code.
- **Decision 3 — relevance signal** — shipped (PR #119) as `ways tune-precision`: parent-family activity classes, off-class irrelevance rate, and a breadth-based cross-cutting guard that keeps the named false positive (`meta/tracking`) out of the vocab-narrowing remedy. Verified live (69 ok / 64 cross-cutting / 25 mis-targeted / 44 low-n of 202 ways).
- **Decision 4 — gated apply** — split. `half_life` apply ships in `tune-curves --apply`. `embed_threshold` apply needs a derivation that the data did not support: `way_fired` carried no firing score. The **fire-score telemetry** that makes a principled raise possible shipped (PR #120) — `fire_score` is now logged on first-fires. The **suggestion + apply** itself is **deferred until that telemetry accumulates** (it cannot be validated over an empty population), sequenced exactly as `token_position`→`tune-curves` was. Tracked as the only remaining build.
- **Negative (log growth)** — addressed (PR #121): `events.jsonl` is bounded by tail-compaction in `log_event`.

This ADR is **Accepted** with the `embed_threshold`-apply slice of Decision 4 as tracked, data-gated future work — a sequencing constraint, not an open design question.
