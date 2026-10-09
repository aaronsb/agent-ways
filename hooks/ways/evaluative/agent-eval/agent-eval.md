---
description: an agent-eval evaluative loop for a multi-turn, tool-using agent, judged on the end state it leaves and its transcript across repeated trials; tasks with reference solutions, isolated trials, code graders first and calibrated model graders where needed, pass@k or pass^k, capability versus regression suites
vocabulary: agent sometimes finishes the ticket sometimes wanders off how often agent eval agent evals eval suite trial trials grader graders code grader model grader human grader llm judge grader calibration transcript transcripts end state graded reference solution pass@k pass^k capability eval regression eval saturated saturation graduate reward hacking grader loophole broken task benchmark infrastructure noise
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Agent-Eval Loop

The subject is an agent: a model that takes many turns, calls tools, and changes an environment. One answer is not the unit. A **task** has defined inputs and success criteria. Each attempt at it is a **trial**. The **transcript** records the trial: outputs, tool calls, intermediate results. The **outcome** is the state of the environment when the trial ends. **Graders** score aspects of a trial. `evaluative/rubric` scores a single answer; this loop scores whole runs.

## Grade the outcome, and the transcript beside it

Grade what the agent produced: the file written, the record changed, the test suite passing in the environment afterwards. A grader that requires a fixed sequence of steps fails agents that found a different or better route. Grading the transcript alongside the outcome checks is often useful too, for how the agent got there as well as where it ended. This corpus's practice adds hard checks on the path where a constraint applies, such as a forbidden tool call or a cost budget.

## Every task has a reference solution

A known working output that passes all graders proves the task is solvable and the graders accept a correct answer. With frontier models, a 0% pass rate across many trials (0% pass@100) is most often a sign of a broken task (ambiguous, impossible, or graded wrongly), not of an incapable agent. Ambiguity in the task statement shows up as noise in the scores.

## Isolate each trial, and record its configuration

Start every trial from a clean environment. Shared state between trials causes correlated failures from the infrastructure, and leaks answers: an agent can read what an earlier trial left behind, such as version-control history or cached files.

Record with each trial run the model and version under test and the resource configuration. CPU and memory allocation alone have moved agentic coding scores by several percentage points; time limits can also matter in some configurations. A difference between two runs below about 3 percentage points deserves skepticism until both configurations are documented and matched.

## Graders, in order of preference

- **Code graders** wherever the criterion can be checked deterministically: reproducible and fast, but brittle to valid variation. This corpus's practice is to compare numbers with a tolerance and normalise formatting, so a correct answer is not rejected for a spurious difference.
- **Model graders** where judgement is needed. Give each a clear rubric and one dimension to judge, let it reason before it scores, and let it answer "unknown". Allow partial credit on tasks with multiple components. Prefer a different model from the one being evaluated. Calibrate it against human grades on a sample before trusting it at scale.
- **Human grading** is the reference for calibrating model graders, too slow and costly for every trial.

Make graders hard to game: passing should require solving the task, not exploiting a loophole in how it is checked. Test both sides: cases where a behaviour should happen and cases where it should not.

## Repeat trials, and pick the statistic

Agents vary run to run, so run several trials per task. Report **pass@k** (at least one of k trials succeeds) when one good attempt is what the product needs, and **pass^k** (all k succeed) when every attempt must work. pass^k falls as k grows. This corpus's practice is to report the number of tasks and trials with every score.

## Read the transcripts

Graders are verified by reading transcripts and grades from many trials. A failure should look fair: it is clear what the agent got wrong. A grader that fails correct runs is fixed in the grader. A pass reached through a loophole is fixed in the task or grader. Each such fix is a change to the judge, made and reported on its own (see `evaluative`).

## Two suites

- **Capability suite:** what the agent cannot yet do reliably. Low pass rates are expected.
- **Regression suite:** what it already does. Pass rates near 100%; a drop is a defect.

A capability task the agent now passes reliably graduates to the regression suite. A suite at 100% still tracks regressions but gives no signal for improvement. Grow the suite from real failures: a few dozen tasks drawn from them is a useful start, and user-reported failures are converted into test cases.

## See Also

- evaluative(evaluative) — parent: author and judge, the shared core, choosing the loop
- evaluative/rubric(evaluative) — scoring a single model answer
- evaluative/clean-room(evaluative) — a fresh environment per run
- subagents(meta) — a subagent's report is a claim to grade
