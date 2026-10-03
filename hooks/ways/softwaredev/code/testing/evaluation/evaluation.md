---
description: the evaluation loop that decides a fix or a test is done; reproduce the reported failure in the reporter's environment, drive the path the real program takes, fix every review finding and re-review on the scenarios that found them, and state how many runs the evidence rests on
vocabulary: reproduce reproduced repro reporter environment as reported real path real loop event loop stateful headless driver tick ticks redraw draw between keys calls by hand directly test-only shortcut reviewer review finding findings re-review round rounds original scenario replay fix the bug loop count runs looped repeat thousand times denominator reproduced before fix confirm fixed flaky flake can't reproduce
pattern: \bre-?review(ed|ing)?\b|(can.?t|cannot|unable to|could ?n.?t) (repro|reproduce)\b
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Evaluation Loops

An evaluation loop is the run that decides whether a fix or a test is done. Four moves keep it honest: reproduce the failure where it was reported, drive the path the real program takes, re-review each fix on the scenario that found it, and count what ran. A fifth move, planting the defect to prove the test can see it, is in the gates way.

## Reproduce the failure as reported

Check a fix against the failure the reporter saw, in the environment where it happened. Start from the reporter's inputs, paths, sizes, versions, and configuration. A failure reproduced in your own setup can be a different failure from the one reported. A fix verified in a short checkout path once left four reported failures standing in the reporter's 131-character path.

When a report lists several failures, confirm each one before the fix and again after it. Record whether each reproduced before the fix. A green after a fix for a failure that never reproduced has no before-state to compare against. When the reporter's environment is out of reach, name the parts you matched and the parts you could not.

## Drive the path users take

A test reaches its state the way the real program does. When the real loop calls an update function only under a condition, the test drives the loop and lets the loop make the call. A test that calls `tick()`, `update()`, or a setter by hand can reach states the program never reaches and miss states it does.

A headless driver keeps the real cadence. When the interactive loop draws after every key, the driver draws after every key. When drawing sets a value that input reads later, such as a scroll bound, the driver draws before it sends the next key. Timers, ticks, and redraws fire in the same order and under the same conditions as in the interactive run.

A test-only entry point or flag is part of the subject. Check that it runs the same code as the user's path, or drive the user's path instead.

## Fix every finding, then re-review

Fix every finding a review raises, or record why one is declined. Then review again. The re-review replays each finding's original scenario against the fixed code: the input, the key sequence, and the environment that exposed the defect. Reading the new diff is one part of the re-review. A stateful change often takes several rounds. Keep reviewing while rounds still find defects.

## Count what ran

A pass count carries its denominator. A flake that fails 2 runs in 300 passes 20 runs in a row about seven times in eight, so "20/20 passed" leaves it undetected. For an intermittent failure, loop the test binary until the failure appears and record the run count at which it reproduced. After the fix, run a multiple of that count. That count sits beside the mechanism-level evidence that the recovery way requires for intermittent defects.

Report each evaluation with three facts: the command that ran, how many times, and whether the failure reproduced before the fix.

```
- Reproduced before fix: yes, 3 failures in 1500 runs of the race test
- After fix: 0 failures in 4500 runs; the lock now covers the reorder path
- Original review scenarios replayed: 4 of 4 pass
```

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "It passes on my machine" | Run it in the reporter's setup, or name what differs. |
| "The unit test covers that state" | Does the test reach the state the way the real loop does? |
| "The diff addresses each comment" | Replay each finding's scenario on the fixed code. |
| "Ran it 20 times, all green" | State the failure rate those 20 runs can rule out. |
| "I couldn't reproduce it, but the fix looks right" | Record it as unreproduced and unverified. |

## See Also

- code/testing/evaluation/screens(softwaredev) — child: the visual loop for terminal screens
- code/testing/gates(softwaredev) — plant the defect so a gate is seen red; read state through the client's path
- code/testing/gates/assertions(softwaredev) — golden baselines and the enumerated-delta review
- environment/recovery(softwaredev) — classifying a flaky failure and the evidence that confirms an intermittent fix
- environment/hostparity(softwaredev) — a result on one host is evidence about that host
- environment/debugging(softwaredev) — finding the root cause once the failure reproduces
- delivery/merge(softwaredev) — remediating review findings before the merge gate
