# Authoring docs that describe the matching engine

A style reference for any doc, way body, or skill that explains **how ways match,
score, or get tuned**. The matching model changed with ADR-156 (calibrated
relevance scoring); a lot of prose across the tree still teaches the retired
raw-cosine threshold model. This page is the single source of truth for what to
say — consult it whenever you write or edit a surface that asserts how matching
works.

## The current model — the paragraph every engine doc must agree with

> A way has two lanes. The **keyword lane** is the regex `pattern:` field. The
> **semantic lane** embeds the prompt and takes cosine `s` against the way's alias
> (`description` + `vocabulary`). ADR-156 maps that cosine to a **relevance
> probability** with a per-model logistic `g(s) = σ(a·s + b)`, fit at
> corpus-generation from a committed probe corpus and stored in
> `embed-manifest.json` (EN AUC ≈ 0.955). A way **fires** when
> `g(s) ≥ τ_s  ∨  (keyword_match ∧ g(s) ≥ τ_k)`, with global
> `τ_s = 0.5` (`semantic_fire_probability`) and `τ_k = 0.15`
> (`keyword_floor_probability`), which are **independent**. The keyword lane is
> **floor-gated**: a pattern hit only fires when the semantic probability already
> clears `τ_k`, so a keyword can't drag in an unrelated prompt *when calibration is
> loaded*. With no calibrated signal the keyword lane fails open (fires unconditionally),
> which is also what `pattern_strict: true` forces by design.
> `pattern_strict: true` bypasses the gate (unconditional keyword fire).

If a doc says anything that contradicts that paragraph, the doc is wrong.

## Retired vocabulary — delete or replace on sight

These are gone from the engine. They appear in older prose and read as live
controls; they are not.

| Retired (delete / replace) | Status | Current statement |
|---|---|---|
| `embed_threshold:` frontmatter field | REMOVED | no per-way threshold exists; firing is global |
| `config.default_embed_threshold` (0.35 / 0.40) | REMOVED | cosine → probability `g(s)`, then global `τ_s` |
| `keyword_gate_fraction` / γ / "keyword fires at γ·T" | REMOVED | keyword floor `τ_k`, independent of `τ_s` |
| "raise / lower the threshold on a way" (as a remedy) | REMOVED | edit vocabulary/pattern, then measure |
| per-way threshold tuning (any framing) | REMOVED | global `τ_s` / `τ_k` only |
| raw-cosine thresholding `s ≥ T` | REMOVED | `g(s)=σ(a·s+b)` → probability, threshold in prob space |
| parent-boost applied to a raw per-way cosine threshold | CHANGED | the boost now operates in probability space — see below |

**Parent-boost is NOT retired.** Both `parent_threshold_multiplier` (0.8) and
`parent_boost_floor` (0.30) are live config keys (`config.rs`). When an ancestor way has
fired in the session, an in-domain child's semantic bar is lowered from `τ_s` to
`(τ_s × parent_threshold_multiplier).max(parent_boost_floor)` — by default
`max(0.5 × 0.8, 0.30) = 0.40` (`scan/mod.rs` `effective_thresholds`). The multiplier boosts
the child (lowers its bar); the floor stops cascading boosts from reaching the noise band.
ADR-156's change was that this acts on the global probability `τ_s`, not a raw per-way cosine
threshold — the multiplier itself was not removed. Do not write that `parent_boost_floor`
"replaced" the multiplier; they work together.

## Concepts docs must now teach

The change is not only deletion — these are load-bearing and currently absent from
most prose:

- **`g(s)` calibration** — the per-model logistic, `embed-manifest.json`, and the
  AUC ship-gate that rejects a bad fit at corpus-generation.
- **`τ_s` / `τ_k` independence** — a leaky keyword is tightened by raising `τ_k`
  without touching the semantic bar `τ_s`.
- **`pattern_strict`** — unconditional keyword fire; bypasses the gate.
- **`pattern_keep`** — a frontmatter list exempting a *measured* common-word keep
  from the pattern-hygiene lint (ADR-155 §5): the keyword is load-bearing and its
  off-sense noise is floor-gated.
- **The remedy loop** — when a way mis-fires, the fix is **measure → edit
  vocabulary/pattern → re-measure** through `tools/scripts/probe-measure.py`. Never
  "move a threshold." There is no per-way threshold to move.

## One Diátaxis mode per page

Every page is exactly one of tutorial / how-to / reference / explanation (see the
`docs` skill for the full model). A page serving two modes serves neither — split
it. In particular:

- **How-to** (author/tune a way): the task recipe. Assumes competence, omits the
  teaching.
- **Reference** (frontmatter fields, CLI, config keys): complete and consultable,
  not read start-to-finish.
- **Explanation** (why the calibrated model, the IR lineage): understanding, not
  steps.

Keep frontmatter field catalogs, config-key lists, and CLI tables in **reference**
pages; keep "how do I make this way fire correctly" in **how-to**; keep "why is it
shaped this way" in **explanation**.

## Timestamped artifacts — do not rewrite history

- **ADRs are immutable.** Never edit an ADR to reflect a later decision. An ADR that
  describes the pre-156 world is correct history, not staleness. Supersede with a new
  ADR; don't rewrite the old one.
- **Design notes and implementation plans are dated thinking.** When one led to a
  shipped decision, add a dated **provenance banner** and preserve the body — the
  exploratory reasoning *is* the artifact's value. Do not rewrite it to present tense.
  Example banner:
  > *Written during the ADR-156 exploration (pre-ship). The framing below treats the
  > γ·T gate on raw cosine as the live control surface; ADR-156 superseded it with the
  > calibrated `g(s)` and independent `τ_s`/`τ_k`. Preserved for the reasoning that led
  > there — see ADR-156 for the shipped model.*

## Do not disturb — accurate as written

These describe live, unchanged mechanisms; leave them alone when purging threshold
prose:

- **`ways tune`** — the *locale* alias audit (fidelity / discrimination vs the English
  root anchor, ADR-139/125). It never wrote relevance thresholds; it fixes stub
  quality by re-authoring.
- **Salience / signal decay** — turn-based exponential decay (ADR-121), a model
  separate from relevance scoring.
- **Progressive disclosure and token-gated re-fire** (ADR-104/105/126), the
  three-root runtime (ADR-143), sentence-salience input reduction (ADR-130), the
  authored disclosure graph and removal of BM25 (ADR-125), and the two embedding
  models (EN + multilingual for localized mode).

## Voice

Sober and literal. Injected way, ADR, and frontmatter text reads as doctrine, not
chat — state the model, cite the ADR, show the measurement. No hype.

## See also

- ADR-156 — calibrated relevance scoring (the shipped model).
- ADR-155 — semantic gating of the keyword channel; the pattern-hygiene lint and
  `pattern_keep`.
- `meta/knowledge/authoring` — the way authoring how-to.
- `meta/knowledge/optimization` — the vocabulary/matching tuning how-to.
- `tools/scripts/probe-measure.py` — the measurement path for the remedy loop.
