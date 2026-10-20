---
description: the evaluation loop that decides a fix or a test is done; reproduce the reported failure in the reporter's environment, drive the path the real program takes, fix every review finding and re-review on the scenarios that found them, and state how many runs a flaky or intermittent test result rests on; for repeated model or agent trials, the unbiased pass@k and pass^k estimators, a harness hash, and harness errors kept apart from wrong answers
vocabulary: reproduce reproduced repro reporter reported environment real path real loop event loop stateful headless driver tick ticks redraw draw between keys calls hand directly test-only shortcut reviewer review finding findings re-review round rounds original scenario replay loop count runs looped repeat thousand times denominator failure rate once a week intermittent sometimes fails passed locally passed 50 times race call it fixed reproduced before fix confirm fixed flaky flake can't reproduce run-count unreproduced statistics trials per case pass@k pass^k unbiased estimator harness hash run lineage errors log rate limited truncated answers refusals judge cost headless runs
pattern: \bre-?review(ed|ing)?\b|(can.?t|cannot|unable to|could ?n.?t) (repro|reproduce)\b|unbiased (pass@k )?estimator|harness.?(sha|hash)\b|rate.?limit(ed)? (runs?|trials?)
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Evaluation Loops

An evaluation loop is the run that decides whether a fix or a test is done. It has four moves: reproduce the failure where it was reported, drive the path the real program takes, re-review each fix on the scenario that found it, and count what ran. A fifth move, planting the defect to prove the test can see it, is in the gates way.

## Reproduce the failure as reported

Check a fix against the failure the reporter saw, in the environment where it happened. Start from the reporter's inputs, paths, sizes, versions, and configuration. A failure reproduced in your own setup can be a different failure from the one reported.

When a report lists several failures, confirm each one before the fix and again after it. Record whether each reproduced before the fix. When the reporter's environment is out of reach, name the parts you matched and the parts you could not.

## Drive the path users take

A test reaches its state the way the real program does. When the real loop calls an update function only under a condition, the test drives the loop and lets the loop make the call. A test that calls `tick()`, `update()`, or a setter by hand can reach states the program never reaches and miss states it does.

A headless driver keeps the real cadence. When the interactive loop draws after every key, the driver draws after every key. When drawing sets a value that input reads later, such as a scroll bound, the driver draws before it sends the next key. Timers, ticks, and redraws fire in the same order and under the same conditions as in the interactive run.

A test-only entry point or flag is part of the subject. Check that it runs the same code as the user's path, or drive the user's path instead.

## Fix every finding, then re-review

Remediate each finding as delivery/merge requires, then review again. The re-review replays each finding's original scenario against the fixed code: the input, the key sequence, and the environment that exposed the defect. Reading the new diff is one part of the re-review. A stateful change often takes several rounds. Keep reviewing while rounds still find defects.

## Count what ran

A pass count carries its denominator. A flake that fails 2 runs in 300 passes 20 runs in a row about seven times in eight, so a 20/20 pass usually misses it. For an intermittent failure, loop the test binary until the failure appears and record the run count at which it reproduced. After the fix, run a multiple of that count. That count sits beside the mechanism-level evidence that the recovery way requires for intermittent defects.

Report each evaluation with three facts: the command that ran, how many times, and whether the failure reproduced before the fix.

```
- Reproduced before fix: yes, 3 failures in 1500 runs of the race test
- After fix: 0 failures in 4500 runs; a trace of the reorder path shows it now holds the lock
- Original review scenarios replayed: 4 of 4 pass
```

## Repeated trials of a model or agent

A model run is one sample, so the same counting rules apply, with these additions. The graders and suites are in evaluative/agent-eval.

- Refine the setup on two or three cases, then run one slice, before the full matrix.
- Run each case at least three times, and score the case by the mean of its trials. Confirm any single-run change at the full trial count.
- With n trials and c successes per task, pass@k is the mean of 1 - C(n-c,k)/C(n,k), and pass^k is the mean of C(c,k)/C(n,k). The shortcut 1 - (1-p)^k is biased. The two diverge fast: a 75% per-trial rate gives a pass^3 of about 42%.
- Hash every file that affects a score into one harness hash, and compare only runs with the same hash. Record each run's dataset version, prompt version, model configuration, and judge settings.
- Log each harness failure with a class: timeout, model mismatch, grader refusal, grader error, API error, or harness error. A run that hit a rate or usage limit often scores zero. It is an error, not a wrong answer. Leave truncated answers ungraded and out of the pass rate. Keep refusals as their own metric.
- Record tool calls, tokens, elapsed time, answer cost, and judge cost for each trial. Keep judge cost apart from answer cost.
- For Claude Code, run headless with `claude -p` and JSON output. Scope tools with `--allowedTools`, and keep runs apart with worktrees or `--no-session-persistence`.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "It passes on my machine" | Run it in the reporter's setup, or name what differs. |
| "The unit test covers that state" | Does the test reach the state the way the real loop does? |
| "The diff addresses each comment" | Replay each finding's scenario on the fixed code. |
| "Ran it 20 times, all green" | State the failure rate those 20 runs can rule out. |
| "I couldn't reproduce it, but the fix looks right" | Record it as unreproduced and unverified. |
| "The eval score dropped, so the change regressed" | Check the errors log first. A rate-limited run scores zero too. |

## See Also

- code/testing/evaluation/screens(softwaredev) — child: the visual loop for terminal screens
- code/testing/gates(softwaredev) — plant the defect so a gate is seen red; read state through the client's path
- code/testing/gates/assertions(softwaredev) — golden baselines and the enumerated-delta review
- environment/recovery(softwaredev) — classifying a flaky failure and the evidence that confirms an intermittent fix
- environment/hostparity(softwaredev) — a result on one host is evidence about that host
- environment/debugging(softwaredev) — finding the root cause once the failure reproduces
- delivery/merge(softwaredev) — remediating review findings before the merge gate
- evaluative(evaluative) — when an agent writes the code and a harness judges it: the shared core and the loop types
