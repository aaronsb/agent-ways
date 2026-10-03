# Yes/no relevance gate probe

A probe, run 2026-09-30 to 2026-10-01, of one question: can a model asked "is this way relevant to the conversation's most recent turns? yes or no" tell the relevant way injections from the rest, quickly enough to sit in a hook? The decision record that cites it is ADR-195 (evidence); the decisions built on it are ADR-196 (the gate) and ADR-502 (the daemon). The gate as shipped is explained in [the relevance judge](../../explanation/relevance-judge/relevance-judge-the-model.md).

## Answer

- The current matcher injects an off-topic way about nine times in ten. On 40 fires drawn at random from live telemetry, 4 were relevant (10%, Wilson 95% interval 4–23%). One fully labelled working session agreed: 4 of 29 prompt-lane and about 20 of 159 tool-lane injections were relevant. The matcher's own fire score separates relevant from irrelevant at AUC 0.53 on the random sample.
- **Claude Haiku 4.5 discriminates well.** With the way's path and summary against the last turn, AUC 0.94 on both the main set and the random sample, against 0.78 for a word-overlap baseline. At threshold 0.3 it blocks 58% of irrelevant real fires while losing 2% of relevant ones; with about 4,000 characters of context plus a session gist, it blocks 83% at 10% lost. Its answers did not change across repeats or rewording. It costs about 0.8 s per call and roughly $0.0007 per call at current prices.
- **A local 0.6B reranker (Qwen3-Reranker-0.6B) does not beat word overlap,** and on the random live sample it is a coin flip (AUC 0.49). It runs at about 140 ms per way warm on 16 CPU threads, but at that load it heats the machine. It is shelved, not ruled out (see Local model, below).

## Data

Labelled data, scores and per-item results hold private session text. They stay at `~/.local/state/agent-ways/probes/yesno-gate/` on the machine that ran the probe, mode 0700, and are never committed.

