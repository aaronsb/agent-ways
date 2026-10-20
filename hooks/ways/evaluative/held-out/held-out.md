---
description: held-out evaluation for code whose tests were written by the same person or agent that wrote the code; cases from a fresh writer with no session context who sees only a one-sentence statement of the situation, kept apart from the author, scored only at the gate, and retired once their failures are seen; also success criteria written as numbers, a first case set built from observed failures, two-step synthetic cases, and benchmark contamination
vocabulary: wrote the code and every test for it myself same author wrote both passes on the examples i wrote myself someone who never saw the code writes new ones i will not look at held-out holdout cases written by someone else independent case writer fresh agent no context unseen cases the author never saw self-written cases echo its own wording retire the set gap between held-out and self-written score near-misses contamination self-fit overfitting success criteria eval cases eval dataset golden set synthetic cases tuples persona edge cases should-not cases public benchmark canary training data version the set
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Held-Out Cases

Cases the author writes share the author's assumptions and its vocabulary. A subject tuned against them learns to pass them. Held-out cases are written by someone who has not seen the subject, kept away from the author while it tunes, and scored only at the gate. The distance between the held-out score and the self-written one measures how far the author fit its own cases.

This applies across loop types: the cases of `evaluative/probe-set`, the tasks of `evaluative/agent-eval`, the scenarios of `evaluative/rubric`, the test window of `evaluative/forecast`. Those ways say how each kind of case is scored. This one says how the set is built, who writes it, and who may see it.

## When it applies

The same person or agent wrote the code and every test for it, and all the tests pass. The tests check what the author thought to check, in the author's own terms.

## Criteria before cases

- Write each success criterion as a number on a named set. "Safe outputs" becomes "fewer than 1 in 1,000 outputs flagged, over the full set".
- State the target as a change from a baseline measured on the current version.
- Cover each dimension that can fail: task fidelity, consistency, relevance, tone, privacy, use of context, latency, and cost. Say which errors are minor, which are egregious, and what share of minor errors is allowed.

## The first self-written set

- Run the task with no change and write down each failure in plain words. Those failures and the manual checks you already make become the first cases. Twenty to fifty real cases is enough to start.
- Add edge cases on purpose: missing, irrelevant, very long, harmful, and ambiguous inputs. Include cases where the behaviour should not happen. A set of positive cases alone rewards a subject that always acts.
- Write each case so two domain experts would reach the same verdict on their own. Shape the expected answer so code can check it: a choice, a string, a file state, or a list of key points.
- Tag each case by kind so results can be split later.

## Who writes the cases

A fresh agent with no session context. A stronger model gives a stronger check. A model of the same tier with no context gives a cheaper one, and still removes the author's memory of how the subject is built.

## What the writer sees

One sentence stating the situation the subject serves: who is in it and what they need. Never the implementation, its configuration, its trigger words, or its existing cases. A writer that has read the subject's wording writes cases that echo it, and those cases measure the echo.

Ask for a spread: plain cases, cases phrased the way a person in the situation would put it without naming the topic, and near-misses that should not match.

## Synthetic cases

1. Name the dimensions, such as feature, scenario, and persona.
2. Write about 20 tuples by hand.
3. Have a model generate more tuples. Turn each tuple into a query in a separate prompt. The separate step avoids repetitive phrasing.
4. Run the queries through the real system and read the traces.

The generating model sees the same one-sentence statement a held-out writer sees. Synthetic cases cannot show how common a failure is in real use, so compare them with real inputs once you have some. A person reviews model-written cases before they count.

## Keep the set apart

- Store the set where the author does not read it during tuning, and record who wrote it, from what statement, and when.
- Score it only at the gate. The author sees the score and the count, not the failing cases.
- Once the author has seen a case's failure, that case is training data. Retire the set and have new cases written for the next gate.
- Version the set each time it changes. A set that a script builds from sources is never edited by hand.
- Treat public benchmark tasks as possibly present in a model's training data. Prefer private cases drawn from your own failures. Canary strings do not reliably keep a task out of training data.
- Check that a format change, such as relabelled answer options, does not move the score.

## Report both scores

Report the held-out score beside the self-written score, with case counts for each. A large gap means the subject fits its own cases better than the situation. Close the gap by changing the subject against new self-written cases, then score a fresh held-out set.

## See Also

- evaluative(evaluative) — parent: author and judge, the shared core, choosing the loop
- evaluative/probe-set(evaluative) — scoring a sampled set with a metric
- evaluative/agent-eval(evaluative) — tasks and graders for agents
- documentation/validate(documentation) — the same fresh-reader move applied to documentation
