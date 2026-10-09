---
description: a rubric evaluative loop for a language model's non-deterministic answer, where wording changes every run; score each answer against a rubric of expected points with a hit threshold alongside deterministic checks, and test each rubric item against a known-good and a known-bad answer
vocabulary: grade llm summaries generated text quality language model answer reply wording phrasing varies every run different answer each time non-deterministic stochastic exact string match flaky just skim a few outputs rubric rubric item hit miss threshold expected points known-good known-bad answer live model call api key tokens sampling temperature
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
- **A model used as a judge** is calibrated before its verdicts count (see `evaluative/agent-eval`).

## Variance and cost

A single run is one sample. When a scenario flips between runs, run it several times and report the hit rate with its count, then tighten either the scenario or the rubric. Live runs spend tokens. Run the suite when a change alters what the model sees or does, and record which scenarios ran.

## See Also

- evaluative(evaluative) — parent: author and judge, the shared core, choosing the loop
- code/testing/evaluation(softwaredev) — pass counts carry their denominator
- code/security/injection/prompt(softwaredev) — scenario inputs are data to the model under test
