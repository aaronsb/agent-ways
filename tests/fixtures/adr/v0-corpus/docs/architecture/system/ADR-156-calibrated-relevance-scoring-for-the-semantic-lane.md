---
status: Accepted
date: 2026-07-04
deciders:
  - aaronsb
  - claude
related:
  - ADR-155
  - ADR-125
  - ADR-134
---

# ADR-156: Calibrated relevance scoring for the semantic lane

## Context

The matching engine thresholds **raw cosine similarity**. The semantic lane
fires when `s = cos(query, alias) ≥ T_w`, and the ADR-155 keyword gate admits a
pattern hit when `s ≥ γ·T_w` (`γ = keyword_gate_fraction`, default 0.4). Both
boundaries live on the raw cosine scale, and each way's `embed_threshold` is set
by hand.

Raw cosine is not comparable across ways. A cosine of 0.30 against one alias can
be strongly relevant while against another it is noise, because aliases occupy
different regions of the embedding space (different vocabulary breadth, different
neighbourhoods). The per-way `embed_threshold` exists to absorb that
incomparability — it is, in effect, a **single hand-placed calibration point per
way**. The design note *The Lexical Gate as a Conditional Threshold* reads the
current rule as the decision boundary of Bayesian log-odds evidence fusion for a
binary lexical feature, and identifies the one component the implementation
lacks: `g(s)`, a calibration from cosine to a relevance probability.

We fit `g(s) = σ(a·s + b)` over intent/noise probe sets across six ways spanning
the regimes (96 probes). The findings (provisional, small-sample, but
consistent):

- A **single global calibration** separates intent from noise at pooled
  **AUC 0.956**, with the `P = 0.5` boundary at `cos ≈ 0.29`.
- On the calibrated scale, today's constants are revealed as mis-set: the
  default `embed_threshold = 0.40` corresponds to `P ≈ 0.95` (the semantic lane
  fires only at 95% confidence), while the gate floor `γ·T = 0.16` corresponds
  to `P ≈ 0.03` (the keyword lane fires at 3% confidence). The keyword lane is
  load-bearing precisely because the semantic threshold is set far too high, and
  it leaks precisely because the floor is set far too low.
- Per-way `embed_threshold` values largely **collapse** once calibrated: five of
  six ways sit at the untuned default; the separable ones share essentially one
  boundary. The per-way knob is mostly compensation for the uncalibrated scale,
  not per-way signal.
- Ways whose keyword-conditioned intent and noise distributions **overlap**
  (e.g. `meta/memory`, AUC 0.80) are exposed by a low per-way AUC — a principled,
  measurable signal that no threshold can save that keyword.

Three standing problems follow from thresholding raw cosine: the thresholds are
not portable, they **drift silently when the corpus is regenerated** (a
hand-tuned constant is calibrated to one embedding snapshot), and because scores
are never turned into a common currency, the two model lanes (EN, multi) cannot
be combined — they are OR-ed with separately hand-set thresholds.

## Decision

Introduce `g(s)`: a per-model calibration from cosine to relevance probability,
and express every fire threshold in probability space.

1. **Calibration.** For each embedding model `m`, fit
   `g_m(s) = σ(a_m·s + b_m)`. Calibration is a **fitted, versioned artifact**,
   produced offline from a committed, curated probe corpus and validated on
   held-out probes (fit is rejected below an AUC floor). It is stamped with the
   model and corpus version it was fit against, so a corpus regeneration that
   moves embeddings triggers a refit rather than silent drift. The numbers above
   are illustrative of the shape, not the production fit.

2. **Fire rule in probability space.** For a way `w` and prompt `q` with keyword
   indicator `k`:

       fire ⇔ g_m(s) ≥ τ_s(w)   ∨   ( k ∧ g_m(s) ≥ τ_k )

   evaluated per model and OR-ed across models (probabilities are now
   comparable). `τ_s` is the **semantic threshold** (global default, e.g.
   `P = 0.5`) and `τ_k` is the **keyword floor** (global default, e.g.
   `P ≈ 0.15`). Both are absolute probabilities.

3. **The keyword floor is decoupled from the semantic threshold.** `τ_k` and
   `τ_s` are independent, so a leaky keyword can be tightened without raising the
   semantic bar. This subsumes `keyword_gate_fraction`: the fixed ratio `γ` is
   retired in favour of two independent probabilities — the coupling named in
   the design note is resolved as a side effect of moving to probability space.

