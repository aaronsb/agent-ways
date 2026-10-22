---
description: building graders for an agent eval suite that the agent cannot game, and gating prompt and model changes on their scores; graders kept out of the agent's reach, a grader selftest before its scores count, code checks tried before a judge, fail-to-pass and pass-to-pass test lists, partial credit from key-point coverage, and a small regression suite that gates prompt, model and pipeline changes in CI
vocabulary: grader selftest empty answer fails tampered tests stubbed grader opened answer file pinned container key points coverage composite every part passes keyword check regex before a judge bleu perplexity off-the-shelf metric regression gate ci gate pass threshold production baseline new model release production failure sampled live traffic production monitoring
pattern: \bfail_?to_?pass\b|\bpass_?to_?pass\b
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Grading Agent Trials

`evaluative/agent-eval` says what to grade: the end state and the transcript, over repeated trials. This way keeps the grade honest, and uses it to gate changes.

## When it applies

An agent eval's scores are about to decide something, such as keeping a change, merging it, or switching models, and nobody has checked that the graders reject a wrong answer or that the agent could not reach them.

## A grader proves itself first

- A grader passes a selftest before its scores count. The reference answer passes. An empty answer, an "I don't know", and an answer to a different question each fail. Rerun it after each grader change, and store its runs apart from real ones.
- Many failures that look subjective yield to a keyword check or a regex, so try code before a judge. A model judge follows `evaluative/rubric/judge`.
- For a coding task, keep two test lists: FAIL_TO_PASS tests must start passing, and PASS_TO_PASS tests must keep passing. Run both in a pinned container.
- Generic metrics do not gate. BLEU, perplexity, and similar off-the-shelf scores say little about the task. Similarity metrics still help for retrieval.

## Keep the graders out of reach

Assume the agent will exploit any grader it can see. Scoring code, reference answers, and test files sit where the agent cannot read or edit them. Check transcripts for edited tests, stubbed graders, and opened answer files.

## Partial credit and composites

- Split each reference answer into fixed key points, and grade each key point present or absent. Partial credit is the coverage across key points. State the pass rule, such as coverage of at least 0.6.
- A composite passes only when every part passes.
- A grader that throws an exception has not graded the trial. Record it, rerun it, and count it in the report, outside the pass rate. A crash or timeout the agent caused is a fail. The rule for each failure class is in `evaluative/statistics`.

## The regression gate

A separate small suite, mostly deterministic, gates every prompt, model, or pipeline change in CI. Set its pass threshold explicitly against the production baseline. Some eval tools default to 1.0, which fails any imperfect case. Gate a user-facing agent on pass^k, and attach the results to the pull request. Add each new production failure to the suite, and rerun the full suite on each new model release before switching. Run expensive judges on sampled live traffic instead of in CI. Offline evals catch only part of what breaks, so pair them with production monitoring, user feedback, and transcript review.

## See Also

- evaluative/agent-eval(evaluative) — parent: tasks, trials, reference solutions, the two suites
- evaluative/rubric/judge(evaluative) — a model as judge, validated against human labels
- evaluative/statistics(evaluative) — what a pass rate counts, and its interval
- evaluative/ablation/rounds(evaluative) — a grader fix is its own round
