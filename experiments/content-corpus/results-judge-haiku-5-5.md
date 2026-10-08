# Relevance judge on Claude Haiku 5.5: threshold and adoption check

Question: does the judge's 0.3 pass threshold hold on Haiku 5.5, and should 5.5 replace Haiku 4.5 as the shipped judge? Run on 2026-10-08.

Verdict: no. Haiku 5.5 ranks worse than 4.5 outside the noise, its p95 latency is over 3 s, and no single threshold on 5.5 matches 4.5's operating point. Haiku 4.5 with threshold 0.3 and a 2000 ms timeout stays shipped. 5.5 remains selectable per profile.

## Decision rule (set by the operator before the analysis)

Keep 5.5 only if both hold, otherwise stay on 4.5:

1. Within noise: the 95% interval of the paired bootstrap difference AUC(5.5) minus AUC(4.5) includes 0, for strict and family labels.
2. Time acceptable: the timeout the rule picks (p99 x 1.2, rounded up to 500 ms) is at most 2500 ms, and 5.5's p95 is at most 2000 ms.

Both failed (below), so the profiles keep Haiku 4.5, threshold 0.3, timeout_ms 2000.

## Method

- Set: all 332 golden groups, arm A (today's `way_text`: route, then description), as in `results-judge-ab.md`. Candidates are the alias-cosine top 3 (targeted prompt) or top 2 (`none` prompt). 978 candidates (972 in the 330 paired groups).
- Baseline: the cached `claude-haiku-4-5` arm-A rows, not re-run. The cache's gid numbering predates the current golden export, so rows are paired by the Anthropic request hash (same prompt, candidates and order). 330 groups pair; one call failed (below) and one fresh group has no cached 4.5 row.
- 5.5 request: `--provider openrouter --model anthropic/claude-haiku-5.5`, built as `net.rs` builds the OpenRouter body: chat completions, system message, `record_judgements` as a forced function tool, `max_tokens` 96 + 64n, no `temperature` (`accepts_sampling` is false for 5.5). One attempt per group, sequential, no retries.
- Bounds: first a 50-group spot check (cap $0.01, 60 calls), then the remaining 282 groups (cap $0.08 and 340 calls in total). The loop stops before a call that could cross the cap.
- Labels: `strict` is the expected way; `family` is the expected way, its parent or a child, as `judge_ab.py` defines them.
- Bootstrap: 4000 resamples of groups, seed 11, `judge_ab.py compare --sample 332`.

## Spend

332 judge calls (50 spot check + 282), $0.05232 provider-reported (OpenRouter cost). The Anthropic `count_tokens` estimates for the spot check were free and are not judge calls.

## Results, full set (strict labels: 253 relevant, 719 irrelevant; family: 318 relevant, 654 irrelevant)

| | Haiku 4.5 | Haiku 5.5 |
|---|---|---|
| AUC strict | 0.980 | 0.957 |
| AUC family | 0.941 | 0.904 |
| strict at 0.3: irrelevant passed / rejected | 185 / 534 (25.7% passed) | 416 / 303 (57.9% passed) |
| strict at 0.3: relevant passed | 251 / 253 (99.2%) | 252 / 253 (99.6%) |
| family at 0.3: irrelevant passed / rejected | 146 / 508 (22.3% passed) | 353 / 301 (54.0% passed) |
| family at 0.3: relevant passed | 290 / 318 (91.2%) | 315 / 318 (99.1%) |
| P(yes) of irrelevant candidates, median / p75 | 0.15 / 0.70 (strict) | 0.60 / 0.70 (strict) |

Paired bootstrap of AUC(5.5) minus AUC(4.5), 95% interval:

| labels | difference | interval | includes 0 |
|---|---|---|---|
| strict | -0.0229 | [-0.0370, -0.0100] | no |
| family | -0.0365 | [-0.0555, -0.0178] | no |

5.5 as a function of threshold (all 332 groups):

| threshold | strict irrelevant passed | strict recall | family irrelevant passed | family recall |
|---|---|---|---|---|
| 0.3 | 57.9% | 99.6% | 54.0% | 99.1% |
| 0.6 | 51.7% | 99.6% | 47.7% | 98.1% |
| 0.7 | 36.3% | 98.0% | 34.3% | 89.7% |
| 0.75 | 22.9% | 97.2% | 22.0% | 84.0% |
| 0.8 | 19.3% | 96.9% | 18.6% | 82.4% |
| 0.9 | 5.0% | 81.5% | 5.2% | 65.5% |

4.5's operating point at 0.3 is 25.7% irrelevant passed with 99.2% strict recall, and 22.3% with 91.2% family recall. On 5.5, matching the irrelevant pass rate takes a threshold of 0.75 (strict recall 97.2%, family recall 84.0%, which is below 4.5's 91.2%). Holding family recall at or above 4.5's takes a threshold of 0.65 or lower, where about half of irrelevant candidates pass. No single 5.5 threshold matches 4.5 on both axes. The nearest compromise, about 0.75, gives up 7 points of family recall.

## Latency (5.5 via OpenRouter, 331 successful calls)

| p50 | p90 | p95 | p99 | max | over 2000 ms |
|---|---|---|---|---|---|
| 1535 ms | 2257 ms | 3296 ms | 6625 ms | 7337 ms | 52 (15.7%) |

The cached 4.5 median for the same groups is about 970 ms. The timeout rule gives p99 x 1.2 = 7950 ms, rounded up to 8000 ms, against a limit of 2500 ms. A timeout that long would hold a prompt for seconds when the provider is slow, and the current 2000 ms timeout would fall back (inject every matched way) on 15.7% of calls.

## Request failures

One of 332 calls failed: group g214 returned HTTP 200 but its tool-call arguments did not parse into the expected shape (`'str' object has no attribute 'get'` while reading `judgements`), at 1440 ms. No HTTP errors, no provider errors, no timeouts.

## Spot check (first 50 groups)

The first 50 groups, a seeded stratified sample (all 18 `none` groups and 32 targeted ones), were run first at $0.00758. They read AUC 0.985 (4.5) against 0.982 (5.5), and 5.5 passed 34.5% of irrelevant candidates at 0.3 against 19.0%, with recall 16 of 16 for both; the threshold that matched on that sample was 0.75 to 0.80. The full set confirms the shift in scale, and shows what 50 groups could not resolve: the ranking gap, which is outside the noise at 332 groups.

## Reproduce

```
WAYS_BIN=tools/target/release/ways python3 judge_ab.py prepare
python3 judge_ab.py estimate --sample 50 --arm A
python3 judge_ab.py run --arm A --provider openrouter --model anthropic/claude-haiku-5.5 --sample 332 --cap 0.08
python3 judge_ab.py compare --model anthropic/claude-haiku-5.5 --provider openrouter --sample 332
```

Cached rows in `judge-ab-calls.jsonl` carry `model` and `provider`; older rows default to `claude-haiku-4-5` and `anthropic`. The 5.5 rows are keyed by the gid of the current `prepare` output.
