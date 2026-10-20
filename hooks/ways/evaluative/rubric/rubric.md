---
description: a rubric evaluative loop for a language model's non-deterministic answer, where wording changes every run; score each answer against a rubric of expected points with a hit threshold alongside deterministic checks, and test each rubric item against a known-good and a known-bad answer; an LLM judge gets one criterion per call, PASS and FAIL conditions, critique before verdict, and validation against human labels by true positive and true negative rate
vocabulary: grade llm summaries generated text quality language model answer reply wording phrasing varies every run different answer each time non-deterministic stochastic exact string match flaky just skim a few outputs rubric rubric item hit miss threshold expected points known-good known-bad answer live model call api key tokens sampling temperature variance non-determinism exact-match quoting single-quoted model-written llm judge llm-as-judge judge prompt judge model pass fail critique verdict pairwise position bias human labels tpr tnr dev test split kappa
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Rubric Loop

The subject is a model's answer: it varies between runs, and the same intent can be phrased many ways. The loop runs each scenario and judges the result in two layers: deterministic checks on what the run did, and a rubric on what the answer said. A rubric scores one output. A multi-turn agent judged on the path it took and the state it left is `evaluative/agent-eval`.

## When it applies

Text a language model writes, such as summaries, descriptions, replies or release notes, comes out differently each run, and quality is judged by reading a few outputs. An exact-match test fails on every run, so nothing checks the rest.

## A scenario

A scenario is a fixed setup (a project directory, seeded files, a prompt, sometimes a second turn) and a check script. The run is the real one: the model, and the tools and configuration a user would have.

- **Deterministic checks first.** Did the run exit cleanly, did the expected context load, was one commit made, is the working tree clean, does the file the run wrote pass its linter. These do not depend on phrasing.
- **A rubric for the answer.** Each item is a pattern the answer should contain ("names the decision kind", "puts the question to the operator"). The scenario passes when at least a stated number of items hit. The threshold allows phrasing to vary without letting an answer that misses the point through.

## Test the judge

The rubric is an instrument and fails the way instruments fail.

- **Every check script must parse.** A script the shell cannot parse stops partway when it is sourced. The checks after the error never run, and the scenario reports what ran as a full pass. The runner parses each script before running it and fails the scenario when it does not parse.
- **Each rubric item is tested against a known-good and a known-bad answer.** The good answer hits, the bad one misses. A pattern that hits both is measuring nothing. One that misses the good answer will fail correct runs.
- **Quoting is part of the pattern.** Characters the shell interprets inside a quoted pattern change what the pattern is. Prefer single-quoted patterns.
- **A model used as a judge** is calibrated before its verdicts count (below, and `evaluative/agent-eval`).

## A model as judge

Give each criterion its own judge call. A judge that scores several criteria at once lets one skew another. Keep judges on short outputs, and check a long output, such as a generated file, with code.

The judge prompt has four parts:

1. The task and the one criterion.
2. Exact PASS and FAIL conditions, taken from failures seen in real outputs.
3. Two to four labelled examples: a clear pass, a clear fail, and a borderline case.
4. Structured output with the critique before the verdict, such as `{critique, result}`.

Tell the judge to return Unknown when the information to decide is missing. The candidate sits inside a tag, and the judge treats it as data, never as instructions. The judge does not reward length, and it marks a plausible reading of the source as partial, not as a contradiction. Give it the source when the verdict depends on it. A judge that never sees the source can score a wrong reference as a model failure.

Ask for binary PASS or FAIL with a written critique, and split a graded quality into several binary checks. If a scale is unavoidable, label every anchor. With thinking off, ask for the reasoning in one tag and the verdict in another, and parse only the verdict.

Name the judge model on every run, and prefer one that differs from the model under test. Some eval tools default to a small judge model. Check which model served each grade, and fail the grade when it is not the one requested.

For a subjective criterion such as tone, judge each pair in both orders. Count a win only when both orders agree, and a tie when they disagree. Check that a padded answer does not win.

## Validate the judge against human labels

- One domain expert owns the labels. Without one, two labellers each label 20 to 50 outputs and settle their disagreements first.
- Label about 100 cases per failure mode, near half pass and half fail.
- Split the labels into train (10 to 20%), dev (40 to 45%), and test (40 to 45%). Prompt examples come from train only.
- Report the true positive rate and the true negative rate separately. Aim above 90% on dev, with 80% as the floor. Tune on dev, and run test once at the end.
- Use kappa only for agreement between two human labellers.
- Recheck on fresh labels on a schedule, and after any change to the judge model or prompt.

| Rationalization | Counter |
|---|---|
| "The judge agrees with me 95% of the time." | When failures are 5% of cases, a judge that always passes agrees 95% of the time. Report both rates. |
| "A 1 to 5 score gives more detail." | Nobody can act on a 3 versus a 4. Split it into binary checks. |
| "The judge model is strong enough to grade itself." | Models favour their own outputs. Check against human labels. |

## Variance and cost

A single run is one sample. When a scenario flips between runs, run it several times and report the hit rate with its count, then tighten either the scenario or the rubric. Live runs spend tokens. Run the suite when a change alters what the model sees or does, and record which scenarios ran.

## See Also

- evaluative(evaluative) — parent: author and judge, the shared core, choosing the loop
- code/testing/evaluation(softwaredev) — pass counts carry their denominator
- code/security/injection/prompt(softwaredev) — scenario inputs are data to the model under test
