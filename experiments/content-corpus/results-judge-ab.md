# Judge input A/B: scores, band and described route

Run 2026-10-05 with `judge_ab.py`, for ADR-701 §4 and issue #664. The question: does the relevance judge reject more irrelevant candidates without losing relevant ones when each candidate's text carries (B) share, margin and a band label, (C) the described route, or (D) both, against (A) today's text?

## Method

**Arms.** Only `Candidate.text` varies. Everything else in the request is the production request from `tools/ways-agent/src/net.rs` and `tools/ways-agent-core/src/judge.rs` with the shipped `anthropic` profile: model `claude-haiku-4-5`, temperature 0, `max_tokens` 64 + 48n, the same system prompt and instruction, the strict `record_judgements` tool forced by `tool_choice`, one request per prompt with every candidate, the last turn cut to 1,200 characters, and `<` neutralised. P(yes) is the confidence of a yes, or one minus the confidence of a no. A candidate passes at P(yes) ≥ 0.3.

| Arm | Candidate text |
|---|---|
| A | `a › b › c`, then the way's description (today's `way_text`) |
| B | A, then `matcher: <band> match (share 0.71 of the top 8 ways, margin 0.195 over the next way)` |
| C | the route, then one line per ancestor directory, `within a › b: <description>`, then the way's description |
| D | C, then B's line |

- **Scores.** Share is a softmax at τ 0.08 over the prompt's top 8 ways by alias cosine. Margin is the cosine gap to the next-ranked way. Both follow `scan/candidate_log.rs`. Band: share ≥ 0.5 strong, < 0.35 weak, otherwise uncertain (ADR-700 §4).
- **Ancestor descriptions.** Issue #664's resolution: the description of the way in that directory. The six nodes with no way (`softwaredev`, `meta`, `itops`, `collaboration`, `workstation`, `meta/knowledge/authoring`) take the authored lines in `region-descriptions.tsv`. Every ancestor on every path had a description.
- **Golden set.** `golden-synthetic.tsv` plus `tests/routing-golden.tsv`: 332 prompts, 314 targeted and 18 `none`. Candidates are the top 3 by alias cosine for a targeted prompt and the top 2 for a `none` prompt, in rank order, 978 in all. Alias top-1 is 0.656, as in ADR-700. The expected way is in the top 3 for 81% of targeted prompts. *Strict* labels count only the expected way as relevant (254 relevant, 724 irrelevant). *Family* labels also count the expected way's parent or a direct child (319 / 659).
- **Real fires.** The ADR-195 probe's labelled prompt-lane fires (set 1 `real_relevant` and `real_irrelevant`, set 2 random), read from the probe's private directory: the last user turn and the label. No session transcript was read. One item's way is no longer in the corpus and was dropped. That leaves 139 items in 128 prompts (52 relevant, 87 irrelevant), mostly one candidate per prompt. Share and margin come from scoring the turn against today's alias corpus. The committed cache holds item ids, way ids, labels, scores and verdicts for these, and no conversation text.
- **Statistics.** AUC is Mann-Whitney on P(yes). The best threshold maximises rejected minus lost (Youden). Paired differences use a bootstrap over prompts (2,000 resamples, 95% percentile intervals). Every call succeeded, so every prompt is paired across all four arms.

## Cost

| | Calls | Input tokens | Output tokens | USD |
|---|---|---|---|---|
| Estimate (count_tokens on a 25-prompt sample per arm and set, output from the ADR-195 per-candidate rate) | 1,840 | 2.12 M | 0.17 M | 2.95 |
| Actual, from the API's usage fields | 1,840 | 2,111,820 | 153,760 | **2.88** |

The price is Haiku 4.5 at $1 per million input tokens and $5 per million output tokens. Mean input per call was 1,024 tokens for A, 1,100 for B, 1,196 for C and 1,271 for D. p50 latency was 0.95 to 1.00 s for every arm. p95 was 1.56 to 1.69 s, measured from this script at 4 calls in parallel.

## Results

### Golden set, strict labels (332 prompts, 254 relevant / 724 irrelevant)

| Arm | AUC | Irrelevant rejected at 0.3 | Relevant lost at 0.3 | Best threshold | Rejected / lost at best |
|---|---|---|---|---|---|
| A | 0.980 | 74.3% | 0.8% | 0.92 | 97.8% / 10.2% |
| B | 0.984 | 73.2% | 0.0% | 0.85 | 92.4% / 3.5% |
| C | 0.968 | 72.8% | 1.6% | 0.92 | 95.6% / 10.2% |
| D | 0.976 | 73.6% | 0.4% | 0.82 | 91.2% / 5.1% |

| Pair | ΔAUC [95% CI] | Δ rejected | Δ lost |
|---|---|---|---|
| B − A | +0.003 [−0.002, +0.008] | −1.1% [−3.4, +1.2] | −0.8% [−2.0, 0.0] |
| C − A | −0.012 [−0.021, −0.004] | −1.5% [−3.7, +0.8] | +0.8% [−0.8, +2.7] |
| D − A | −0.004 [−0.012, +0.003] | −0.7% [−3.0, +1.6] | −0.4% [−1.7, +0.8] |
| D − C | +0.008 [+0.001, +0.015] | +0.8% [−1.4, +3.1] | −1.2% [−2.7, 0.0] |
| D − B | −0.008 [−0.013, −0.003] | +0.4% [−1.7, +2.8] | +0.4% [0.0, +1.2] |

### Golden set, family labels (319 relevant / 659 irrelevant)

