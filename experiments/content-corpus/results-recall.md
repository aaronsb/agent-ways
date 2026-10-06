# Admission recall: where late interaction drops expected ways

Measured 2026-10-05 on branch `spike/admission-recall` (from `ways-graph` 0bdd6619). Question from ADR-700 §10: on surfaces of two or three joined golden prompts, late-interaction admission let through only 302 of the 620 expected ways. Which stage drops them, and which operating points recover recall without flooding the judge?

## Method

- **Corpora and binary.** `make ways-rebuild`, then `OUT=/tmp/recall-out python3 experiments/content-corpus/run.py experiments/content-corpus/golden-synthetic.tsv tests/routing-golden.tsv`. Embeddings come from the `way-embed` the scan resolves first, as in `confirm.py`.
- **Surfaces.** `confirm.py`'s 310 surfaces, seed 11: 110 T+T, 60 T+N, 80 T+T+N, 60 T+T+T. T+T and T+N have two prompts; T+T+N and T+T+T have three. `build_surfaces` keeps only surfaces where each prompt is exactly one chunk, so on these surfaces a prompt never becomes several chunks.
- **Multi-chunk prompts.** Only 3 of the 332 golden prompts split into two chunks on their own. An auxiliary set of 93 surfaces holds each of them alone (3), with one other targeted prompt (45), and with two (45). Chunk ownership comes from chunking each prompt separately. These 93 surfaces carry the "own prompt 2+ chunks" breakdown, and they rest on 3 prompts.
- **Single-prompt surfaces.** 329 of the 332 golden prompts are one chunk alone. `late_interaction::run` returns `None` below 2 chunks and the single-vector gate runs, so a single-sentence prompt never reaches late interaction. The 3 two-sentence prompts do.
- **Pipeline.** `confirm.py`'s port of `late_interaction.rs`, with K, the gates, the cap and the share denominator exposed. At the shipped point it reproduces `confirm.py`: 302 of 620 expected ways admitted, 250 confirmed (302 × 0.828).
- **Stages.** Each expected way is assigned the first stage that drops it, in pipeline order:
  - `topk`: in no chunk's top K, so it gets no softmax mass and is never ranked. A peak alone cannot admit it.
  - `gate`: ranked, but share below the share gate and peak below the peak gate.
  - `cap`: passed the co-gate, but cut by the survivor cap (survivors sorted by peak).
  - `confirm`: survived, but today's body confirm is below 0.35. Today's confirm is the won chunk against the way's `chunk_body` sentences, max.
  - `fired`: confirmed.
- **Confirm scores.** A way's won chunk is its peak chunk over all match rows, and no operating point changes it. So the confirm score depends only on (surface, way). It is computed once for every pair any setting admits, in one batched `way-embed similarity` call, and cached in `$OUT/recall-confirm-cache.json`.
- **Share modes.**
  - `chunks` (shipped): summed mass / n_chunks.
  - `topics`: summed mass / the number of distinct per-chunk top-1 ways, so chunks that agree on a winner count as one topic.
  - `max`: the way's mass in its best single chunk.
  - `top1` and `top2`: admit any way in the top 1 or top 2 of some chunk. The share gate does not apply to them.
  - The peak gate and the cap apply in every mode.
- **Sweep.** For `chunks` and `topics`: K {8, 12, 16} × share {0.05, 0.10, 0.15} × peak {0.40, 0.45, 0.50} × cap {6, 8, 10}. `max` also gets share 0.20, 0.25, 0.30 and 0.40, because on its per-chunk scale the shipped 0.15 on 2–3 chunks corresponds to 0.30–0.45. That gives 432 settings.
- **Labels.** As in `confirm.py`. A way is *relevant* when it is an expected way of the surface, *related* when it is an ancestor or descendant of one, and *irrelevant* otherwise.
- **Metrics.**
  - Expected-way recall after admission and after confirm.
  - Candidates admitted per surface. Each one costs a confirm call.
  - Candidates fired per surface. These reach the judge.
  - Irrelevant candidates in both of those counts. The irrelevant fired count is what the judge would have to reject.

## Where expected ways stop (shipped point)

K 8, share 0.15, peak 0.50, cap 6, share / n_chunks. Main surfaces, every prompt one chunk:

