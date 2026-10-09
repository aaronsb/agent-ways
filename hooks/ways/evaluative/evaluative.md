---
description: evaluative loop engineering, where a coding agent writes the software and a harness and oracle it does not control judge whether the work is done; separating the author from the judge, and choosing which kind of evaluative loop fits the product
vocabulary: evaluative loop coding agent writes all the code what decides its work is done author judge harness oracle verdict agent-written agent authored self-grade grade its own homework answer key baseline regenerate weaken loosen tolerance exit status instrument greater loop development loop judge the output looks right done claim count harness grows escaped defect
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Evaluative Loops

In an evaluative loop the agent writes the software and something it does not control decides whether the work is done. Three roles: the **author** (the agent), the **instruments** (the harness, drivers and readouts that run the product and observe it), and the **oracle** (what the output is compared against: a frozen baseline, an external reference, a metric over a sample, a rubric, or the operator). Done is a verdict the agent read back from an instrument. A verdict the agent asserted is a claim (see delivery/groundwork/claims).

## When it applies

Someone has finished a change and is about to call it done, hand it over, or ship it, and the evidence so far is that it reads correctly, looks right, or the author is confident. Nothing outside the author has run it and reported back.

## The shared core

These hold in every loop type below.

- **The author does not touch the judge.** The agent never writes, weakens, or regenerates the oracle that judges its own change: no re-recorded baseline, widened tolerance, deleted case, or rubric item rewritten to match what the code now produces. A change to the oracle is its own change, shown to the operator with what moved and by how much.
- **The verdict is the exit status.** A harness that prints failures and exits zero leaves the agent as the only gate. The run fails when a check fails.
- **Gates and instruments are proven as code/testing/gates requires:** a skipped check reads as skipped, and each instrument has been seen red once.
- **A cited count comes from a run in this change.** A pass count in a commit, PR, or handoff names the command and was produced after the last edit.
- **The harness grows with the product.** A feature ships its check in the same change. A defect that escaped the harness becomes a case before the fix lands.

## Choose the loop

Pick by what the product emits and what can judge it. A project often runs more than one.

| The output is | What can judge it | Loop |
|---|---|---|
| A running interactive program: a game, a web page, a GUI | State readouts plus the agent looking at screenshots | `evaluative/perceptual` |
| Behaviour inside a platform that is risky or hidden to run directly: a compositor, a kernel module, a device, a shared service | The platform's own state, read inside an isolated instance | `evaluative/sandboxed` |
| An installer, package, or first-run setup | A fresh disposable environment, seeded with a user's existing configuration | `evaluative/clean-room` |
| A reconstruction or port with surviving reference output | An external answer key, plus a frozen baseline for regression | `evaluative/fidelity` |
| A ranking, routing, retrieval, or scoring change | A metric over a sampled set, with an unrelated set for separation | `evaluative/probe-set` |
| A model's answer, worded differently every run | Deterministic checks plus a rubric with a hit threshold | `evaluative/rubric` |
| A multi-turn agent that uses tools | The end state it leaves and its trajectory, over repeated trials | `evaluative/agent-eval` |
| A predictive or analytical model of time-ordered data | Held-out future data, against a naive baseline | `evaluative/forecast` |
| A measurement with no fixed expected value: timings, telemetry, drift | A model of normal behaviour, judged by the residual | `evaluative/model-oracle` |
| A change to an agent's context: a prompt, skill, tool, or guidance | Arms with and without it, graded by a blind judge | `evaluative/ablation` |
| The cases for any loop above | Cases written by someone who never saw the subject, scored only at the gate | `evaluative/held-out` |

Some of these sit close together. `sandboxed` isolates a running subject so it can be observed. `clean-room` proves the product installs and first runs from nothing. `rubric` scores one answer, and can run inside a clean room once its install has passed. `agent-eval` judges a whole run of an agent, not a single answer. `forecast` evaluates a model the agent wrote; `model-oracle` uses a fitted model as the judge of something else.

## See Also

- delivery/groundwork/claims(softwaredev) — an agent's report is a claim until something else has run it
- code/testing/gates(softwaredev) — executed, discovered, absent; a zero needs a positive control
- code/testing/evaluation(softwaredev) — reproduce as reported, drive the real path, count what ran
- environment/hostparity(softwaredev) — a verdict on the authoring host says nothing about other hosts