4. **Global thresholds only.** One `(a_m, b_m)` per model and one global
   `(τ_s, τ_k)`. No per-way threshold override ships in v1: calibration removes
   the incomparability that per-way `embed_threshold` existed to patch (AUC 0.956
   under a single boundary), and a way that still fails to separate is a
   way-content problem for the pattern-hygiene sweep (fix its alias or keyword),
   not a knob. A per-way probability override can be reintroduced later if
   telemetry identifies a way that genuinely needs one.

5. **Clean cutover — no compatibility mode.** Raw-cosine thresholding,
   `keyword_gate_fraction`, and the raw `embed_threshold` field are removed, not
   shimmed. Carrying a second scoring path would mean maintaining the exact ruler
   this ADR replaces. Existing per-way `embed_threshold` values are stripped from
   way frontmatter in the same change — the ways fall to the calibrated global
   boundary, and the pattern-hygiene sweep is already touching every
   pattern-bearing way. `pattern_strict` still bypasses the gate. Calibration is
   generated together with the corpus (`ways corpus`), so it is present wherever
   embeddings are; the only degenerate path retained is the existing genuine
   no-embedding case (non-embeddable way, or engine not run), which continues to
   fail open on the author's keyword. The change ships as a version bump the
   operator deploys deliberately.

Telemetry-based refitting from the ADR-134 streams (`way_fired`,
`way_nearmiss`, `way_keyword_gated` are the observed `S⁺`/`S⁻` samples) is the
intended successor to the offline fit, but is **not** in this decision: the
feature must be live to generate calibrated telemetry.

## Consequences

### Positive

- One interpretable, comparable decision boundary. Per-way threshold hand-tuning
  becomes the exception, not the norm.
- The keyword floor and semantic threshold are independently settable; the
  over-loose gate and over-strict semantic lane can each be corrected, and the
  fixed `γ` coupling is retired.
- Thresholds stop drifting on corpus regeneration: calibration is refit and
  version-stamped, not silently invalidated.
- Model lanes become combinable (comparable probabilities), enabling future
  log-odds composition of EN + multi and of per-alternation lexical evidence.
- The pattern-hygiene sweep (ADR-155 §5) gains an **objective** remove/keep
  metric — per-keyword AUC and the calibrated boundary — replacing hand-set
  intent/noise heuristics.

### Negative

- A new fitted artifact to own and version. A bad fit degrades every way at once;
  mitigated by an AUC validation gate that rejects a bad fit at generation time
  and by version stamping. A missing or invalid calibration is a
  corpus-generation error, not a silent fallback to the retired raw-cosine path.
- Behaviour change for existing installs, delivered as a clean cutover: the
  firing set shifts (the intended correction) and existing per-way
  `embed_threshold` values are removed in the same release. Gated behind a
  version bump the operator deploys, watched via the near-miss / gated telemetry.
  There is no rollback short of reverting the release — acceptable because the
  install is versioned and operator-deployed.

### Neutral

- Sequences the remaining ADR-155 §5 work behind this: the corpus rescore and the
  pattern-hygiene sweep run *after* calibration lands, against the better ruler.
- Establishes the probe corpus as a committed regression asset and sets up
  telemetry refitting and per-alternation weighting as named follow-ons.

## Alternatives Considered

- **Keep thresholding raw cosine, hand-tune per way (status quo).** Rejected: not
  portable, drifts on regeneration, and forces the strict-semantic / loose-gate
  split that makes the keyword lane both load-bearing and leaky.
- **Score-level fusion (weighted sum / RRF of lexical and dense scores),
  thresholded once.** The mainstream hybrid-retrieval combiner. Rejected as a
  larger rewrite for no additional benefit here: the existing gated two-threshold
  rule already *is* the decision boundary of log-odds fusion for a binary lexical
  feature (design note), so calibrating the score achieves the same gain with a
  smaller, more legible change.
- **Per-way calibration curves.** Rejected for this decision: needs per-way
  labelled data and reintroduces the per-way tuning burden calibration removes. A
  global fit plus an optional per-way `τ_s` reaches AUC 0.956.
- **Fit from telemetry on day one.** Rejected: the feature must be live to emit
  calibrated telemetry. Offline fit from a curated probe corpus bootstraps it;
  telemetry refitting follows.
- **Backward-compatible rollout (dual scoring paths).** Keep raw-cosine
  thresholding and the legacy `embed_threshold` working alongside the calibrated
  path. Rejected: it would require maintaining the exact ruler this ADR replaces,
  the firing behaviour would depend on which path a way happened to take, and the
  install is versioned and operator-deployed — a clean cutover at a version bump
  is both safe and honest, where a compatibility shim is permanent drift surface.