| group | n | topk | gate | cap | confirm | fired |
|---|---|---|---|---|---|---|
| all | 620 | 70 (11%) | 248 (40%) | 0 | 52 (8%) | 250 (40%) |
| 2 prompts | 280 | 31 (11%) | 103 (37%) | 0 | 26 (9%) | 120 (43%) |
| T+T | 220 | 25 | 77 | 0 | 21 | 97 |
| T+N | 60 | 6 | 26 | 0 | 5 | 23 |
| 3 prompts | 340 | 39 (11%) | 145 (43%) | 0 | 26 (8%) | 130 (38%) |
| T+T+N | 160 | 23 | 68 | 0 | 12 | 57 |
| T+T+T | 180 | 16 | 77 | 0 | 14 | 73 |

Rank of the expected way among the match rows of its own prompt's chunk:

| own-chunk rank | n | topk | gate | confirm | fired |
|---|---|---|---|---|---|
| 1 | 396 | 0 | 105 | 52 | 239 |
| 2–3 | 97 | 0 | 86 | 0 | 11 |
| 4–8 | 56 | 0 | 56 | 0 | 0 |
| 9+ | 69 | 68 | 1 | 0 | 0 |

The auxiliary surfaces each contain one multi-chunk prompt:

| group | n | topk | gate | cap | confirm | fired |
|---|---|---|---|---|---|---|
| own prompt 2+ chunks | 93 | 30 (32%) | 47 (51%) | 0 | 0 | 16 (17%) |
| own prompt 1 chunk | 135 | 14 (10%) | 67 (50%) | 0 | 6 (4%) | 48 (36%) |

The three multi-chunk prompts behave differently:

- `documentation/adr/consider` ranks 9th or lower in its own chunks on every surface and is lost at top-k. That is a ranking miss, and no gate setting changes it.
- `meta/develop` is rank 1 in its own chunk on all 31 surfaces and is gated on all 31. Its mass is divided across its own two chunks and the other prompts' chunks.
- `softwaredev/delivery/groundwork/sequence` fires on 16 surfaces and is gated on 15.

**Reading.**

- **The share gate drops the most.** It accounts for 248 of the 370 ways lost before confirm.
  - Gated ways peak on their own chunk (241 of 248), with a mean peak of 0.339 and a mean share of 0.078.
  - 105 of them are the top-1 way of their own chunk. On N chunks, `share = mass / N`, so a chunk's winner needs mass ≥ 0.30 in that chunk on 2 chunks and ≥ 0.45 on 3. At τ 0.08 over 8 close cosines, a winner's mass is often lower.
  - That is why 3-prompt surfaces lose more at the gate (43%) than 2-prompt surfaces (37%).
  - Ways ranked 2–8 in their own chunk almost never clear the gate.
- **Top-k drops 11%.** Nearly all of these rank 9th or lower in their own chunk, which is a ranking miss in the single-sentence alias match.
- **The survivor cap never binds.** At the shipped gates no surface has more than 6 survivors.
- **Confirm drops 52 ways (8%),** all of them top-1 ways that passed the gate.
- **The ceiling is low.** Only 396 of 620 expected ways (64%) are the top-1 way of their own chunk. That is close to the alias router's single-prompt top-1 of 0.656 (ADR-700 §9). No admission rule that relies on per-chunk ranking can go far past it.

## Sweep

In the tables:

- *rec adm* and *rec fired* are expected-way recall after admission and after confirm.
- *adm/s* is candidates admitted per surface (confirm calls).
- *fired/s* is candidates fired per surface (judge candidates).
- *irr* is the irrelevant part of each count.
- *aux* is recall after admission / after confirm on the 93 multi-chunk surfaces.

The full sweep is in `$OUT/recall-sweep.tsv`.

**One factor at a time from the shipped point:**

