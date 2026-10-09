---
description: held-out evaluation cases written by someone other than the author; a fresh writer with no session context sees only a one-sentence statement of the situation, never the implementation or its wording, and the set is kept apart, scored only at the gate, and retired once the author has seen its failures
vocabulary: passes on the examples i wrote myself someone who never saw the code writes new ones i will not look at held-out holdout cases written by someone else independent case writer fresh agent no context blind cases unseen cases the author never saw self-written cases echo its own wording retire the set replace with new cases gap between held-out and self-written score
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Held-Out Cases

Cases the author writes share the author's assumptions and its vocabulary. A subject tuned against them learns to pass them. Held-out cases are written by someone who has not seen the subject, kept away from the author while it tunes, and scored only at the gate. The distance between the held-out score and the self-written one measures how far the author fit its own cases.

This applies across loop types: the cases of `evaluative/probe-set`, the tasks of `evaluative/agent-eval`, the scenarios of `evaluative/rubric`, the test window of `evaluative/forecast`. Those ways say how each kind of case is scored; this one says who writes it and who may see it.

## Who writes the cases

A fresh agent with no session context. A stronger model gives a stronger check. A model of the same tier with no context gives a cheaper one, and still removes the author's memory of how the subject is built.

## What the writer sees

One sentence stating the situation the subject serves: who is in it and what they need. Never the implementation, its configuration, its trigger words, or its existing cases. A writer that has read the subject's wording writes cases that echo it, and those cases measure the echo.

Ask for a spread: plain cases, cases phrased the way a person in the situation would put it without naming the topic, and near-misses that should not match.

## Keep the set apart

- Store the set where the author does not read it during tuning, and record who wrote it, from what statement, and when.
- Score it only at the gate. The author sees the score and the count, not the failing cases.
- Once the author has seen a case's failure, that case is training data. Retire the set and have new cases written for the next gate.

## Report both scores

Report the held-out score beside the self-written score, with case counts for each. A large gap means the subject fits its own cases better than the situation. Close the gap by changing the subject against new self-written cases, then score a fresh held-out set.

## See Also

- evaluative(evaluative) — parent: author and judge, the shared core, choosing the loop
- evaluative/probe-set(evaluative) — scoring a sampled set with a metric
- evaluative/agent-eval(evaluative) — tasks and graders for agents
- documentation/validate(documentation) — the same fresh-reader move applied to documentation