| Set | Size | What it is |
|---|---|---|
| Main (`data/eval.jsonl`) | 168 | Real prompt-lane fires judged relevant (48) and irrelevant (52); look-alike pairings (30); unrelated pairings (15); relevant ways that did not fire (16); ambiguous (7). Each item carries the last four turns, about 4,000 characters of turns, and a one-line session gist (the session's generated title, or its first prompt). |
| Random (`data/random40.jsonl`) | 40 | Fires drawn uniformly (seed 20260930) from 661 verified prompt-lane fires |
| Session (`data/session_unlabelled.jsonl`) | 189 | Every way injection in one long working session, all lanes, with the trigger excerpt |

Turns contain only what the user typed and the assistant's prose; injected guidance, tool calls and results, reminders and peer messages are removed. Labels were written by one model; the session set was labelled again, blind, by the main session's model (agreement 171/189, Cohen's kappa 0.59), and the operator labelled four items, agreeing with all four once one item was shown with wider context. A kappa of 0.59 between careful labellers is the ceiling any judge can be held to.

## Method

- **Judges.** Qwen3-Reranker-0.6B, with P(yes) as the softmax over its yes/no logits, through llama.cpp at the repository's pinned commit (agrees with the transformers reference within 0.005 over 144 items, no answer flips). Claude Haiku 4.5 at temperature 0, with a forced structured answer `{relevant, confidence}`.
- **Way text.** A: the way's description. B: its path through the ways tree, then the description. C: the path with each ancestor's description, then the description (six nodes had no description; drafts were used).
- **Context.** The last turn; the last four turns; about 4,000 characters of turns; the same plus the session gist.
- **Controls.** Shuffled labels; token-Jaccard overlap between the context and the way's description and vocabulary; the matcher's fire score joined from telemetry.
- **Metrics.** AUC; noise removed (irrelevant fires blocked) against relevant lost (relevant fires blocked) across thresholds; per-category accuracy; calibration by decile; latency. Intervals are bootstrap 95% (1,000 resamples).

## Results

**Discrimination (AUC)**

| Judge, input | Main set, no ambiguous | Random sample |
|---|---|---|
| Haiku, B + last turn | 0.938 | 0.941 |
| Haiku, B + ~4,000 chars + gist | 0.921 | not run |
| Haiku, A + four turns | 0.892 | 0.878 |
| Local, B + last turn | 0.841 | 0.493 |
| Local, A + four turns | 0.805 | 0.701 |
| Word overlap (Jaccard) | 0.778 | 0.30 |
| Matcher fire score | 0.70 (n=76) | 0.53 |
| Shuffled labels | 0.50 | 0.50 |

Paired against local on the same items, Haiku is ahead by 0.087–0.097 AUC, with intervals clear of zero.

**Operating points** (noise removed / relevant lost, %, real fires of the main set)

| Judge, input, threshold | Main set | Random sample |
|---|---|---|
| Haiku, B + last turn, 0.3 | 58 [43–71] / 2 [0–7] | 86 [74–97] / 0 |
| Haiku, B + ~4,000 chars + gist, 0.3 | 83 [72–93] / 10 [2–19] | not run |
| Local, A + four turns, 0.002 | 67 [54–80] / 23 [11–36] | 67 / 25 |

The random sample has only 4 relevant fires, so its relevant-lost figures are poorly bounded.

**Inputs.** The path (B) was best or tied for both judges. Ancestor descriptions (C) never beat the summary alone. Longer context did not raise AUC above four turns; with Haiku the long context plus gist removed the most noise at a higher relevant loss.

**Stability.** Five repeats of ten identical inputs: no flips, no spread, for every judge. Rewording the instruction: no flips; mean P(yes) shift 0.006 (Haiku), 0.009 (local). Haiku's confidence takes a handful of distinct values, so each engine needs its own threshold.

**Latency** (this machine, CPU)

| Judge | Per way, p50 | Notes |
|---|---|---|
| Local, last turn, 16 threads | 136 ms | load 0.42 s; one-shot process 2.3 s for six ways |
| Local, four turns, 8 threads | 540–640 ms | |
| Local, ~4,000 chars, 8 threads | about 1.5 s | |
| Haiku | 720–820 ms | about 2.2 s for six concurrent calls |

Six ways in one llama.cpp request were no faster than six requests. The ROCm build of llama.cpp segfaulted at load and was not measured.

## Significance, batching and agreement

Added 2026-10-01; the numbers and their method are in ADR-195's addendum.

- At threshold 0.3, across the 140 real fires of sets 1 and 2, precision of what is injected goes from 0.37 to 0.65 with one call per way and to 0.67 with one call per prompt (p < 0.0001 for each, paired bootstrap).
- One call per prompt, the form the gate uses, discriminates as well as one call per way (AUC 0.936 against 0.924 on set 1) and costs 1.0 s for three or four candidates.
- On set 3, judged from the trigger excerpt alone, Haiku's agreement with careful labels is kappa 0.26 against a 0.59 ceiling between labellers, and no better than chance on the 29 prompt-lane items. The excerpt is a weaker input than the conversation turns the gate sends; live `way_judged` events measure the deployed gate.

## Local model

Why the 0.6B reranker failed, as inferred from the results:

- The gate only sees candidates the matcher already picked, so every candidate is topically plausible. What is left to decide is whether the conversation is doing the work or only touching its words. A search reranker answers the topical question, the one already answered, which is why it lands on word overlap and drops to chance on random live fires.
- At 0.6B it follows the instruction weakly; rewording barely moved its scores.
- Its inputs are out of distribution: a block of conversation as the query, a keyword-list description as the document.
- Its scores saturate near 0 and 1, leaving no graded range for a threshold.

A local engine is worth revisiting with a model that reasons about the conversation rather than ranks documents: a small instruction model asked the yes/no question directly, or a model fine-tuned on yes/no labels like these. It must beat the word-overlap control on the random sample and stay within a CPU budget that does not heat the machine under live bursts. Tracked in the issue linked from ADR-196.

## Scripts

`scripts/` holds the probe code; none of it embeds session text. `judge/judge.py` is the judge interface (local llama.cpp or transformers, or Haiku), `judge/bench.py` and `judge/stability.py` the benchmarks, `data/` builds the labelled sets from telemetry and transcripts, and `eval/` scores and analyses them. The Haiku scorer reads its key from `WAYS_JUDGE_KEY_FILE`, defaulting to `~/.config/agent-ways/keys/anthropic`. The scripts expect the data directory above and a llama.cpp build from the `tools/way-embed/llama.cpp` submodule.
