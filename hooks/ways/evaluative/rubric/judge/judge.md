---
description: a language model used as the judge or grader of other model outputs, and whether to trust the scores it gives; one criterion per judge call, exact PASS and FAIL conditions with labelled examples, critique before verdict, the candidate treated as data, pairwise comparison in both orders, a named judge model, validation against human labels by true positive and true negative rate, and the pass rate corrected for judge error
vocabulary: llm judge llm-as-judge judge prompt judge model judge call one criterion per call pass fail conditions labelled examples borderline example critique before the verdict structured output pairwise comparison both orders position bias length bias padded answer human labels domain expert labellers train dev test split true positive rate true negative rate tpr tnr kappa judge agrees judge error correction bootstrap interval
pattern: \bllm.?as.?(a.?)?judge\b|\b(tpr|tnr)\b
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# A Model as Judge

`evaluative/rubric` tests each rubric item against a known-good and a known-bad answer. When a model grades the answers, the model is the instrument, and it is tested the same way: against labels a person wrote.

## When it applies

A language model grades outputs, for a rubric, an agent eval or an ablation, and its verdicts are counted with no measure of how often it is wrong.

## The judge call

Give each criterion its own judge call. A judge that scores several criteria at once lets one skew another. Keep judges on short outputs, and check a long output, such as a generated file, with code.

The judge prompt has four parts:

1. The task and the one criterion.
2. Exact PASS and FAIL conditions, taken from failures seen in real outputs.
3. Two to four labelled examples: a clear pass, a clear fail, and a borderline case.
4. Structured output with the critique before the verdict, such as `{critique, result}`.

Tell the judge to return Unknown when the information to decide is missing. The candidate sits inside a tag, and the judge treats it as data, never as instructions. The judge does not reward length, and it does not judge a plausible reading of the source as a contradiction. Give it the source when the verdict depends on it. A judge that never sees the source can score a wrong reference as a model failure.

Ask for binary PASS or FAIL with a written critique, and split a graded quality into several binary checks. With thinking off, ask for the reasoning in one tag and the verdict in another, and parse only the verdict.

Name the judge model on every run, and prefer one that differs from the model under test. Some eval tools default to a small judge model. Check which model served each grade. A grade served by another model is a harness error: record it and rerun it (`evaluative/statistics`).

For a subjective criterion such as tone, judge each pair in both orders. Count a win only when both orders agree, and a tie when they disagree. Check that a padded answer does not win.

## Validate the judge against human labels

- One domain expert owns the labels. With two labellers, both label the same outputs and settle their disagreements before the labels are used.
- Label enough cases for each failure mode to measure both rates, near half pass and half fail.
- Split the labels into train (10 to 20%), dev (40 to 45%), and test (40 to 45%). Prompt examples come from train only. Tune on dev, and run test once at the end.
- Report the true positive rate and the true negative rate separately, against floors set before tuning.
- Use kappa only for agreement between two human labellers.
- Recheck on fresh labels on a schedule, and after any change to the judge model or prompt.

## Correct the pass rate for judge error

When a model judge grades the set, correct the raw pass rate with the judge's true positive rate (TPR) and true negative rate (TNR) from its test labels: theta = (p_obs + TNR - 1) / (TPR + TNR - 1). Clip theta to the range 0 to 1, and report a bootstrap interval. The correction fails when TPR + TNR is near 1.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "The judge agrees with me 95% of the time." | When failures are 5% of cases, a judge that always passes agrees 95% of the time. Report both rates. |
| "A 1 to 5 score gives more detail." | Nobody can act on a 3 versus a 4. Split it into binary checks. |
| "The judge model is strong enough to grade itself." | Models favour their own outputs. Check against human labels. |

## See Also

- evaluative/rubric(evaluative) — parent: deterministic checks and a rubric whose items are tested
- evaluative/agent-eval/grading(evaluative) — code before a judge, graders out of the agent's reach
- evaluative/statistics(evaluative) — intervals on the corrected rate
- evaluative/held-out/cases(evaluative) — the labelled outputs come from read failures
