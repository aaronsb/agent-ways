---
contract: adr/v1
kind: evidence
capability: matching
status: proposed
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
