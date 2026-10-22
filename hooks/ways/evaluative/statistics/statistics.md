---
description: the statistics behind any eval score over a set of cases or repeated trials; what a pass rate counts when a run crashes, times out or hits a rate limit, standard errors and 95% intervals, clustered errors, paired tests between variants, interval width by set size, the unbiased pass@k and pass^k estimators, a harness hash so only like runs are compared, and control for provider drift
vocabulary: error bars standard error confidence interval 95% interval clustered standard errors paired test mean difference correlation between variants power analysis set size interval width unsettled gap unbiased estimator infrastructure failure rate limited usage limit api error harness crash grader exception kept out of the pass rate agent timeout counts as a fail errors log failure class refusals truncated answer harness hash run lineage dataset version interleave variants same-day rerun provider drift dead metric score against cost judge cost answer cost
pattern: \berror bars\b|\bpower analysis\b|\bclustered (standard )?errors\b|\bpaired (t.?)?test\b|\bunbiased (pass@k )?estimator\b|\bharness.?(sha|hash)\b|\bdead metric\b
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Eval Statistics

A score over a set of cases or a run of trials is an estimate. This way holds for every loop type that reports one: probe-set metrics, rubric hit rates, agent-eval pass rates, ablation arms, and tuning rounds.

## When it applies

An eval score is about to be reported or compared, such as one variant at 68% against another at 62%, with no interval, no count of the runs that failed for reasons outside the subject, and no check that both ran on the same harness.

## What a pass rate counts

- Infrastructure failures are not results: a rate or usage limit, an API error, a harness crash, a grader exception or refusal, a grade served by a model other than the one requested. Record each one with its class, rerun it, and count it in the report. Keep it out of pass rates and out of cost and latency medians. A rate-limited run often scores zero, and scored as a wrong answer it reads as a regression.
- Timeouts and crashes the agent caused are fails. The subject ran and did not finish.
- A truncated answer is classed by its cause. A harness output cap is an infrastructure failure. The subject running out its own budget is a fail.
- Judgements are binary for each criterion or key point. Partial credit comes from coverage across key points (`evaluative/agent-eval/grading`).
- Keep the subject's refusals as their own metric beside the pass rate.

## Repeated trials

A model run is one sample, so the counting rules of `code/testing/evaluation` apply, with these additions.

- Refine the setup on two or three cases, then run one slice, before the full matrix.
- Run each case several times, and score the case by the mean of its trials. Confirm any single-run change at the full trial count.
- With n trials and c successes per task, pass@k is the mean of 1 - C(n-c,k)/C(n,k), and pass^k is the mean of C(c,k)/C(n,k). The shortcut 1 - (1-p)^k is biased. The two diverge fast: a 75% per-trial rate gives a pass^3 of about 42%.

## Error bars

- Report each score with its standard error and a 95% interval: the mean plus or minus 1.96 standard errors.
- Cluster the standard errors when cases come in groups, such as several questions about one document. Clustered errors can be several times the naive ones.
- Compare variants with a paired test on the same cases. Report the mean difference, its interval, and the correlation between the variants.
- Choose the set size with a power analysis before the run.
- Near a 50% pass rate, the 95% interval is about plus or minus 14 points at 50 cases, 10 at 100, and 5 at 400. That is arithmetic from the standard error of a proportion. A smaller gap is unsettled. A finer metric, such as key-point coverage, narrows it.
- When a model judge grades the set, correct the pass rate for judge error first (`evaluative/rubric/judge`).

## Compare like with like

- Hash every file that affects a score into one harness hash, and compare only runs with the same hash. Record each run's dataset version, prompt version, model configuration, and judge settings.
- Interleave variants (A, B, A, B), on reruns too. Before blaming a change, rerun the unchanged version the same day, because the model provider can drift too.
- Record tool calls, tokens, elapsed time, answer cost, and judge cost for each trial, with its date. Keep judge cost apart from answer cost, and label estimates as estimates.
- When a change moves several metrics, report all of them, cost and latency included, and plot the score against cost for each variant.
- A metric that never moves across rounds is dead. Check that its grader measures what its rule says, then fix or drop it before the next round.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "It went from 62% to 68%, so it helped." | On 50 cases that gap is inside the interval. Call it unsettled. |
| "The eval score dropped, so the change regressed." | Check the errors log first. A rate-limited run scores zero too. |
| "The metric held steady, so nothing broke." | Check that it can move at all. A dead metric holds steady too. |
| "The numbers tell the story." | Read the failures (`evaluative/held-out/cases`). Otherwise you do not know whether the graders work. |

## See Also

- evaluative(evaluative) — parent: author and judge, the shared core, choosing the loop
- evaluative/agent-eval/grading(evaluative) — key-point coverage and the grader selftest
- evaluative/rubric/judge(evaluative) — judge error rates and the corrected pass rate
- evaluative/held-out/cases(evaluative) — reading failures before the score
- code/testing/evaluation(softwaredev) — a pass count carries its denominator
