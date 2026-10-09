# Hubness penalty in ranking (ADR-700 §5, ADR-701 §6)

Measured 2026-10-08 under the ADR-703 gate. The implementation was not merged; it is preserved in PR #884 (commit `6a1a2603`), and the flags and files named below exist only there. Self-evaluated: the probe sets and the fitting prompts are written by the corpus authors, so these results show how the penalty interacts with the authored structure, not real-prompt quality.

ADR-700 §5 measured a CSLS hubness penalty matching the body-fusion gain on the content-corpus experiment. ADR-701 §6 lists it as a matching change gated on evaluation. This run implements it behind `matching.hubness: off|single|both` (default `off`) on top of the shipped body score (`body_rank: scaled-single`) and measures it.

## Method

**Fit.** `ways corpus` scores a fitting set once, after the body sidecar, and writes each way's 32 highest scores to `ways-hubness-en.json`, in two lists: the alias cosine, and the scaled body-rank score `(alias + 0.25 × best section) / 1.25`. A way's hub is the mean of its top k. The manifest records the file under `hubness`, and a scan uses it only when its model id matches the installed embedder.

**Fitting set.** Every way's prompt-lane golden prompts, minus the ways `golden --probes` selects (the tree sample and the joined sample draw only from those), minus tool-surface rows and `none` rows. On this tree that is 114 prompts. No prompt in it appears in any evaluation set. The fit uses no labels: each way's hub comes from its scores against every fitting prompt, whichever way the prompt was written for.

**Penalty.** `score − λ × (hub − mean hub)`, the mean taken over all ways in the file (centred). Two other shapes were measured: one-sided, `λ × max(0, hub − mean hub)`, which lowers hubs and raises nothing; and raw, `λ × hub`, which lowers every way.

**Composition with the body score.** The penalty applies to the score the path ranks on, and is drawn from the list fitted on that same score: on the single-vector path under the default, the scaled fused score and the fused list. Penalising the alias cosine before fusion is the same as penalising the fused score by `λ / 1.25` with the alias list; `--hubness-alias-fit` measures the alias list at the same λ.

- **Single-vector path** (`single`, `both`): a prompt that chunks to one piece has its English rows lowered after fusion, before calibration. A prompt of several chunks keeps unpenalised single-vector rows, as it keeps unfused ones.
- **Late interaction** (`both`): each chunk's rows are lowered after any fusion, before masking, share, peak and admission. Confirmation reads the unpenalised score, so corroboration is unchanged.
- Decision records name `hubness` when a path applied it, as they name `body_rank`.