| Arm | AUC | Rejected at 0.3 | Lost at 0.3 | Best threshold | Rejected / lost at best |
|---|---|---|---|---|---|
| A | 0.941 | 77.7% | 8.8% | 0.85 | 88.8% / 15.0% |
| B | 0.936 | 75.7% | 9.7% | 0.80 | 93.0% / 17.9% |
| C | 0.947 | 76.8% | 7.8% | 0.85 | 87.3% / 11.3% |
| D | 0.939 | 76.5% | 9.4% | 0.75 | 85.7% / 11.3% |

No difference against A is outside its interval: B −0.006 [−0.015, +0.003], C +0.006 [−0.005, +0.015], D −0.002 [−0.013, +0.009] AUC. The differences in rejected and lost at 0.3 are within ±2 points, and every interval contains zero.

### Real fires (128 prompts, 52 relevant / 87 irrelevant)

| Arm | AUC | Rejected at 0.3 | Lost at 0.3 | Best threshold | Rejected / lost at best |
|---|---|---|---|---|---|
| A | 0.899 | 74.7% | 7.7% | 0.65 | 74.7% / 7.7% |
| B | 0.922 | 74.7% | 5.8% | 0.72 | 79.3% / 7.7% |
| C | 0.902 | 64.4% | 5.8% | 0.85 | 82.8% / 19.2% |
| D | 0.913 | 73.6% | 9.6% | 0.72 | 75.9% / 9.6% |

| Pair | ΔAUC [95% CI] | Δ rejected | Δ lost |
|---|---|---|---|
| B − A | +0.023 [−0.001, +0.052] | 0.0% [−6.4, +6.3] | −1.9% [−6.5, 0.0] |
| C − A | +0.003 [−0.040, +0.044] | **−10.3% [−18.2, −2.4]** | −1.9% [−10.7, +6.0] |
| D − A | +0.014 [−0.023, +0.052] | −1.1% [−9.8, +7.3] | +1.9% [−4.1, +8.9] |
| D − C | +0.011 [−0.021, +0.052] | +9.2% [+2.4, +16.1] | +3.8% [−5.0, +13.1] |

### Anchoring: does the judge echo the band?

Pass rate at 0.3 by band and strict label, golden set:

| Band, label | A | B | C | D |
|---|---|---|---|---|
| strong, relevant (119) | 99% | 100% | 100% | 100% |
| strong, irrelevant (8) | 38% | 38% | 38% | 38% |
| uncertain, relevant (40) | 98% | 100% | 98% | 100% |
| uncertain, irrelevant (21) | 29% | 29% | 19% | 24% |
| weak, relevant (95) | 100% | 100% | 97% | 99% |
| weak, irrelevant (695) | 25% | 27% | 27% | 26% |

- The judge does not echo the band. On strong and weak candidates, the verdict matches the band's suggestion (strong passes, weak blocks) 68.9% of the time with the band shown (B) and 69.7% without it (A). With the route described, the figures are 69.2% (D) and 68.7% (C). On real fires they are 52.8% against 54.3% (B against A) and 55.1% against 47.2% (D against C).
- Showing the band flipped 68 golden verdicts against A. 29 moved toward the band and became correct. 37 moved against it and became wrong, almost all weak irrelevant candidates that now passed. The weak-relevant cell is where the band would mislead, and the judge passed it at 100% either way.
- Most candidates are weak: 790 of 978 on the golden set and 118 of 139 real fires. The real fires are weak because the way that fired is seldom the top alias match for the turn alone.

## Limits

- **Ceiling on strict labels.** Arm A already reaches 0.980 AUC with 0.8% relevant lost at 0.3, so the golden set has little room to show a gain. The family variant shows that many "irrelevant" siblings are arguably relevant.
- **Real fires are few.** 52 relevant items in mostly single-candidate prompts. Intervals span about ±5 AUC points and ±7 points in rejection. Their labels come from one model (ADR-195), and their share and margin are computed against today's corpus from the last turn alone, not at fire time.
- **Synthetic golden prompts** are model-written single sentences. The candidate pool is the top 3 or top 2 by alias cosine, not the late-interaction admission or the matcher's thresholds, so it differs from what production would send.
- **One run per arm.** At temperature 0 ADR-195 saw no flips over repeats; that was not re-checked here.
- **One phrasing per arm.** Another wording of the score line or the described route could do better or worse. B's line explains share and margin in words because the judge receives no other definition.
- **Not production timing.** The calls ran with a 60 s timeout instead of the 2 s deadline, so no call failed open.

## Conclusion

No arm rejects more irrelevant candidates than today's input at the production threshold: on both sets every difference in rejection is within noise or negative. The score line (B) is the only arm with a consistent, small upside. It lost no relevant golden candidates (2 fewer than A) and raised AUC on real fires by 0.023, with intervals that touch zero, and the judge does not echo the band it is shown. The described route (C) lowered golden AUC by 0.012 and rejected 10 points fewer irrelevant real fires (interval −18 to −2), and combining it with scores (D) only recovers A's level. Neither C nor D meets ADR-701 §4's bar, and C repeats the probe's finding that ancestor descriptions do not help the judge (ADR-195, issue #664). B also falls short of that bar, which asks for more irrelevant fires rejected. What it shows is fewer relevant ways lost, and confirming that needs a larger labelled set of real fires.

Reproduce: `OUT=/tmp/judge-ab-out python3 experiments/content-corpus/judge_ab.py prepare && python3 experiments/content-corpus/judge_ab.py estimate && python3 experiments/content-corpus/judge_ab.py run --cap 15 && python3 experiments/content-corpus/judge_ab.py analyze`. With `judge-ab-calls.jsonl` present, `run` makes no calls and `analyze` reproduces these tables. The real-fire set needs the probe's private directory. Without it, `prepare` builds the golden set alone.
