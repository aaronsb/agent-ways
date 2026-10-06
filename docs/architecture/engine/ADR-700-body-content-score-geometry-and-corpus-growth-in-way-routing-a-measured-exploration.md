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
  - ADR-702
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
- When first read on 2026-10-05 the event log held 79,097 events from 2026-07-01 to 2026-10-05, including 26,466 `way_fired` with `fire_score`, 7,455 `way_nearmiss` with `margin`, and 726 `way_judged` with `p_yes` and `verdict`. It records neither prompt text nor the ranked candidate list.
- The event log's size cap (`KEEP_EVENTS_BYTES`, 24 MiB in `session.rs`) deletes the oldest lines permanently. Later on 2026-10-05 the same file held 59,916 lines starting 2026-08-13: about six weeks of history were dropped during the day. At October's volume of 3,000 to 8,000 events a day the cap keeps about two weeks.

### 9. Choosing a region first does not beat flat competition

Added 2026-10-05 from `experiments/content-corpus/subtree.py` and `results-subtree.md`, on the same 314 targeted rows.

| Method | top-1 | none AUC | fixed / broken vs flat | p |
|---|---|---|---|---|
| flat cosine | 0.656 | 0.947 | | |
| domain first, region scored by its best way | 0.656 | 0.947 | 0 / 0 | 1.00 |
| domain first, region scored by mean of its top 3 | 0.618 | 0.951 | 9 / 21 | 0.04 |
| domain first, region scored by its root way | 0.599 | 0.967 | 13 / 31 | 0.01 |
| top 3 domains by top-3 mean, then the way | 0.653 | 0.947 | 0 / 1 | 1.00 |
| tree descent by mean of top 3 | 0.510 | 0.956 | 13 / 59 | < 0.01 |
| way score + 0.1 × domain top-3 mean | 0.659 | 0.953 | 1 / 0 | 1.00 |
| hubness-corrected flat | 0.678 | 0.933 | 14 / 7 | 0.19 |
| hubness-corrected + 0.25 × domain top-3 mean | 0.682 | 0.948 | 14 / 6 | 0.12 |
| domain given by an oracle (upper bound) | 0.771 | 0.947 | 36 / 0 | < 0.01 |

- A region scored by its best way always contains the overall best way, at every level, so region-first selection scored that way reproduces flat argmax exactly. Every other region score loses, because it rewards crowded regions or penalises regions with no root way. `softwaredev` and `meta` have no root way.
- Of the 108 flat misses, 69 pick a way in the wrong domain and 39 the wrong way inside the right domain. An oracle domain would fix 36 of the 69.
- No region-first method changes how fast top-1 falls as the corpus grows (12.9 points from 34 to 137 ways for flat, 11 to 15 for the others, within noise).
- The headroom the oracle shows needs a region signal independent of the way scores, such as embedded directory descriptions or a domain classifier. Those were not tested. Regions defined by authored edges were not tested either.

### 10. Body confirmation from section vectors is no worse, and much faster

Added 2026-10-05 from `experiments/content-corpus/confirm.py` and `results-confirm.md`: 310 surfaces of two or three joined golden prompts, run through a Python port of the late-interaction pipeline that matched `ways author match` on 30 sampled surfaces within 0.001 for peak, share and confirm. 424 candidates were admitted: 302 relevant, 106 irrelevant.

| Confirmation | AUC | relevant kept at 0.35 | irrelevant rejected at 0.35 |
|---|---|---|---|
| first 8 prose sentences, embedded per call (today) | 0.659 | 0.828 | 0.406 |
| all heading sections, precomputed | 0.692 | 0.834 | 0.406 |
| sections, alias vector for a way with none | 0.697 | 0.841 | 0.406 |

- The AUC difference is +0.033 with a 95% bootstrap interval of −0.019 to +0.083: no worse, not shown better. The shipped gate of 0.35 keeps the same balance with sections.
- Two ways have no sections (`itops/policy`, `softwaredev/code/security/injection`); without the alias fallback their real matches are rejected. On the twelve ways with one section, AUC is 0.589 today and 0.690 with sections.
- The confirm stage costs a mean of 108 ms per surface today, from about 1.4 `way-embed similarity` subprocesses at 79 ms each. With preloaded section vectors it costs under 1 ms. That saving needs the match pass to return the won chunks' vectors, which `way-embed match` does not do today; without it one embed call per surface remains.
- Confirmation rejects about 41% of collisions either way. It is a weak filter, and the judge carries most of the precision.
- Admission let through 302 of the 620 ways the surfaces were built to match. Late-interaction recall on multi-topic surfaces is low, before confirmation runs.
- Two `way-embed` builds are installed and differ by about 0.002 in cosine: `~/.cache/agent-ways/user/way-embed`, which the scan resolves first, and `~/.claude/bin/way-embed`.

### 11. Region signals independent of way scores do not recover the headroom

Added 2026-10-05 from `experiments/content-corpus/region.py` and `results-region.md`. Three region signals were embedded with the shipped model: directory descriptions resolved as #664 proposes (6 of 37 written for the experiment from the ways' own descriptions, without reading the golden prompts), centroids of each domain's alias vectors, and centroids of its body-section vectors.

