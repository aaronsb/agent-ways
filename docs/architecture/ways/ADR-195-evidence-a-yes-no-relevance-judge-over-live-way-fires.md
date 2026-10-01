---
contract: adr/v1
kind: evidence
capability: matching
status: accepted
date: 2026-10-01
deciders:
  - aaronsb
related:
  - ADR-160
  - ADR-193
  - ADR-194
---

# ADR-195: Evidence: a yes/no relevance judge over live way fires

## Summary

A probe measured how often injected ways are relevant, and whether a model asked "is this way relevant to the conversation's last turns? yes or no" can tell the relevant fires from the rest. The current matcher injects off-topic ways about nine times in ten. A hosted model (Claude Haiku 4.5) separates relevant from irrelevant fires clearly, ahead of a word-overlap baseline. A local 0.6B reranker (Qwen3-Reranker-0.6B) does not beat word overlap. The full report, with method and scripts, is `docs/research/yesno-relevance-gate/`. Data and labels stay private at `~/.local/state/agent-ways/probes/yesno-gate/` on the machine that ran the probe, because they hold session text.

## Method

- **Labelled sets.** (1) 168 items built from real prompt-lane fires and session transcripts: fires judged relevant (48) and irrelevant (52), look-alike pairings (30), unrelated pairings (15), relevant ways that did not fire (16), ambiguous (7). (2) 40 fires drawn uniformly at random from 661 verified prompt-lane fires. (3) Every way injection in one long working session, all lanes (189 rows), labelled blind by two judges. Turns contain only typed text and assistant prose, with injected guidance, tool output and reminders removed.
- **Labels.** One model labelled sets 1 and 2. The main session's model labelled set 3 a second time, blind, and agreed on 171 of 189 rows (Cohen's kappa 0.59). The operator labelled four items and agreed with all four after seeing the wider context of one of them.
- **Judges.** Qwen3-Reranker-0.6B, P(yes) from the yes/no token logits, through llama.cpp at the pinned commit (agrees with the transformers reference within 0.005 over 144 items). Claude Haiku 4.5 with a forced structured answer `{relevant, confidence}` at temperature 0.
- **Inputs.** Way text: A the summary, B the path through the ways tree plus the summary, C the path with each ancestor's description. Context: the last turn, the last four turns, about 4,000 characters, and about 4,000 characters plus a one-line session gist.
- **Controls.** Shuffled labels, token-Jaccard word overlap, and the matcher's own fire score joined from telemetry.

## Findings

**The current matcher.** Relevant share of injections: 4 of 40 on the random sample (10%, Wilson 95% 4–23%); 4 of 29 on the prompt lane and 19–23 of 159 on the tool lane in the labelled session. The matcher's fire score separates relevant from irrelevant at AUC 0.53 on the random sample. Much of the tool-lane noise comes from broad `commands:` patterns, e.g. supply-chain ways firing on `git log` and `gh issue create`.

**Discrimination (AUC).**

| Judge, input | Set 1 (no ambiguous) | Set 2 (random) |
|---|---|---|
| Haiku, B + last turn | 0.938 | 0.941 |
| Haiku, B + ~4,000 chars + gist | 0.921 | not run |
| Haiku, A + last four turns | 0.892 | 0.878 |
| Local, B + last turn | 0.841 | 0.493 |
| Local, A + last four turns | 0.805 | 0.701 |
| Word overlap (Jaccard) | 0.778 | 0.30 |
| Shuffled labels | 0.50 | 0.50 |

Haiku is ahead of the local model by about 0.09 AUC on matched configurations, outside the paired bootstrap intervals. The local model's interval contains the word-overlap baseline on set 1.

**Operating points** (noise removed / relevant lost, % with 95% intervals, real fires of set 1):

| Judge, input, threshold | Set 1 real fires | Set 2 random |
|---|---|---|
| Haiku, B + last turn, 0.3 | 58 [43–71] / 2 [0–7] | 86 [74–97] / 0 |
| Haiku, B + ~4,000 chars + gist, 0.3 | 83 [72–93] / 10 [2–19] | not run |
| Local, A + last four turns, 0.002 | 67 [54–80] / 23 [11–36] | 67 / 25 |

