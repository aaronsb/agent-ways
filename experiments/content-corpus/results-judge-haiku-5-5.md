# Relevance judge threshold on Claude Haiku 5.5: spot check

Question: does the judge's 0.3 pass threshold (`THRESHOLD` in `judge_ab.py`, measured on Claude Haiku 4.5) still hold on Haiku 5.5? This is a spot check on 50 golden groups, not a re-measurement.

## Method

- Set: golden, arm A (today's `way_text`: route, then description), as in `results-judge-ab.md`.
  Candidates are the alias-cosine top 3 (targeted prompt) or top 2 (`none` prompt).
- Sample: `--sample 50`, seeded draw (seed 20261008), stratified. All 18 `none` groups (the pool holds only 18) plus 32 targeted groups, round-robin over (candidate count, expected way in candidates or not). 132 candidates: 16 strict-relevant, 116 irrelevant. 60 groups was the ask; the projected cost of 60 was $0.0110 typical, over the $0.01 bound, so the sample was cut to 50 (projected $0.0091 typical).
- Baseline: the cached `claude-haiku-4-5` arm-A rows for the same 50 groups, not re-run. The cache's gid numbering predates the current golden export, so rows are paired by the Anthropic request hash (identical prompt, candidates and order), which matched all 50.
- 5.5 request: `--provider openrouter --model anthropic/claude-haiku-5.5`, built as `net.rs` builds the OpenRouter body: chat completions, system message, `record_judgements` as a function tool with `tool_choice` forced, `max_tokens` 96 + 64n, no `temperature` (`accepts_sampling` is false for Haiku 5.5). One attempt per group, no retries, sequential.
- Bounds: at most 60 calls and $0.01. Run with `--cap 0.01 --max-calls 50`; the loop stops before a call that could cross the cap.
- Estimate: Anthropic `count_tokens` on the 4.5-tokenizer body, scaled by 1.3 for the 5.5 tokenizer, priced at the Haiku 5.5 list price ($0.10 / $0.50 per MTok, `cost.rs`).
- Relevance labels: `strict` is the expected way; `family` is the expected way, its parent or a child, as defined in `judge_ab.py`.

## Results

Actual: 50 judge calls, $0.00758 (provider-reported OpenRouter cost), 0 request failures (all HTTP 200, all answers parsed, every candidate judged). Median latency 1545 ms (max 2565 ms); the cached 4.5 median for the same groups is 951 ms.

Strict labels (16 relevant, 116 irrelevant candidates):

| | Haiku 4.5 | Haiku 5.5 |
|---|---|---|
| AUC | 0.985 | 0.982 |
| irrelevant rejected at 0.3 | 94 / 116 (81.0%) | 76 / 116 (65.5%) |
| irrelevant passed at 0.3 | 22 / 116 (19.0%) | 40 / 116 (34.5%) |
| relevant passed at 0.3 | 16 / 16 | 16 / 16 |
| P(yes) relevant, median [min] | 0.95 [0.92] | 0.93 [0.80] |
| P(yes) irrelevant, median / p75 | 0.10 / 0.15 | 0.15 / 0.65 |

Family labels (20 relevant, 112 irrelevant): AUC 0.959 (4.5) vs 0.951 (5.5); at 0.3, 4.5 rejects 92 / 112 and loses 2 / 20 relevant, 5.5 rejects 76 / 112 and loses 0 / 20.

5.5 pass rate on irrelevant candidates and recall on relevant ones by threshold (strict):

| threshold | 0.3 | 0.5 | 0.7 | 0.75 | 0.8 | 0.9 |
|---|---|---|---|---|---|---|
| irrelevant passed | 34.5% | 30.2% | 24.1% | at or under 19.0% | 12.9% | 4.3% |
| recall | 100% | 100% | 100% | 100% | 100% | 87.5% |

Matching 4.5's operating point at 0.3 (19.0% of irrelevant passed, 100% recall): any threshold from 0.75 to 0.80 on 5.5 does it. On family labels (4.5: 17.9% passed, 90% recall) the match needs about 0.70 to 0.75, and no single value meets both exactly.

## Reading

Ranking quality is unchanged: AUC differs by 0.003 (strict) and 0.008 (family), well inside what 16 relevant candidates can resolve. What moved is the scale. 5.5 puts more irrelevant candidates in the 0.3 to 0.75 band that 4.5 leaves nearly empty (irrelevant p75 is 0.65 on 5.5 against 0.15 on 4.5), so a 0.3 threshold passes 18 more irrelevant candidates than on 4.5 with no recall gain. Relevant candidates stay at 0.80 and above on 5.5, so there is room to raise the threshold.

## Verdict

0.3 does not hold on Haiku 5.5 as a like-for-like operating point: it lets through 34.5% of irrelevant candidates against 19.0% on 4.5, at the same 100% recall on this sample. The data points to about 0.75 (0.75 to 0.80 matched 4.5 on both axes on strict labels; the family labels want 0.70 to 0.75). Caveat: this is 50 groups with 16 strict-relevant candidates, so recall is only bounded loosely (a threshold of 0.8 leaves the weakest relevant candidate at 0.80 with no margin), and the threshold should be treated as an estimate to confirm on the full 332-group golden set before changing the shipped profile.

## Reproduce

```
WAYS_BIN=tools/target/release/ways python3 judge_ab.py prepare
python3 judge_ab.py estimate --sample 50 --arm A
python3 judge_ab.py run --arm A --provider openrouter --model anthropic/claude-haiku-5.5 --sample 50 --cap 0.01 --max-calls 50
python3 judge_ab.py compare --model anthropic/claude-haiku-5.5 --provider openrouter --sample 50
```

The new rows in `judge-ab-calls.jsonl` carry `model` and `provider`; older rows default to `claude-haiku-4-5` / `anthropic`. `gid` is the group's index in the current `prepare` output.