| Method | top-1 | fixed / broken vs flat | p | share of the oracle's 11.5 points |
|---|---|---|---|---|
| flat cosine | 0.656 | | | 0% |
| oracle domain | 0.771 | 36 / 0 | < 0.01 | 100% |
| best region-first (alias centroids) | 0.459 | 9 / 71 | < 0.01 | −172% |
| best top-2 regions (hubness-corrected alias centroids) | 0.551 | 11 / 44 | < 0.01 | −92% |
| way score + 0.1 × description similarity | 0.669 | 6 / 2 | 0.29 | 11% |
| hubness-corrected flat | 0.678 | 14 / 7 | 0.19 | 19% |
| hubness-corrected + 0.1 × body-centroid similarity | 0.675 | 13 / 7 | | 17% |

- Each independent signal picks the right domain less often (18.5 to 57.6%) than the domain of the flat best way already is (78.0%), so every hard region choice loses.
- The given descriptions fail as well as the written ones: `documentation` and `ea` win their own prompts 34% and 30% of the time, against 59% and 85% for flat. One vector per region does not carry enough to choose among regions.
- The best soft bonus was chosen on the rows it is reported on, so its 11% is optimistic. On hubness-corrected scores it adds nothing.
- No signal changes the corpus-growth slope.
- Not tested: multi-vector regions, a trained domain classifier, and a reader of the described route. #664's own proposal, the route as judge input, is a separate question this measures nothing about.

### 12. The share gate drops most expected ways on multi-topic surfaces

Added 2026-10-05 from `experiments/content-corpus/recall.py` and `results-recall.md`, on the 310 joined-prompt surfaces of §10 (620 expected ways), with the Python port of the late-interaction matcher. The new admission modes exist only in the port.

| Stage where an expected way stops, shipped settings | Ways |
|---|---|
| not in a chunk's top 8 | 70 (11%) |
| share below 0.15 and peak below 0.50 | 248 (40%) |
| 6-survivor cap | 0 |
| confirm below 0.35 | 52 (8%) |
| fired | 250 (40%) |

- Share divides a way's summed mass by the number of chunks, so a chunk's own winner needs 0.30 of that chunk's mass on a two-chunk surface and 0.45 on three. 241 of the 248 gated ways peak on their own prompt's chunk, and 105 are that chunk's top-ranked way.
- 329 of the 332 golden prompts form one chunk alone, so a single prompt never reaches late interaction; the single-vector path decides it.
- Raising top-K lowers recall, the cap never binds, normalising share by topic count changes nothing, and lowering the peak gate to 0.40 adds 1.10 irrelevant admissions per surface.

| Admission | recall after confirm | change, 95% interval | candidates per surface to the judge (irrelevant) |
|---|---|---|---|
| shipped | 0.403 | | 1.05 (0.20) |
| each chunk's top-ranked way, or peak ≥ 0.50 | 0.503 | +0.100 [+0.076, +0.125] | 1.53 (0.47) |
| best single-chunk mass ≥ 0.20, or peak ≥ 0.50 | 0.526 | +0.122 [+0.098, +0.148] | 1.70 (0.57) |

- Either change raises recall by 10 to 12 points and sends the judge 46 to 62% more candidates, with confirmations per surface rising from 1.37 to about 2.6.
- Only 64% of expected ways are the top-ranked way of their own chunk, so recall above about 0.66 after admission needs better ranking within a chunk, not looser gates.
- Evidence on prompts that split into several chunks rests on three golden prompts.

### 13. Scores and the described route do not improve the judge

Added 2026-10-05 from `experiments/content-corpus/judge_ab.py` and `results-judge-ab.md`: 1,840 calls to the production judge (claude-haiku-4-5, temperature 0, the shipped prompt, tool and threshold 0.3), varying only the candidate text. Cost $2.88. Arms: A today's input; B adds share, margin and a band (ADR-700 §4 bounds); C adds the described route (#664's resolution); D adds both.

| Set | Arm | AUC | irrelevant rejected at 0.3 | relevant lost at 0.3 |
|---|---|---|---|---|
| golden, 254 relevant / 724 irrelevant | A | 0.980 | 74.3% | 0.8% |
| | B | 0.984 | 73.2% | 0.0% |
| | C | 0.968 | 72.8% | 1.6% |
| | D | 0.976 | 73.6% | 0.4% |
| earlier probe's labelled real fires, 52 / 87 | A | 0.899 | 74.7% | 7.7% |
| | B | 0.922 | 74.7% | 5.8% |
| | C | 0.902 | 64.4% | 5.8% |
| | D | 0.913 | 73.6% | 9.6% |

- No arm rejects more irrelevant candidates than today's input. B is the only arm with a consistent small upside (fewer relevant lost; AUC +0.023 on real fires, interval −0.001 to +0.052).
- The described route lowers AUC on the golden set (−0.012, interval −0.021 to −0.004) and rejects 10.3 points fewer irrelevant real fires (interval −18.2 to −2.4), repeating the earlier probe's finding against ancestor descriptions.
- The judge does not echo the band: its verdict agrees with the band 68.9% of the time with the band shown and 69.7% without.
- Today's input already scores 0.980 AUC on the golden set, which leaves little room. The real-fire set is small and model-labelled; candidates were the top 3 by cosine, not production's admitted set.

## Limits

- Prompts are model-written. A labelled set of real prompts from session transcripts was not built; reading transcripts for it needs the operator's explicit approval.
- Single-sentence, single-vector scoring only.
- Ranking only; no thresholds or calibration were fitted.
- Gains of about 2 points are within noise at this size.
- Hubness correction and body fusion were not tested together, and neither was routing within a subtree first.