| change | rec adm | rec fired | adm/s | irr adm/s | fired/s | irr fired/s | aux |
|---|---|---|---|---|---|---|---|
| shipped (K8 S.15 P.50 cap6 chunks) | 0.487 | 0.403 | 1.37 | 0.34 | 1.05 | 0.20 | 0.307 / 0.281 |
| K 12 | 0.461 | 0.390 | 1.24 | 0.27 | 0.99 | 0.17 | 0.307 / 0.281 |
| K 16 | 0.435 | 0.369 | 1.13 | 0.23 | 0.92 | 0.15 | 0.298 / 0.276 |
| share 0.10 | 0.595 | 0.468 | 2.13 | 0.85 | 1.42 | 0.42 | 0.408 / 0.382 |
| share 0.05 | 0.760 | 0.579 | 5.16 | 3.28 | 2.51 | 1.15 | 0.675 / 0.570 |
| peak 0.45 | 0.510 | 0.426 | 1.61 | 0.51 | 1.25 | 0.34 | 0.333 / 0.307 |
| peak 0.40 | 0.585 | 0.485 | 2.42 | 1.10 | 1.88 | 0.79 | 0.430 / 0.386 |
| cap 8 or 10 | 0.487 | 0.403 | 1.37 | 0.34 | 1.05 | 0.20 | 0.307 / 0.281 |
| share / topics | 0.489 | 0.403 | 1.38 | 0.35 | 1.05 | 0.21 | 0.307 / 0.281 |
| max share ≥ 0.30 | 0.527 | 0.429 | 1.55 | 0.44 | 1.17 | 0.28 | 0.421 / 0.377 |
| max share ≥ 0.25 | 0.602 | 0.469 | 1.96 | 0.68 | 1.40 | 0.41 | 0.491 / 0.412 |
| **max share ≥ 0.20** | 0.684 | 0.526 | 2.68 | 1.21 | 1.70 | 0.57 | 0.658 / 0.557 |
| max share ≥ 0.15 | 0.724 | 0.553 | 3.96 | 2.29 | 2.13 | 0.88 | 0.689 / 0.583 |
| **top1 (each chunk's winner)** | 0.656 | 0.503 | 2.56 | 1.16 | 1.53 | 0.47 | 0.654 / 0.557 |
| top2 | 0.731 | 0.550 | 4.85 | 3.09 | 2.34 | 1.05 | 0.684 / 0.588 |

- **Raising K lowers recall.** More ways enter each softmax, so the winner's mass, and its share, falls.
- **The cap does not bind** at any gate near the shipped point. It starts to matter only once share drops to 0.05–0.10 under `max`.
- **Topic normalisation changes nothing here.** On these surfaces each prompt is one chunk, so the number of distinct top-1 ways is almost the number of chunks. Even on the multi-chunk set it barely moves, because the two chunks of one prompt usually have different winners.
- **The peak gate is the costlier lever.** At 0.40 it admits 1.10 irrelevant ways per surface for +10 points of admission recall. Share-side changes buy the same recall for less noise.

**Recall after confirm against noise.** These are points on the frontier of recall after confirm versus irrelevant ways fired per surface, all with peak 0.50 and cap 6:

| setting | rec adm | rec fired | fired/s | irr fired/s |
|---|---|---|---|---|
| K8 chunks S.15 (shipped) | 0.487 | 0.403 | 1.05 | 0.20 |
| K12 max S.30 | 0.485 | 0.405 | 1.05 | 0.20 |
| K16 chunks S.10 | 0.535 | 0.435 | 1.21 | 0.30 |
| K8 chunks S.10 | 0.595 | 0.468 | 1.42 | 0.42 |
| K8 top1 | 0.656 | 0.503 | 1.53 | 0.47 |
| K8 max S.20 | 0.684 | 0.526 | 1.70 | 0.57 |
| K8 max S.15 | 0.724 | 0.553 | 2.13 | 0.88 |
| K8 max S.10 | 0.769 | 0.589 | 2.72 | 1.31 |
| K16 max S.05 cap10 (most recall) | 0.815 | 0.618 | 3.82 | 2.18 |

From the shipped point to `max` 0.15, each point of recall after confirm costs about 0.045 irrelevant fired ways per surface. Past `max` 0.15 it costs about 0.20, more than four times as much.

## The two best settings

Paired surface bootstrap against the shipped point (2000 resamples, 95%):

| setting | rec fired Δ | irr fired/s Δ | confirm calls / surface | judge candidates / surface |
|---|---|---|---|---|
| **top1**: K 8, admit each chunk's top-1 way or peak ≥ 0.50, cap 6 | +0.100 [+0.076, +0.125] | +0.27 [+0.21, +0.33] | 1.37 → 2.56 | 1.05 → 1.53 |
| **max 0.20**: K 8, best single-chunk mass ≥ 0.20 or peak ≥ 0.50, cap 6 | +0.122 [+0.098, +0.148] | +0.37 [+0.30, +0.44] | 1.37 → 2.68 | 1.05 → 1.70 |

Stage loss under each, on the main surfaces:

| setting | topk | gate | cap | confirm | fired |
|---|---|---|---|---|---|
| shipped | 70 | 248 | 0 | 52 | 250 |
| top1 | 70 | 143 | 0 | 95 | 312 |
| max 0.20 | 70 | 126 | 0 | 98 | 326 |

- On the multi-chunk surfaces, both settings bring the ways whose own prompt is 2+ chunks from 16 of 93 fired to 62 and 61. `meta/develop` and `groundwork/sequence` now fire. `adr/consider` stays lost at top-k.
- Both settings let the gate stop depending on the number of chunks, which is the defect: a chunk's winner is admitted whether the surface has 2 chunks or 3.
- **Confirm becomes the second-largest loss.** It rejects 95–98 expected ways: 23% of admitted relevant ways under top1, up from 17% at the shipped point, and 43 of the 105 ways top1 newly admits. The new admits have weaker body support. The irrelevant ways that confirm keeps (0.47 or 0.57 per surface) go to the judge.
- **Judge cost.** If the judge runs once per candidate, its calls rise 46% (top1) or 62% (max 0.20). If it runs once per surface over all candidates, the call count is unchanged and each call carries about half a candidate more.
- **Confirm cost.** Under today's per-survivor subprocess (79 ms each, `results-confirm.md`), confirm calls nearly double, adding about 95 ms per surface. The section sidecar of ADR-700 §10 removes that cost once the match pass returns chunk vectors.

**Recommended operating point: top1.** Admit every chunk's top-1 way, keep the peak co-gate at 0.50, K 8 and cap 6, and drop the share gate.

- It recovers 10 points of recall after confirm on the main surfaces (0.403 → 0.503) and 28 points on the multi-chunk set (0.281 → 0.557).
- It sends the judge one more irrelevant candidate about every four surfaces.
- It has no share threshold to tune, and its admission recall (0.656) sits at the ceiling the alias ranking allows (0.639 rank-1, plus peak admits).
- `max 0.20` buys 2.3 more points of recall for 0.10 more irrelevant fired ways per surface and a tuned threshold. It is the alternative if recall is worth more than judge load.
- Neither is clearly better than the other. Both are clearly better than any setting of the shipped share rule at equal noise.

## Limits

- **Synthetic prompts.** The prompts are model-written golden rows joined as sentences. They are not real multi-sentence prompts with response context.
- **Synthetic labels.** An "irrelevant" way can be useful, a parent domain for example. Related ways are counted separately (0.04–0.07 fired per surface at the picks).
- **Multi-chunk prompts rest on n = 3.** Only 3 golden prompts are multi-sentence. The auxiliary set repeats each of them about 31 times, so its rows are 3 data points, not 93. The main surfaces exclude multi-chunk prompts by construction.
- **The new modes are not in the binary.** `top1`, `top2`, `max` and `topics` exist only in this port. The port was checked against `ways author match` (in `confirm.py`) only at the shipped constants. Here it reproduces `confirm.py` exactly at the shipped point (302 / 250).
- **Confirm was batched.** Confirm scores came from one batched `similarity` call per run, not one per survivor. The cosine of a pair does not depend on its batch, so this should not change scores. It was not checked pair by pair against the per-call scores.
- **Only today's confirm was used.** Confirm (b), the section sidecar, was not swept. `results-confirm.md` shows it keeps and rejects at the same rates at 0.35.
- **No surface over 3 prompts** and no real-session surfaces were tested. The confirm gate was held at 0.35.
- **Sampling noise.** One point of recall is about 6 ways. The bootstrap intervals above are over the 310 main surfaces only.

## Conclusion

The share gate, not the cap or confirm, drops most expected ways: 248 of 620 (40%), 105 of them the top-1 way of their own chunk. `share = mass / n_chunks` asks a chunk's winner for 0.30 of its chunk's mass on two chunks and 0.45 on three, and the survivor cap never binds. Admitting each chunk's top-1 way, with the peak co-gate kept, raises recall after confirm from 0.403 to 0.503 (+0.100, CI +0.076 to +0.125) and from 0.281 to 0.557 on surfaces with a multi-sentence prompt. The judge then sees 1.53 candidates per surface in place of 1.05, 0.47 of them irrelevant in place of 0.20. Recall beyond about 0.66 at admission needs better per-chunk ranking, not looser gates: 11% of expected ways rank below 8th in their own chunk, and 36% are not its winner.

## Reproduce

```sh
make ways-rebuild
OUT=/tmp/recall-out python3 experiments/content-corpus/run.py experiments/content-corpus/golden-synthetic.tsv tests/routing-golden.tsv
OUT=/tmp/recall-out python3 experiments/content-corpus/recall.py experiments/content-corpus/golden-synthetic.tsv tests/routing-golden.tsv
```

The first `recall.py` run takes about 4 minutes, nearly all of it the batched confirm call. Later runs read `$OUT/recall-confirm-cache.json`. The output includes `$OUT/recall.log`, if redirected, `$OUT/recall-sweep.tsv` (every setting) and `$OUT/recall.json` (per-way traces at the shipped point, and the sweep).
