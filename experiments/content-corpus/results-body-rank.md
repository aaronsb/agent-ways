# Body score in ranking (ADR-701 §6)

Measured 2026-10-08 on branch `adr-701-body-rank`. ADR-701 §6 adopts `alias + 0.25 × best section` as the ranking score only after an evaluation shows a gain with no loss in separating relevant prompts from unrelated ones, and says that once adopted, body confirmation uses a section other than the one that contributed to the ranking score. This run implements that rule behind `matching.body_rank: off|on` (default `off`) and measures it.

## Method

**What the flag does.** With `body_rank: on` and a complete body sidecar (ADR-701 §7), each way's cosine against a prompt chunk becomes `alias + 0.25 × max over the way's section vectors`.

- **Late-interaction path** (2 or more chunks). The fused cosine replaces the alias cosine in every chunk's rows before masking, softmax share, peak, the peak co-gate and the admission rule. Confirmation then reads the chunk the way won, sets aside the section that contributed to that chunk's score, and takes the best of the remaining sections. A way with one section or none confirms against its alias cosine on that chunk.
- **Single-vector path** (a prompt that chunks to exactly one piece; a prompt of several chunks whose late-interaction run fails stays on alias cosines). The prompt's chunk is matched once more with its vector returned, and each English row becomes `alias + 0.25 × best section`. The calibration `g(s)` then maps the fused cosine exactly as it maps an alias cosine. There is no confirmation stage on this path.
- **Not touched:** the Bash lane, the multilingual lane, the retired-key and sidecar-absent states. Without a complete sidecar the flag does nothing, so fused and alias-only scores are never mixed in one ranking.

ADR-701 does not scope the fusion to one path ("the alias matcher, with the fused score once adopted"), so both paths take it. The two paths form scores in different places (the late path in `fuse_if_on`, before `rank_and_admit`; the single path in `fuse_single`, on the `EmbedScores` rows), both through the one function `fused_score`.

**Runs.** One corpus built from the committed tree (`ways corpus --ways-dir hooks/ways --output DIR`, sidecar included), and the same binary for every run:

```
ways author probe tests/probes/tree-sample.tsv        --ways-dir hooks/ways --corpus DIR [--body-rank on] --tsv
ways author probe tests/probes/tree-sample-joined.tsv --ways-dir hooks/ways --corpus DIR [--body-rank on] --tsv
ways author probe NONE.tsv --ways-dir hooks/ways --corpus DIR --body-rank off|on --unrelated
```

`--body-rank` overrides `matching.body_rank` for the run and is named in the summary header whenever the mode is not `off` (the first pass printed it only for `on`). `--unrelated` ignores each row's expected way and prints the way ranked first, its score (summed share on the late path, calibrated probability on the single-vector path) and the ways that fired.

**Unrelated sets.** (Committed as `tests/probes/unrelated-golden-none.tsv`, `unrelated-routing-none.tsv` and `unrelated-joined.tsv`; run them with `--unrelated`.) All `none` rows: the 15 prompts of `hooks/ways/golden-none.jsonl` and the 3 `none` rows of `tests/routing-golden.tsv`. All 18 are one sentence, so they exercise only the single-vector path. To cover the late path, 15 joined unrelated prompts were built from the same 18 (nine pairs and six triples, sentences joined with `. `). The converted files are probe TSVs with `expected_way = none`.

**Flag off is unchanged.** Output with the flag off (default) was compared byte for byte with the output of the binary built from `main` before this branch: `cmp` reports no difference for the table and the `--tsv` form on both `tree-sample.tsv` and `tree-sample-joined.tsv`.

## Results: relevant prompts

Pass = the expected way fires and no `must_not` way outranks it. Top-1 = the expected way ranks first. "Other ways fired" counts the ways that fired on a probe besides the expected one, summed over rows.

### tree-sample.tsv (single-sentence; 129 of 130 scored rows take the single-vector path)

| metric | off | on |
|---|---|---|
| scored | 130 | 130 |
| pass | 81 (62.3%) | 97 (74.6%) |
| top-1 | 71 | 74 |
| expected way fires | 86 | 107 |
| rows where another way also fired | 94 | 120 |
| other ways fired (total) | 456 | 1375 |

Expected way's final stage, all scored rows: below-threshold 42 to 22, fired 86 to 107, keyword-gated 1 to 0, not-admitted 1 to 1. Of the failing rows: below-threshold 42 to 22, fired but a `must_not` way outranked it 5 to 10, keyword-gated 1 to 0, not-admitted 1 to 1.

Flipped rows: 17 fail to pass, 1 pass to fail.

| flip | expected way | kind | stage off to on |
|---|---|---|---|
| fail to pass | data/migrations/idempotent | situational | below-threshold to fired |
| fail to pass | documentation/adr | situational | below-threshold to fired |
| fail to pass | documentation/api | situational | below-threshold to fired |
| pass to fail | itops/policy | direct | fired to fired (a `must_not` way now outranks it) |
| fail to pass | itops/proposals | situational | below-threshold to fired |
| fail to pass | meta/trust | situational | below-threshold to fired |
| fail to pass | meta/wrap | direct | keyword-gated to fired |
| fail to pass | meta/wrap | situational | fired to fired (a `must_not` way no longer outranks it) |
| fail to pass | softwaredev/architecture | situational | below-threshold to fired |
| fail to pass | softwaredev/architecture/threat-modeling | direct | below-threshold to fired |
| fail to pass | softwaredev/architecture/threat-modeling | situational | below-threshold to fired |
| fail to pass | softwaredev/code/quality | situational | below-threshold to fired |
| fail to pass | softwaredev/code/security/injection/prompt | situational | below-threshold to fired |
| fail to pass | softwaredev/code/supplychain | situational | below-threshold to fired |
| fail to pass | softwaredev/code/supplychain/depscan | situational | below-threshold to fired |
| fail to pass | softwaredev/code/testing/gates | situational | below-threshold to fired |
| fail to pass | softwaredev/code/testing/gates/assertions | situational | below-threshold to fired |
| fail to pass | writing | situational | below-threshold to fired |

Top-1 changed on 9 rows: 6 gained, 3 lost.

### tree-sample-joined.tsv (79 rows, all late interaction)

| metric | off | on |
|---|---|---|
| scored | 79 | 79 |
| pass | 55 (69.6%) | 54 (68.4%) |
| top-1 | 64 | 64 |
| expected way fires | 57 | 55 |
| rows where another way also fired | 40 | 39 |
| other ways fired (total) | 52 | 69 |

Expected way's final stage, all scored rows: below-threshold 2 to 2, fired 57 to 55, not-admitted 8 to 7, not-confirmed 12 to 15. Of the failing rows: fired but outranked 2 to 1, not-admitted 8 to 7, not-confirmed 12 to 15.

Flipped rows: 7 fail to pass, 8 pass to fail.

| flip | expected way | stage off to on |
|---|---|---|
| pass to fail | documentation | fired to not-confirmed |
| pass to fail | documentation/api | fired to not-confirmed |
| pass to fail | itops/incident | fired to not-confirmed |
| pass to fail | itops/policy | fired to fired (a `must_not` way now outranks it) |
| fail to pass | itops/proposals | not-admitted to fired |
| pass to fail | meta/choices | fired to not-confirmed |
| pass to fail | meta/knowledge/authoring/pii-free | fired to not-confirmed |
| pass to fail | meta/trust/delegation | fired to not-confirmed |
| fail to pass | meta/wrap | fired to fired (a `must_not` way no longer outranks it) |
| fail to pass | softwaredev/architecture | not-confirmed to fired |
| fail to pass | softwaredev/architecture/threat-modeling | not-confirmed to fired |
| pass to fail | softwaredev/delivery/groundwork/permission | fired to not-confirmed |
| fail to pass | softwaredev/delivery/release | fired to fired (a `must_not` way no longer outranks it) |
| fail to pass | softwaredev/environment | not-confirmed to fired |
| fail to pass | workstation/pkghistory | not-confirmed to fired |

Top-1 changed on 4 rows: 2 gained, 2 lost. Seven of the eight losses are rows that were admitted and confirmed against their best section before, and fail confirmation against the next-best section now. The confirmation rule is stricter by construction, and more rows land in `not-confirmed` (12 to 15).

**Offset: single-section ways.** Three of the seven joined fail-to-pass rows come from the rule for a way with one section, not from the fusion. Such a way has no other section, so confirmation reduces to its alias cosine on the won chunk, which must reach the confirm gate of 0.35. With the flag off, the same rows are confirmed against their one section, which scores low:

| way | stage off | confirm off (the one section) | stage on | confirm on (alias cosine) |
|---|---|---|---|---|
| softwaredev/architecture | not-confirmed | 0.2138 | fired | 0.3804 |
| softwaredev/environment | not-confirmed | 0.1415 | fired | 0.4439 |
| itops/proposals | not-admitted | - | fired | 0.4484 (admitted on fused peak 0.5076, which crosses the 0.50 peak co-gate) |

The alias cosine is higher than the section cosine for these ways, so they pass. Without these three the late set would show 4 fail-to-pass against 8 pass-to-fail.

## Results: separation from unrelated prompts

A loss is more unrelated rows firing a way, or higher top scores that move a row toward the bar. The bar on the single-vector path is a calibrated probability of 0.5; on the late path a way is admitted at share 0.15 (or peak 0.50) and then has to confirm.

| set | path | rows | rows firing, off | rows firing, on | max top score, off | max top score, on | mean top score, off | mean top score, on |
|---|---|---|---|---|---|---|---|---|
| golden-none.jsonl | single | 15 | 0 | **1** | 0.4370 | 0.7876 | 0.1048 | 0.1825 |
| routing-golden.tsv `none` | single | 3 | 0 | 0 | 0.0437 | 0.0856 | 0.0372 | 0.0697 |
| joined unrelated (built from the 18) | late | 15 | 0 | 0 | 0.2266 | 0.2417 | 0.1374 | 0.1492 |

The one row that fires with the flag on is "translate this sentence to french: the meeting has been moved to noon". The top way, `ea/calendar`, goes from probability 0.4370 to 0.7876, and `ea/calendar`, `ea/briefing` and `ea/intelligence` all fire. The top score rises on all 15 golden-none rows. The fused cosine raises every way's cosine by about 0.25 × (its best section cosine), and the calibration was fitted on alias cosines, so the increase moves ways across the bar that the alias score left below it. The effect shows in the relevant sets too: ways fired besides the expected one go from 456 to 1375 across the 130 single-sentence probes.

## Control: the same fusion, rescaled

To separate "the body section carries information" from "every score went up", the same runs were repeated with the fused cosine divided by 1.25, which keeps the alias scale and the calibration's meaning. This is an experiment, not shipped: it was a temporary line in `fused_score` (reverted), so the numbers below are reproducible by dividing there.

| set | pass off | pass rescaled | top-1 off | top-1 rescaled | fires off | fires rescaled | other ways fired off | rescaled |
|---|---|---|---|---|---|---|---|---|
| tree-sample (single) | 81 | 86 | 71 | 75 | 86 | 91 | 456 | 433 |
| tree-sample-joined (late) | 55 | 54 | 64 | 65 | 57 | 54 | 52 | 44 |

Flips, rescaled: single-sentence 6 fail to pass and 1 pass to fail; joined 6 fail to pass and 7 pass to fail. Unrelated rows firing: 0 of 15 (golden-none), 0 of 3 (routing-golden), 0 of 15 (joined); max top scores 0.4606, 0.0430 and 0.1970 against 0.4370, 0.0437 and 0.2266 with the flag off.

## Verdict against ADR-701 §6

ADR-701 §6 adopts the fused score only when the evaluation shows a gain with no loss in separating relevant prompts from unrelated ones.

**As specified (`alias + 0.25 × best section`): keep off.**

- On the single-sentence set the pass rate rises 16 points (81 to 97), but the separation check fails. One unrelated golden-none prompt fires three ways, the top score on unrelated prompts rises on 15 of 15 rows (max 0.437 to 0.788), and other ways fired across the relevant probes triple (456 to 1375). The gain comes largely from raising every score past a bar that was calibrated on alias cosines.
- On the late-interaction set there is no gain: pass 55 to 54, top-1 unchanged at 64, expected-way fires 57 to 55, with 8 rows lost and 7 gained. The different-section confirmation rule costs seven rows that confirmed before.
- The unrelated late-path check shows no row firing, but top scores rise.

**Open for the operator.** The rescaled control shows the section carries signal that does not come from inflation: on single-sentence prompts it gains 5 passes and 4 top-1 with no unrelated row firing and fewer other ways fired (456 to 433). It still shows no gain on the late path (55 to 54 passes, 3 fewer expected fires). The ADR's formula without a rescaling or a refit of the calibration on fused cosines does not pass its own rule. Adopting a rescaled or refitted variant is a change to the ADR's text and needs its own evaluation on held-out real prompts, which this run did not have: all three sets are tree-sampled or hand-written, and the fit and the evaluation share the corpus.

`matching.body_rank` stays `off`. The default is not changed here.

## Not covered

- Held-out real prompts (ADR-701 §6 names them). The tree-sampled probes and the golden none rows are what the repo holds.
- The Bash lane and the multilingual lane.
- A calibration refit on fused cosines (ADR-700 §10 measured fusion on top-1 only).
- The 18 unrelated prompts are all one sentence; the 15 joined ones are synthetic combinations of the same 18.

## 2026-10-08, second pass: the scaled blend and the lone-section confirm

Measured on branch `adr-701-body-rank-scaled`, after #881 merged. The first pass found that `alias + 0.25 × best section` raises every score above the scale the calibration was fitted on, and the rescaled control looked better than the literal formula. This pass adds that blend as a mode and measures it beside the others.

### Method

- **`matching.body_rank: scaled`.** Rank score = (alias + w × best section) / (1 + w), w = 0.25. It sits on the alias scale, so the calibration `g(s)` and the peak and share gates keep their meaning. `on` stays the literal ADR formula for comparison. Confirmation under both is the first pass's rule: set aside the contributing section, and a way with one section or none confirms against its alias cosine on the won chunk (raw, never scaled).
- **Lone-section confirm variant.** A way with exactly one section confirms against that section, reusing it, instead of against its alias. It is a probe-only switch (`--single-section-confirm`) and cannot be set from config.
- **Weight.** `--body-rank-weight W` (probe only) sets w for the sweep. Without it w is 0.25.
- **Runs.** The same corpus, probe files and unrelated sets as the first pass, one binary. Commands, with `NONE` standing for each of the three unrelated files:

```
ways author probe tests/probes/tree-sample.tsv        --ways-dir hooks/ways --corpus DIR --body-rank off|on|scaled [--single-section-confirm] [--body-rank-weight W] --tsv
ways author probe tests/probes/tree-sample-joined.tsv --ways-dir hooks/ways --corpus DIR (same flags) --tsv
ways author probe NONE.tsv                            --ways-dir hooks/ways --corpus DIR (same flags) --unrelated
```

- **Check on the harness.** The `off` and `on` outputs of this build are byte-identical to the first pass's, and `off` is byte-identical to the build from `main` before either pass.
- "Other ways fired" is the number of ways that fired on a probe besides the expected one, summed over rows. Flipped rows are against `off`. The flipped rows for `on` are in the first pass above.

### tree-sample (single-sentence)

| config | scored | pass | top-1 | expected fires | other ways fired |
|---|---|---|---|---|---|
| off | 130 | 81 | 71 | 86 | 456 |
| on | 130 | 97 | 74 | 107 | 1375 |
| scaled | 130 | 86 | 75 | 91 | 433 |
| scaled + lone-section confirm | 130 | 86 | 75 | 91 | 433 |
| on + lone-section confirm | 130 | 97 | 74 | 107 | 1375 |

Stage of the expected way, all scored rows:

| stage | off | on | scaled | scaled + lone-section confirm | on + lone-section confirm |
|---|---|---|---|---|---|
| below-threshold | 42 | 22 | 38 | 38 | 22 |
| fired | 86 | 107 | 91 | 91 | 107 |
| keyword-gated | 1 | 0 | 0 | 0 | 0 |
| not-admitted | 1 | 1 | 1 | 1 | 1 |

Flipped rows, off to scaled: 6 fail to pass, 1 pass to fail.

| flip | expected way | stage off to scaled | prompt |
|---|---|---|---|
| pass to fail | meta/trust/delegation | fired to below-threshold | the draft is done, now publish it as me to the shared calendar and inv |
| fail to pass | meta/wrap | keyword-gated to fired | i'm done for today, wrap things up and give me something to paste next |
| fail to pass | meta/wrap | fired to fired | context is nearly full and it's late, land what's in flight and leave  |
| fail to pass | softwaredev/architecture/threat-modeling | below-threshold to fired | we're about to expose an internal service to partners and i want to th |
| fail to pass | softwaredev/code/supplychain/depscan | below-threshold to fired | before running the install on this project, check whether any of the p |
| fail to pass | softwaredev/code/testing/gates | below-threshold to fired | the scanner said zero findings and ci was green, but i'm not sure it e |
| fail to pass | softwaredev/code/testing/gates/assertions | below-threshold to fired | the test stays green even though i broke the feature, the expected val |

Flipped rows, off to scaled + lone-section confirm: 6 fail to pass, 1 pass to fail.

| flip | expected way | stage off to scaled + lone-section confirm | prompt |
|---|---|---|---|
| pass to fail | meta/trust/delegation | fired to below-threshold | the draft is done, now publish it as me to the shared calendar and inv |
| fail to pass | meta/wrap | keyword-gated to fired | i'm done for today, wrap things up and give me something to paste next |
| fail to pass | meta/wrap | fired to fired | context is nearly full and it's late, land what's in flight and leave  |
| fail to pass | softwaredev/architecture/threat-modeling | below-threshold to fired | we're about to expose an internal service to partners and i want to th |
| fail to pass | softwaredev/code/supplychain/depscan | below-threshold to fired | before running the install on this project, check whether any of the p |
| fail to pass | softwaredev/code/testing/gates | below-threshold to fired | the scanner said zero findings and ci was green, but i'm not sure it e |
| fail to pass | softwaredev/code/testing/gates/assertions | below-threshold to fired | the test stays green even though i broke the feature, the expected val |

### tree-sample-joined (late)

| config | scored | pass | top-1 | expected fires | other ways fired |
|---|---|---|---|---|---|
| off | 79 | 55 | 64 | 57 | 52 |
| on | 79 | 54 | 64 | 55 | 69 |
| scaled | 79 | 54 | 65 | 54 | 44 |
| scaled + lone-section confirm | 79 | 52 | 65 | 52 | 44 |
| on + lone-section confirm | 79 | 51 | 64 | 52 | 68 |

Stage of the expected way, all scored rows:

| stage | off | on | scaled | scaled + lone-section confirm | on + lone-section confirm |
|---|---|---|---|---|---|
| below-threshold | 2 | 2 | 2 | 2 | 2 |
| fired | 57 | 55 | 54 | 52 | 52 |
| not-admitted | 8 | 7 | 8 | 8 | 7 |
| not-confirmed | 12 | 15 | 15 | 17 | 18 |

Flipped rows, off to scaled: 6 fail to pass, 7 pass to fail.

| flip | expected way | stage off to scaled | prompt |
|---|---|---|---|
| pass to fail | documentation | fired to not-confirmed | our project documentation has grown into a sprawl with no consistent f |
| pass to fail | documentation/api | fired to not-confirmed | the mobile client keeps getting different error bodies from different  |
| pass to fail | itops/incident | fired to not-confirmed | customers are reporting errors since the last release, should we rever |
| pass to fail | meta/choices | fired to not-confirmed | you went with the redis approach and never told me there were other wa |
| pass to fail | meta/knowledge/authoring/pii-free | fired to not-confirmed | this guidance file i'm about to share still has my coworker's name and |
| pass to fail | meta/trust/delegation | fired to not-confirmed | the draft is done, now publish it as me to the shared calendar and inv |
| fail to pass | meta/wrap | fired to fired | context is nearly full and it's late, land what's in flight and leave  |
| fail to pass | softwaredev/architecture | not-confirmed to fired | we keep arguing about where responsibilities should live across the wh |
| fail to pass | softwaredev/architecture/threat-modeling | not-confirmed to fired | we're about to expose an internal service to partners and i want to th |
| pass to fail | softwaredev/delivery/groundwork/permission | fired to not-confirmed | all our recent patches are in the files we're allowed to edit while th |
| fail to pass | softwaredev/delivery/release | fired to fired | shipping v3 tomorrow, the exact build that passed ci should go out, no |
| fail to pass | softwaredev/environment | not-confirmed to fired | new laptop, nothing builds yet, not sure whether it's missing installs |
| fail to pass | workstation/pkghistory | not-confirmed to fired | this laptop has accumulated a ton of random software over three years  |

Flipped rows, off to scaled + lone-section confirm: 4 fail to pass, 7 pass to fail.

| flip | expected way | stage off to scaled + lone-section confirm | prompt |
|---|---|---|---|
| pass to fail | documentation | fired to not-confirmed | our project documentation has grown into a sprawl with no consistent f |
| pass to fail | documentation/api | fired to not-confirmed | the mobile client keeps getting different error bodies from different  |
| pass to fail | itops/incident | fired to not-confirmed | customers are reporting errors since the last release, should we rever |
| pass to fail | meta/choices | fired to not-confirmed | you went with the redis approach and never told me there were other wa |
| pass to fail | meta/knowledge/authoring/pii-free | fired to not-confirmed | this guidance file i'm about to share still has my coworker's name and |
| pass to fail | meta/trust/delegation | fired to not-confirmed | the draft is done, now publish it as me to the shared calendar and inv |
| fail to pass | meta/wrap | fired to fired | context is nearly full and it's late, land what's in flight and leave  |
| fail to pass | softwaredev/architecture/threat-modeling | not-confirmed to fired | we're about to expose an internal service to partners and i want to th |
| pass to fail | softwaredev/delivery/groundwork/permission | fired to not-confirmed | all our recent patches are in the files we're allowed to edit while th |
| fail to pass | softwaredev/delivery/release | fired to fired | shipping v3 tomorrow, the exact build that passed ci should go out, no |
| fail to pass | workstation/pkghistory | not-confirmed to fired | this laptop has accumulated a ton of random software over three years  |

### Unrelated prompts

| set | config | rows | rows firing | max top score | mean top score |
|---|---|---|---|---|---|
| golden-none.jsonl (single) | off | 15 | 0 | 0.4370 | 0.1048 |
| golden-none.jsonl (single) | on | 15 | 1 | 0.7876 | 0.1825 |
| golden-none.jsonl (single) | scaled | 15 | 0 | 0.4606 | 0.0916 |
| golden-none.jsonl (single) | scaled + lone-section confirm | 15 | 0 | 0.4606 | 0.0916 |
| golden-none.jsonl (single) | on + lone-section confirm | 15 | 1 | 0.7876 | 0.1825 |
| routing-golden none (single) | off | 3 | 0 | 0.0437 | 0.0372 |
| routing-golden none (single) | on | 3 | 0 | 0.0856 | 0.0697 |
| routing-golden none (single) | scaled | 3 | 0 | 0.0430 | 0.0362 |
| routing-golden none (single) | scaled + lone-section confirm | 3 | 0 | 0.0430 | 0.0362 |
| routing-golden none (single) | on + lone-section confirm | 3 | 0 | 0.0856 | 0.0697 |
| joined unrelated (late) | off | 15 | 0 | 0.2266 | 0.1374 |
| joined unrelated (late) | on | 15 | 0 | 0.2417 | 0.1492 |
| joined unrelated (late) | scaled | 15 | 0 | 0.1970 | 0.1288 |
| joined unrelated (late) | scaled + lone-section confirm | 15 | 0 | 0.1970 | 0.1288 |
| joined unrelated (late) | on + lone-section confirm | 15 | 0 | 0.2417 | 0.1492 |

### Weight sweep, scaled, tree-sample (single-sentence)

| w | pass | top-1 | expected fires | other ways fired | golden-none rows firing | golden-none max top | routing none firing |
|---|---|---|---|---|---|---|---|
| off (no body) | 81 | 71 | 86 | 456 | 0 | 0.4370 | 0 |
| 0.15 | 82 | 71 | 87 | 437 | 0 | 0.4524 | 0 |
| 0.25 | 86 | 75 | 91 | 433 | 0 | 0.4606 | 0 |
| 0.35 | 87 | 73 | 93 | 447 | 0 | 0.4677 | 0 |

### Weight sweep, scaled, tree-sample-joined (late), same runs

| w | pass | top-1 | expected fires | other ways fired | joined-unrelated rows firing |
|---|---|---|---|---|---|
| off (no body) | 55 | 64 | 57 | 52 | 0 |
| 0.15 | 52 | 65 | 53 | 44 | 0 |
| 0.25 | 54 | 65 | 54 | 44 | 0 |
| 0.35 | 53 | 66 | 53 | 45 | 0 |

### Reading the numbers

These are the figures, with no choice made. The points the tables show:

- On the single-sentence set, `scaled` gains 5 passes and 4 top-1 over `off`, fires 5 more expected ways, fires fewer other ways (433 against 456), and fires no unrelated row. `on` gains 16 passes and fires one unrelated row and 1375 other ways.
- On the late-interaction set no mode beats `off` on passes (55): `on` 54, `scaled` 54, `scaled` with the lone-section confirm 52, `on` with it 51. Top-1 is 64 (`off`, `on`), 65 (`scaled`).
- The lone-section confirm changes nothing on the single-sentence set, which has no confirmation stage. On the late set it lowers passes by 2 under `scaled` and 3 under `on`, because it takes back the two or three rows the alias confirm admitted (`softwaredev/architecture` and `softwaredev/environment` stop passing under `scaled`).
- The sweep on the single-sentence set: pass 81 (off), 82, 86, 87 at w = 0.15, 0.25, 0.35; top-1 71, 71, 75, 73; other ways fired 456, 437, 433, 447. No unrelated row fires at any w; the golden-none maximum top score is 0.4370, 0.4524, 0.4606, 0.4677. On the late set, pass is 52, 54, 53 at the three weights against 55 for `off`.

## Shipped default: `body_rank: scaled-single`

The operator chose the scaled blend (w = 0.25) on the single-vector path only, with late interaction left on alias scores, as the default. `scaled-single` is that mode; `off`, `on` and `scaled` stay selectable, and the weight is the constant 0.25 (no config key). Decision records carry the mode string only when the scan actually ranked on fused scores, so a late-path scan under the default carries nothing.

Runs on the final build with the default config (no `--body-rank` flag), against the same corpus as above:

```
ways author probe tests/probes/tree-sample.tsv        --ways-dir hooks/ways --corpus DIR --tsv
ways author probe tests/probes/tree-sample-joined.tsv --ways-dir hooks/ways --corpus DIR --tsv
ways author probe NONE.tsv                            --ways-dir hooks/ways --corpus DIR --unrelated   (golden-none, routing-golden none, joined unrelated)
```

| set | result under the default | compared with |
|---|---|---|
| tree-sample (single-sentence) | pass 86, top-1 75, expected way fires 91, other ways fired 433 | the `scaled` numbers above: the same |
| tree-sample-joined (late) | pass 55, top-1 64, expected way fires 57, other ways fired 52 | `off`: rows byte-identical; the whole output identical once the header's `body rank: scaled-single` is removed |
| golden-none.jsonl (15 rows, single) | 0 rows fire | |
| routing-golden `none` rows (3, single) | 0 rows fire | |
| joined unrelated (15 rows, late) | 0 rows fire | |

On tree-sample the default and `scaled` differ in the two rows of the one multi-sentence probe (`meta/develop`, takes the late path): share 0.1172 under the default (alias scores) against 0.1188 under `scaled`. Neither passes, and no count changes. The summary header names the mode when the body score is in use, so default-config probe output now carries ` · body rank: scaled-single`; `--body-rank off` reproduces the earlier output byte for byte.

`bash tests/test-routing-golden.sh` scores raw cosines through way-embed and does not read this setting. It reports top-1 40/40 (100%, floor 90%) and none 3/3 below 0.30, the same before and after the change.

### Single-vector path: one pass for scores and vector

The single-vector path used to run `way-embed` twice per prompt with the flag on: once for the alias scores (`--query` on the reduced prompt) and once in `--batch` mode for the query vector. `way-embed match` returns vectors only in `--batch` mode, so the first English pass now uses `--batch --vectors` for a prompt that chunks to one piece, and the alias scores and the section cosines come from the same embedding. The chunk text is the whitespace-normalised reduced prompt; the old alias pass embedded the reduced prompt as is. Prompts of several chunks, a sidecar that is not complete, or a failed pass score exactly as before. With `body_rank: off` nothing changes: output is byte-identical to `main` on both probe sets, table and `--tsv`.

Re-measured with the default config on the same corpus: tree-sample, tree-sample-joined and all three unrelated sets are **byte-identical** to the Shipped default outputs above. No number moved. Probe time over `tests/probes/tree-sample.tsv` (130 rows, release build, three runs each):

| build | per prompt |
|---|---|
| before, default config | 96.0 to 96.4 ms |
| after, default config | 62.6 to 64.1 ms |
| before, `--body-rank off` | 61.6 to 62.5 ms |
| after, `--body-rank off` | 61.5 to 61.9 ms |
