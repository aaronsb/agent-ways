---
contract: adr/v1
kind: evidence
capability: matching
status: proposed
date: 2026-10-05
deciders:
  - aaronsb
related:
  - ADR-125
  - ADR-127
  - ADR-156
  - ADR-160
  - ADR-502
  - ADR-701
---

# ADR-700: Body content, score geometry and corpus growth in way routing: a measured exploration

Measurements taken 2026-10-05 on the 137 committed ways in `hooks/ways`, with the shipped MiniLM-L6 embedder. They revisit ADR-127, which rejected embedding way bodies in place of the `description + vocabulary` alias, and they extend the question to keeping a body corpus beside the alias, to score transforms, and to how routing behaves as the corpus grows. ADR-701 cites this record.

## Method

**Golden set.** 332 rows: 272 synthetic prompts, one `direct` and one `situational` per way, written by subagents that read each way file and its neighbours; 15 `none` prompts that no way covers; and the 45 rows of `tests/routing-golden.tsv`. A `situational` prompt describes the user's situation without naming the topic. One slice of 68 prompts had its `situational` rows rewritten after the author read the way bodies; every result below holds with that slice removed. The set is at `experiments/content-corpus/golden-synthetic.tsv`.

**Corpora.** The alias corpus is the one `ways corpus` builds. Each body corpus holds the way's prose with frontmatter, code blocks, tables, HTML comments and the See Also section removed, chunked three ways:

| Chunking | Chunks | Per way |
|---|---|---|
| sentence | 2,087 | 15.2 |
| window of 3 sentences, stride 2 | 946 | 6.9 |
| heading-bounded section, split at 120 words | 647 | 4.7 |

Two ways have no body prose and get no chunks.

**Scoring.** Each prompt is embedded once and scored against every way, single-vector, with `way-embed match --threshold 0.0`. A way's body score is its best chunk's cosine. Measures are top-1 accuracy, mean reciprocal rank, recall at 3 and 5, AUC of the right way against wrong ways, AUC of right-way scores against the top score on `none` prompts, and top-1 precision by quintile of each score statistic. Thresholds and calibration (ADR-156) were not evaluated, and neither was the late-interaction path (ADR-160), because golden prompts are one sentence.

**Harness.** `experiments/content-corpus/run.py` builds the corpora and scores fusion variants. `signal.py` reports score separation. `geometry.py` reports score transforms, the domain split and corpus growth. A first run passed `--threshold -1`, which `way-embed` read as each way's own threshold for alias rows but not for body rows; that inflated the fusion gain to +23 fixed and 2 broken. The figures below come from the corrected runs.

## Findings

### 1. Body content replacing the alias loses

| Method | top-1 | recall@5 | situational top-1 |
|---|---|---|---|
| alias | 0.656 | 0.850 | 0.449 |
| body only, section | 0.468 | 0.717 | 0.353 |
| body only, sentence | 0.385 | 0.697 | 0.250 |
| max of alias and body, or reciprocal rank fusion | 0.49 to 0.61 | | |

This agrees with ADR-127.

### 2. A light linear fusion helps slightly

| Method | top-1 | recall@5 | situational top-1 | fixed / broken vs alias |
|---|---|---|---|---|
| alias + 0.25 × body, section | 0.682 | 0.866 | 0.493 | 16 / 8 |
| alias + 0.25 × body, sentence | 0.672 | 0.876 | 0.478 | 13 / 8 |
| alias + 0.25 × body, window | 0.675 | 0.869 | 0.485 | 13 / 7 |

A sign test on 16 against 8 gives p ≈ 0.15. The direction holds for all three chunkings and with the body-informed slice removed (12 / 6). Higher weights fix more rows and break more. Section chunking is the best and the cheapest. Body content also adds its own noise: "add a to-do to call the dentist by friday and mark the passport renewal as done" moved to `data/migrations`. `tests/routing-golden.tsv` scores 97.5 to 100% under every variant because it was tuned against the aliases, so it cannot measure this.

### 3. Body content does not strengthen the match signal

| Score | right way, median | best wrong way, median | margin, median | AUC, all ways | AUC, top-5 candidates |
|---|---|---|---|---|---|
| alias | 0.423 | 0.365 | 0.053 | 0.966 | 0.823 |
| body, section | 0.417 | 0.416 | −0.006 | 0.926 | 0.772 |
| alias + 0.25 × body | 0.518 | 0.460 | 0.059 | 0.968 | 0.818 |