**Inputs.** The path through the ways tree (B) was best or tied for both judges. Ancestor descriptions (C, with drafted descriptions for six undescribed nodes) were never better than the summary alone. Longer context did not raise AUC above the last four turns; with Haiku it removed the most noise at a higher relevant loss.

**Stability.** Five repeats of ten identical inputs gave no answer flips and no spread for either judge. Rewording the instruction flipped no answers (mean P(yes) shift 0.006 for Haiku, 0.009 local). Haiku's confidence takes a handful of distinct values; local P(yes) is continuous but sits near 0 or 1, so its useful thresholds fall below 0.01.

**Latency** (CPU, this machine). Local, warm server: about 140 ms per way at 16 threads with one turn, 540–640 ms with four turns at 8 threads, about 1.5 s with 4,000 characters. Loading the model costs 0.4 s in llama.cpp; loading per call dominates a one-shot process (2.3 s for six ways). Six ways in one request were no faster than six requests. Haiku: 720–820 ms per call, about 2.2 s for six concurrent calls. The ROCm build of llama.cpp segfaults at load and was not measured.

## Limits

- Labels are model judgements, checked by a second model on set 3 and by the operator on four items. A kappa of 0.59 between careful labellers bounds what any judge can show.
- Set 2 holds only 4 relevant fires, so relevant-lost on the random sample is poorly bounded.
- Way summaries are from the corpus at probe time, not the version that fired.
- Only sessions from August to October 2026 still had transcripts.

## Addendum, 2026-10-01: significance, batching and agreement

Appended after acceptance; nothing above is changed. Computed from the same private data, plus 145 Haiku calls for the batched form.

**Significance.** Precision of the injected ways at threshold 0.3, Haiku with the way's path and description against the last turn. The interval and the one-sided p-value come from a paired bootstrap (10,000 resamples) of the precision difference; the bound on relevant ways lost is one-sided Clopper-Pearson at 95%.

| Fires | n | Precision before → after | Difference, 95% CI | p | Relevant lost (upper bound) |
|---|---|---|---|---|---|
| Set 1 real fires | 100 | 0.48 → 0.68 | +0.14 to +0.28 | < 0.0001 | 1 of 48 (≤ 10%) |
| Set 2 random live fires | 40 | 0.10 → 0.44 | +0.09 to +0.68 | 0.016 | 0 of 4 (≤ 53%) |
| Both | 140 | 0.37 → 0.65 | +0.22 to +0.36 | < 0.0001 | 1 of 52 (≤ 9%) |

With about 4,000 characters of context and the session gist, set 1 goes from 0.48 to 0.83, losing 5 of 48 relevant ways (≤ 21%).

**One call per prompt.** The gate sends every candidate of a prompt in one request. Run that way, grouping the same items by prompt (145 calls, 105 items sharing a prompt with at least one other), Haiku reached AUC 0.936 on set 1, against 0.924 for one call per way, and 0.973 against 0.950 on the items that shared a prompt. On set 2 it reached 0.889 against 0.941; set 2 has 4 relevant fires, so one item moving changes that figure. At 0.3, over both sets, precision went from 0.37 to 0.67 (95% CI of the difference +0.23 to +0.38, p < 0.0001), losing 3 of 52 relevant ways (≤ 14%). A call took 0.82 s with one candidate, 1.0 s with three or four, and 2.7 s with five; p95 over the run was 2.4 s, so a 2 s deadline fails open on about 5% of calls at that load.

**Agreement with careful labels** (set 3, 189 injections from one session, judged against the trigger excerpt rather than the conversation turns):

| Rater | Cohen's kappa against the main session's labels | Agreement |
|---|---|---|
| Second labeller | 0.59 | 0.90 |
| Haiku, path + description, 0.3 | 0.26 | 0.77 |
| Local reranker, 0.5 | 0.16 | 0.85 |
| Local reranker, 0.002 | 0.04 | 0.46 |

