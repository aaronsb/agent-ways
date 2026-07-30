---
description: When to reach for the Workflow tool — deterministic multi-agent orchestration (fan-out, staged pipelines, verification and synthesis across many items) and how far up the substrate ladder a task warrants climbing
vocabulary: workflow orchestrate orchestration fan out pipeline multi-agent parallel stage deterministic deliver decompose verify synthesize substrate gate remediate adversarially scale
pattern: orchestrat|fan.?out|pipeline|multi.?agent
scope: agent
refire: 0.15
---
<!-- epistemic: convention -->
# Workflows

A workflow is **deterministic orchestration**: a script that fans agents out,
pipelines them through stages, verifies, and synthesizes — control flow you
*encode* (loops, conditionals, fan-out) rather than improvise turn by turn. Per
ADR-138 it's the third *how*-carrier: a skill is one procedure, a macro is one
injected command, a workflow is many agents coordinated. This way is *when* to
reach for one.

## The substrate ladder

Most work doesn't need a workflow. Climb only as far as the task demands:

| Substrate | Use when |
|---|---|
| **Single agent / inline** | one bounded job you'll watch |
| **Task list** | a few sequential steps, one driver |
| **Task list + subagents** | independent sub-jobs that parallelize, still one driver (see subagents way) |
| **Workflow** | many items × stages, needs deterministic fan-out / verify / synthesis, or scale beyond one context |

Reach for a workflow to be **comprehensive** (decompose and cover in parallel),
**confident** (independent perspectives + adversarial verification before
committing), or to take on **scale one context can't hold** (migrations, audits,
broad sweeps).

## The opt-in cost

A workflow can spawn dozens of agents and burn a large amount of tokens, so it is
**explicit opt-in** — never inferred from a task that merely *would* benefit.
Scout inline first to discover the work-list (list the files, scope the diff),
*then* fan out over it. Match the fan-out to the real scale, and **log what you
deliberately leave uncovered** so a bounded sweep never reads as exhaustive.

## Worked example: the deliver workflow

`develop → pr → review → remediate → merge` is workflow-shaped: pipeline each unit of work through the stages independently; fan the *review* out across dimensions (bugs, security, reuse) and adversarially verify each finding; remediate per finding; **gate at merge** per the four-square (`delivery/merge`): the human gate applies when the work sets direction, and the irreversible one-way door is *release*, not the merge itself (see the autonomy design note). The payoff is wall-clock and rigor, not novelty.

## See also

- subagents(meta) — single delegation, the rung just below a workflow
- the autonomy design note — the substrate ladder and gate taxonomy in full
- choices(meta) — surfacing the curated decision at a gate