Fusion raises right and wrong scores together.

### 4. Relative scores carry the signal that raw cosine lacks

Top-1 correct rate by quintile of each alias statistic:

| Statistic | lowest | 2nd | 3rd | 4th | highest |
|---|---|---|---|---|---|
| raw cosine | 40% | 48% | 60% | 87% | 92% |
| margin over the second way | 36% | 38% | 67% | 87% | 100% |
| softmax share, τ 0.08, over the top 8 | 36% | 37% | 68% | 89% | 98% |
| z-score against all ways | 36% | 48% | 71% | 79% | 94% |

A raw cosine near 0.40 is right about half the time. Margin and share split into a near coin-flip band and a band right 98 to 100% of the time.

### 5. Dot product and Euclidean distance add nothing; hubness correction does

`way-embed` L2-normalises every vector (`way-embed.cpp:275`), and every stored vector has norm 1.0000. Dot product equals cosine, and Euclidean distance is √(2 − 2·cos), so all three rank identically.

| Score | top-1 | AUC, top-5 | right vs `none` AUC | top-1 precision by margin quintile |
|---|---|---|---|---|
| cosine | 0.656 | 0.823 | 0.947 | 35 / 38 / 67 / 87 / 100% |
| centred on the mean way vector | 0.656 | 0.861 | 0.800 | 39 / 41 / 63 / 87 / 97% |
| hubness penalty (CSLS), k = 10 | 0.678 | 0.834 | 0.933 | 34 / 41 / 75 / 89 / 100% |
| centred and hubness | 0.669 | 0.864 | 0.829 | 26 / 44 / 75 / 89 / 100% |

The hubness penalty is each way's mean top-10 cosine over prompts, fitted without labels on half the prompts and evaluated on the other half. It matches the body-fusion gain with no extra vectors and keeps the `none` separation. Centring sharpens the ranking among candidates and weakens the separation from `none` prompts.

### 6. Routing degrades as the corpus grows

Random subsets of the 137 ways, 20 draws per size, scored on the prompts whose way survived:

| Ways | alias top-1 | alias median margin | + 0.25 × body | hubness |
|---|---|---|---|---|
| 34 | 0.785 | 0.121 | 0.795 | 0.788 |
| 68 | 0.729 | 0.095 | 0.748 | 0.745 |
| 102 | 0.684 | 0.077 | 0.706 | 0.701 |
| 137 | 0.656 | 0.068 | 0.682 | 0.678 |

Each doubling costs 6 to 7 points of top-1, and the margin falls by about half across the range. Fusion and hubness keep a steady lead and do not change the slope. Random subsets remove siblings as well as domains, so the curve mostly measures crowding.

### 7. Non-software ways route worse, and both techniques help them more

| Domain of the expected way | n | alias | + body | hubness |
|---|---|---|---|---|
| softwaredev | 167 | 0.731 | 0.749 | 0.731 |
| all others | 147 | 0.571 | 0.605 | 0.619 |
| documentation | 29 | 0.448 | 0.517 | 0.552 |
| meta | 57 | 0.544 | 0.561 | 0.596 |

Per-domain counts are small.

### 8. Code as found

- Disabled domains and ways are embedded in the corpus and filtered in `scan/candidates.rs` after matching. The late-interaction matcher scores against the whole corpus file, so a disabled way still takes softmax mass and one of the six confirmation slots before it drops at confirmation for lack of a body path.
- `corpus.rs` `is_stale` compares way-file modification times only. A configuration change does not mark the corpus stale.
- `body_confirm` embeds up to 8 body sentences per survivor on every multi-sentence prompt.
- `scanner.rs` walks the ways tree with `follow_links(true)`, so a symlinked way directory would be scanned as a second way.
- Two basenames repeat, against ADR-110 §7: `documentation.md` and `prompt.md`.
- The event log holds 79,097 events from 2026-07-01 to 2026-10-05, including 26,466 `way_fired` with `fire_score`, 7,455 `way_nearmiss` with `margin`, and 726 `way_judged` with `p_yes` and `verdict`. It records neither prompt text nor the ranked candidate list.

## Limits

- Prompts are model-written. A labelled set of real prompts from session transcripts was not built; reading transcripts for it needs the operator's explicit approval.
- Single-sentence, single-vector scoring only.
- Ranking only; no thresholds or calibration were fitted.
- Gains of about 2 points are within noise at this size.
- Hubness correction and body fusion were not tested together, and neither was routing within a subtree first.