**Runs.** One corpus built from the committed tree (`ways corpus --ways-dir hooks/ways --output DIR`; 184 ways including the registered projects' ways, 857 sections), one binary:

```
ways author probe tests/probes/tree-sample.tsv            --ways-dir hooks/ways --corpus DIR --tsv [HUB]
ways author probe tests/probes/tree-sample-pleasantry.tsv --ways-dir hooks/ways --corpus DIR --tsv [HUB]
ways author probe tests/probes/tree-sample-joined.tsv     --ways-dir hooks/ways --corpus DIR --tsv [HUB]
ways author probe tests/probes/unrelated-*.tsv            --ways-dir hooks/ways --corpus DIR --unrelated [HUB]
HUB = --hubness single|both [--hubness-lambda L] [--hubness-k K] [--hubness-shape centred|one-sided|raw] [--hubness-alias-fit]
```

All six unrelated sets ran (golden-none, routing-none, joined, and each with a pleasantry): 66 rows. "Max unrelated" is the highest calibrated probability any single-vector unrelated row reached against the 0.5 firing bar; the late-path rows score summed share, reported separately.

**Flag off is unchanged.** With `hubness` off, output was compared byte for byte with the binary built from `main` at 51e2affe on the same corpus: identical for `tree-sample`, `tree-sample-pleasantry` and `tree-sample-joined` (`--tsv`), the `tree-sample` table, and three unrelated sets.

## Results

Default = `body_rank: scaled-single`, `hubness: off`. Single = tree-sample (130 scored, 129 on the single-vector path). Joined = tree-sample-joined (79, all late interaction). Cells are pass / top-1 / expected way fires / other ways fired.

| setting | single | joined | unrelated fired (of 66) | max unrelated | margin to 0.5 |
|---|---|---|---|---|---|
| default | 86 / 75 / 91 / 436 | 55 / 64 / 57 / 52 | 0 | 0.461 | 0.039 |
| body rank off, for reference | 81 / 71 / 86 / 459 | 55 / 64 / 57 / 52 | 0 | 0.438 | 0.062 |
| centred, λ 0.1, k 10 | 85 / 76 / 90 / 411 | unchanged | 0 | 0.472 | 0.028 |
| centred, λ 0.15, k 10 | 86 / 77 / 90 / 407 | unchanged | 0 | 0.478 | 0.022 |
| centred, λ 0.25, k 10 | 87 / 79 / 89 / 389 | unchanged | 0 | 0.489 | 0.011 |
| centred, λ 0.25, k 5 | 86 / 79 / 90 / 390 | unchanged | 0 | 0.477 | 0.023 |
| centred, λ 0.25, k 10, alias fit | 87 / 79 / 89 / 385 | unchanged | 0 | 0.486 | 0.014 |
| centred, λ 0.5, k 5 | 88 / 78 / 92 / 366 | unchanged | 0 | 0.494 | 0.006 |
| centred, λ 0.5, k 10 | 88 / 78 / 91 / 350 | unchanged | **2** | 0.518 | below the bar |
| centred, λ 0.5, k 20 | 88 / 77 / 91 / 347 | unchanged | **2** | 0.542 | below the bar |
| centred, λ 1.0, k 10 | 87 / 75 / 89 / 379 | unchanged | **2** | 0.574 | below the bar |
| one-sided, λ 0.1, k 10 | 85 / 77 / 89 / 406 | unchanged | 0 | 0.461 | 0.039 |
| one-sided, λ 0.25, k 10 | 84 / 77 / 86 / 370 | unchanged | 0 | 0.461 | 0.039 |
| one-sided, λ 0.5, k 5 | 82 / 77 / 86 / 308 | unchanged | 0 | 0.461 | 0.039 |
| one-sided, λ 0.5, k 10 | 83 / 74 / 86 / 314 | unchanged | 0 | 0.461 | 0.039 |
| one-sided, λ 1.0, k 10 | 79 / 75 / 81 / 257 | unchanged | 0 | 0.461 | 0.039 |
| raw, λ 0.25, k 10 | 61 / 79 / 62 / 81 | unchanged | 0 | 0.197 | 0.303 |
| raw, λ 0.5, k 10 | 41 / 78 / 41 / 20 | unchanged | 0 | 0.066 | 0.434 |
| `both`, centred, λ 0.25, k 10 | 87 / 79 / 89 / 389 | 55 / 62 / 57 / 48 | 0 | 0.489 | 0.011 |
| `both`, centred, λ 0.5, k 10 | 88 / 79 / 91 / 350 | 55 / 61 / 57 / 48 | **2** | 0.518 | below the bar |
| `both`, one-sided, λ 0.5, k 10 | 83 / 75 / 86 / 314 | 55 / 63 / 57 / 50 | 0 | 0.461 | 0.039 |
| body rank off, centred, λ 0.25, k 10 | 84 / 72 / 87 / 401 | unchanged | 0 | 0.463 | 0.037 |

- **Pleasantry set.** tree-sample-pleasantry matched tree-sample row for row in every setting with body rank on: the scan drops the opener, so the prompt that is penalised is the same.
- **Late-path unrelated rows.** The highest summed share on the joined unrelated prompts rose from 0.227 to 0.258 (`both`, centred, λ 0.25) and 0.278 (λ 0.5). None of them fired.
- **Cost.** About 63 ms per prompt in every setting, the same as the default. A scan reads one 170 KB file when the mode is on.

### Where the centred gains and losses come from

Against the default, centred λ 0.25, k 10 flips 7 rows on tree-sample (4 fail to pass, 3 pass to fail) and changes top-1 on 6; it flips none on the joined set.

| flip | expected way | kind | stage, default to centred |
|---|---|---|---|
| fail to pass | meta/goals | situational | fired to fired (a `must_not` way no longer outranks it) |
| fail to pass | softwaredev/architecture | situational | below-threshold to fired |
| fail to pass | softwaredev/architecture/threat-modeling | direct | below-threshold to fired |
| fail to pass | softwaredev/code/testing | situational | fired to fired (a `must_not` way no longer outranks it) |
| pass to fail | softwaredev/code/supplychain/depscan | situational | fired to below-threshold |
| pass to fail | softwaredev/code/testing/gates | situational | fired to below-threshold |
| pass to fail | softwaredev/code/testing/gates/assertions | situational | fired to below-threshold |

Two of the gains come from ways below the mean hub crossing the threshold after centring raised them, and two from a hub sibling dropping below the expected way. Every loss is a hub's own prompt falling below the threshold. The one-sided shape, which keeps the drops and removes the rises, shows the split: at λ 0.25 it loses the same three rows, gains only `softwaredev/code/testing`, and ends at 84.

The unrelated prompt that comes closest to firing is the same in every centred setting: "translate this sentence to french: the meeting has been moved to noon", ranked first on `ea/calendar`. `ea/calendar` sits below the mean hub, so centring raises it: 0.461 under the default, 0.489 at λ 0.25, 0.518 at λ 0.5, where it fires (with its pleasantry twin, the 2 rows in the table). Under one-sided and raw shapes it never rises.

The centred penalty's gain on pass therefore comes from the same mechanism that spends the separation margin: raising ways the corpus scores low against everything. Suppressing hubs alone (one-sided) cuts stray fires by up to 41% and loses passes.

## Against the gate

ADR-703 §1 adopts a matching change that gains on the path it changes with no unrelated prompt starting to fire. The brief for this run added that the separation margin (0.039 under the default) must not erode.

- **Single-vector path.** No setting gains on pass without spending margin. Centred λ 0.25, k 10 is the best literal pass of the gate (+1 pass, +4 top-1, 47 fewer stray fires, no unrelated fire), and it leaves 0.011 of the 0.039 margin. Every one-sided setting keeps the margin and loses 1 to 7 passes. Raw collapses pass, because it moves every score off the scale the calibration was fitted on.
- **Late interaction.** No gain in any setting: pass stays 55, top-1 falls by 1 to 3, stray fires fall by 2 to 4.
- **Without the body score.** Centred λ 0.25 on alias scores reaches 84 / 72, below the default's 86 / 75. Hubness does not substitute for the body score here, and on top of it adds top-1 and stray-fire reductions that come with a cost in margin or in passes.

**Recommended setting: `hubness: off`.** On these sets the penalty buys top-1 and fewer stray fires only by spending most of the separation margin (centred) or losing passes (one-sided), and it gains nothing on the late path. The mode was not merged; PR #884 holds it for re-measurement. A later change that raised the margin, such as a recalibration fitted on penalised scores, would be measured under the same gate.