By lane, Haiku's kappa was 0.31 on the tool lane (159 injections, 23 relevant) and −0.02 on the prompt lane (29 injections, 4 relevant). With a trigger excerpt as its only context, Haiku agreed with careful labels well below the ceiling, and on the small prompt-lane sample no better than chance. The gate sends the conversation's turns, the input measured on sets 1 and 2; set 3 says the excerpt alone is not enough, and the live `way_judged` events are the measure of the gate as deployed.

## Addendum, 2026-10-01: request variants and latency

Appended after acceptance; nothing above is changed. Measured against the deployed gate (ways 1.25.0, ways-agent 0.1.0, Haiku 4.5) and the same private labels. Scripts: `docs/research/yesno-relevance-gate/scripts/eval/` (`gate_bench.py`, `latency_probe.py`, `score_variants.py`, `analyze_variants.py`).

**Latency follows candidate count.** Sent directly with the deployed request and no deadline, a call took 0.76 s with one candidate, 1.3 s with 6, 1.8 s with 10, 2.4 s with 15 and 2.5 s with 20, about 0.6 s plus 0.1 s per candidate. Output tokens grow with each candidate (55 at one, 435 at 20); input size barely matters. Through the hook, every deadline fallback in three runs (1, 4 and 12 prompts in parallel) was a prompt with 10 or more candidates. At 12 in parallel, above the 8-slot cap, no request fell back for a busy slot, and judge p50 moved from 1.2 s to 1.3 s. The hook adds about 65 ms to the judge call.

**Verdicts depend on the candidate set.** At temperature 0, five repeats of one set returned the same verdicts. The same way under the same prompt scored differently in different sets: `data/migrations/numbering` for "write a database migration that adds a nullable column" scored 0.15 to 0.25 beside `data` and `data/migrations`, 0.85 beside `data` alone, and 0.15 alone.

**Request variants.** Four requests scored the same 397 units in 238 prompt groups, one call per group: sets 1 and 2 with the last turn, and set 3 with its trigger excerpt. Intervals are a paired bootstrap over prompt groups.

| Variant | AUC | ΔAUC, 95% CI | Precision at 0.3 | Recall at 0.3 | p50 / p95 | Output tokens per candidate |
|---|---|---|---|---|---|---|
| Deployed: `relevant` + `confidence` | 0.920 | | 0.56 | 0.89 | 0.82 / 1.27 s | 46.6 |
| `p_relevant` alone | 0.925 | −0.015 to +0.026 | 0.54 | 0.95 | 0.80 / 1.38 s | 43.6 |
| Described fields in order: subject, match, relevant, confidence | 0.914 | −0.033 to +0.019 | 0.59 | 0.83 | 1.07 / 2.06 s | 65.2 |
| Way catalog in a cached system prompt, fixed 40-id schema | 0.927 | −0.023 to +0.033 | 0.71 | 0.89 | 0.91 / 1.32 s | 46.6 |

No variant ranks better than the deployed request. Scores sit near 0 or 1, so precision barely moves between thresholds 0.3 and 0.7, and the yes or no decides the gate.

- The catalog cut ways passed wrongly from 68 to 35 (precision +0.09 to +0.22) with recall unchanged. It read 7,926 cached tokens a call. Haiku 4.5 caches only a prefix of 4,096 tokens or more, so the deployed request, at about 1,000 tokens with a schema that changes with candidate count, cannot be cached.
- The described fields lost recall (−0.10 to −0.01) and put p95 past the 2 s deadline. Their `match` field said `none` for 167 units, 165 of them labelled not relevant.
- `p_relevant` alone saved 3 output tokens a candidate; the JSON keys and ids dominate the answer.

The operator declined the catalog: halving wrong passes is not worth about 40% more input tokens and 90 ms a call. The deployed request stands. The cost the gate carries is fallbacks on prompts with many candidates.
