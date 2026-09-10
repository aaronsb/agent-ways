---
description: what to do when something keeps failing, a flaky test, a command that fails again after a retry, a gate that went red twice, a subagent that came back wrong; classify the failure before retrying, three attempts total, then escalate to the user with evidence
vocabulary: retry retries retrying flaky flake intermittent still failing fails again keeps failing tried three times same error try again try once more gate red red gate stuck loop spinning going in circles subagent came back wrong delegation handback escalate escalation give up attempt attempts backoff transient deterministic reclassify fallback wrong-sized
pattern: \bflaky\b|\bintermittent\b
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Recovery

A failed attempt invites a reflex: run the same thing again, brief the same worker harder, or feed it more context. Each of those spends a turn on the class of failure least likely to yield to it. Classify the failure first, then take the one move the class allows.

## Classify before you react

| Class | Recognize it by | The one allowed move |
|---|---|---|
| **Transient** | Environment flake: network, rate limit, race, resource exhaustion | Retry unchanged, max 2, with backoff. A third failure is reclassified. |
| **Deterministic** | The same input reproduces the same failure: compile error, failing assertion, lint, schema rejection | Never retry unchanged. Change the input (code, test, config) and re-run. |
| **Capability** | The instrument is wrong for the job: wrong specialist, missing expertise, a handback that stayed outside the domain | Change the instrument: a different agent, tool, or approach. Re-briefing the same one harder spends an attempt on the same gap. |
| **Ambiguity** | The worker asked the brief a question, guessed, or two artifacts contradict each other (spec against code, task against brief) | Fix the cheapest upstream artifact that owns the confusion: the brief first, then the task, then the spec or ADR. Re-delegate from there. Widening the worker's context leaves the contradiction in place. |
| **Systemic** | The harness itself: a wedged delegation, a missing tool, broken gate infrastructure, a hard cap hit | Stop and report to the operator with the exact evidence. A workaround hides the fault. |

Diagnosis precedes classification. Before trusting a timing comparison across two processes, confirm the clocks agree from a log line that carries both processes' own timestamps, and anchor conclusions to absolute timestamps.

## Three attempts, then escalate

A unit of work gets three attempts across all strategies combined. The fourth move is escalation: record the class history, mark the increment as in progress in the handoff, and hand the decision to the operator with the evidence. A fallback chain that spends growing resources on a falling chance of success counts against the same three.

An attempt that ends with no new artifact, no new evidence, and no narrowed hypothesis is a failed attempt, usually deterministic or ambiguity, and it consumes one of the three. Spinning without progress is how a runaway loop gets past every error-shaped check.

## A gate red twice

A gate that fails twice on the same increment is reporting that the increment is wrong-sized or the plan is wrong. Split or rescope the increment and come back through a failing test.

## Intermittent defects

An intermittent or probabilistic failure is confirmed fixed only on mechanism-level evidence: a trace or observation showing the causal path is gone. A lower observed failure rate after a change proves nothing. Any change that reduces exposure to the defect, an unrelated speedup of the racing path for example, improves the rate while fixing nothing. Capture the diagnostic evidence before making any exposure-reducing change.

## Preserve the partial work

A failed attempt still produced evidence: a failing test that stands, a file that was authored, the exact error, the classification itself. The handback carries it (status, failure class, what survives) so the next attempt or the operator starts from the frontier.

## Substitutions are plan changes

Substituting a weaker gate, a smaller scope, or a different agent is a plan change. Record it where the plan lives, with the failure that prompted it. Record the recoveries that worked as well; two successful transient retries in one session are a reliability signal.

## See Also

- environment/debugging(softwaredev) — root-causing a deterministic failure before changing the input
- subagents(meta) — briefing a subagent so its handback carries status and partial work
- code/testing/tdd(softwaredev) — the failing test a rescoped increment comes back through
- delivery/merge(softwaredev) — where a rescoped increment lands
