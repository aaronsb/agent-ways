---
description: When to reach for the Workflow tool — deterministic multi-agent orchestration (fan-out, staged pipelines, verification and synthesis across many items) and how far up the substrate ladder a task warrants climbing
vocabulary: workflow orchestrate orchestration fan out pipeline multi-agent parallel stage deterministic deliver decompose verify synthesize substrate gate remediate adversarially scale work-list scout four-square decomposition sweep opt-in
pattern: orchestrat|fan.?out|pipeline|multi.?agent
scope: agent
refire: 0.15
---
<!-- epistemic: convention -->
# Workflows

A workflow is a script that runs agents: it starts several in parallel, passes their work through stages, checks the results and combines them. The script holds the control flow (loops, conditionals, fan-out), so nobody decides it turn by turn. ADR-138 names three carriers for a procedure: a skill runs one procedure, a macro injects one command's output, and a workflow coordinates many agents. This way says when to use a workflow.

## The substrate ladder

Most work does not need a workflow. The substrate ladder lists the setups from smallest to largest; use the smallest one that handles the task:

| Substrate | Use when |
|---|---|
| **Single agent / inline** | one bounded job you'll watch |
| **Task list** | a few sequential steps, one driver |
| **Task list + subagents** | independent sub-jobs that parallelize, still one driver (see subagents way) |
| **Workflow** | many items × stages, needs deterministic fan-out / verify / synthesis, or scale beyond one context |

Use a workflow when the task needs **full coverage** (split the work and cover every part in parallel), **confidence** (independent views and an adversarial check before committing to an answer), or **more scale than one context holds** (migrations, audits, broad sweeps).

## The opt-in cost

A workflow can start dozens of agents and spend a large amount of tokens, so **run one only when the user opts in explicitly**. A task that would benefit from a workflow is not a request for one. First find the work list inline (list the files, scope the diff), *then* fan out over it. Size the fan-out to the real amount of work, and **record what you deliberately leave uncovered**, so that nobody reads a partial sweep as a complete one.

## Worked example: the deliver workflow

`develop → pr → review → remediate → merge` suits a workflow. Each unit of work passes through the stages on its own. The *review* fans out across dimensions (bugs, security, reuse), and an adversarial check verifies each finding. Each finding the unit owns is fixed in its branch before the merge, and filing an issue does not count as a fix (ADR-613). The four-square in `delivery/merge` **gates the merge**: a person approves when the work sets direction. The step that cannot be undone is the *release*, which comes after the merge (see the autonomy design note). The workflow saves wall-clock time and reviews more thoroughly than one agent does.

## See also

- subagents(meta) — single delegation, the rung just below a workflow
- workflows/fan-out(meta) — child: running a fan-out with shared contracts, serial writes and one report
- the autonomy design note — the substrate ladder and gate taxonomy in full
- choices(meta) — surfacing the curated decision at a gate
