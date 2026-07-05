# Signal Analysis

How to characterize the ways matcher's score distributions empirically — to see
whether signal (expected-way hits) separates from noise (everything else) per
embedding model. The two models (EN-only 384-dim `minilm-l6-v2` and multilingual
768-dim) produce cosine scores in different distributions; the ADR-156 calibration
`g(s) = σ(a·s + b)` is fit per model from those distributions, so it helps to look
at them directly rather than guess.

This is the distribution view. It is not the fire decision — firing happens in
probability space (`g(s) ≥ τ_s ∨ (keyword ∧ g(s) ≥ τ_k)`, global `τ_s = 0.5`,
`τ_k = 0.15`). For the fire rule and calibration in full, see
`../hooks-and-ways/engine-reference.md` (single source of truth). For the per-way
remedy loop, use `tools/scripts/probe-measure.py` (measure → edit
vocabulary/pattern → re-measure). This report is for seeing the raw-cosine bands
that the calibration is fit against — not for moving a threshold (there is no
per-way threshold to move).

## Generating a report

```bash
scripts/signal-report.py
```

Writes three artifacts to the output directory (`--out`, default
`~/.claude/docs/signal-analysis`):

- `scores.csv` — raw `(prompt, lang, expected_way, way_id, model, score)` rows,
  where `score` is the raw cosine similarity
- `score-distributions.png` — histograms of signal vs noise per model, with a
  raw-cosine reference line drawn
- `per-prompt-gap.png` — per-prompt bar chart of the expected way's score vs the
  top competing way, for both models

Swap in your own battery with `--prompts file.jsonl` where each line is
`{"lang": "...", "expected_way": "...", "prompt": "..."}`.

The report scores raw cosine at `--threshold 0.0` (it wants the whole
distribution, not the fired set). The reference line it draws is a raw-cosine
marker (`--en-threshold`, default `0.40`; `--multi-threshold`, default `0.55`),
**not** the live fire boundary. Under ADR-156 the boundary is `g(s) ≥ τ_s` in
probability space; because `g(s)` is monotonic in cosine, that boundary maps to
one cosine value per model, which you can read off the model's fitted `a`/`b` in
`embed-manifest.json`. The default `0.40`/`0.55` markers predate calibration —
treat them as an eyeball guide to where the trough is, not as the mechanism.

## How to read the plots

**score-distributions.png** — two columns, EN and multi. Signal (expected-way
hits) in green, noise (everything else) in red. The black dashed line is the
raw-cosine reference marker. A well-separated model puts a clear trough between the
two distributions; that trough is what the `g(s)` fit turns into a clean
probability curve.

**per-prompt-gap.png** — one row per prompt. Blue bars are EN scores, orange are
multi. For each prompt, you see the expected way's score and the top competing
way. A healthy prompt has the expected way well above any competitor, with visible
daylight between them — that gap, not any single cutoff, is what makes the way fire
for the right prompt and stay quiet for the wrong one.

## What to look for

- **Signal peak buried in the noise band**: the expected way scores no higher than
  unrelated ways. This is a stub problem, not a threshold problem — sharpen the
  way's `description` / `vocabulary` so it embeds closer to its prompts, then
  re-measure with `tools/scripts/probe-measure.py`. There is no per-way threshold
  to lower.
- **Signal and noise overlap significantly**: no cutoff can cleanly separate them,
  and a `g(s)` fit over that overlap earns a low AUC (the fit is rejected below the
  `AUC_FLOOR` of 0.70, leaving the lane uncalibrated). The fix is stub-level — use
  `ways tune` to find the confusers and re-author them.
- **Noise tail riding high on one way**: a specific confuser stub is scoring into
  signal territory. Tighten that stub; don't reach for a global knob to paper over
  one bad pair.
- **Multi-column dominance for English queries**: surprising. English queries
  should usually win in the EN model. If multi wins, either the English stub is
  weak, or the query carries non-English content.

The global `τ_s` / `τ_k` are the only thresholds, and they are independent globals,
not per-way dials (`config.rs:130–131`). Distribution problems on a single way are
fixed at the stub, measured through `probe-measure.py` — never by moving a
threshold.

## Baseline snapshot (2026-04-17, pre-156 raw-cosine reference)

> Measured before ADR-156 shipped, when the matcher thresholded raw cosine per
> model. The distribution figures below (signal min/mean, noise p95/p99) are still
> a valid raw-cosine characterization and read the same way. The **threshold**
> column, however, is the pre-156 raw-cosine cutoff — under ADR-156 firing is
> `g(s) ≥ τ_s` in probability space, so read that column as the reference marker
> the script draws, not as the live boundary. Re-run `signal-report.py` for a
> current snapshot.

From the default 16-prompt battery:

| model | signal min | signal mean | noise p95 | noise p99 | raw-cosine marker |
|-------|-----------:|------------:|----------:|----------:|------------------:|
| EN    | 0.01*      | 0.36        | 0.24      | 0.31      | 0.40              |
| multi | 0.54       | 0.67        | 0.45      | 0.59      | 0.55              |

\* signal_min 0.01 for EN comes from non-English prompts expecting a match; the EN
model doesn't understand them, which is correct — those prompts are picked up by
the multi path instead.

The markers sat in the gap between signal_min and noise_p95 for both models. Multi
showed ~1% p99 leakage (top 1% of noise exceeded 0.55); most of those are
genuinely-ambiguous stubs that `ways tune` flags as discrimination problems — a
stub fix, not a threshold fix.

## See also

- `../hooks-and-ways/engine-reference.md` — the fire rule, `g(s)` calibration, and
  parent-boost, all source-cited. The authority for every number here.
- `../hooks-and-ways/authoring-docs-style.md` — how to write about the engine.
- `tools/scripts/probe-measure.py` — the per-way remedy-loop measurement path.
- ADR-156 — calibrated relevance scoring. ADR-155 — keyword-channel gating.
