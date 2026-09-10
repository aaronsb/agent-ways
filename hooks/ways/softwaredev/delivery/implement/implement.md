---
description: Implementation planning, work breakdown, safe parallelization, briefing the human before writing code, and the four-field shape of a task description
vocabulary: implement build begin start work execute plan breakdown parallelize worktree task sprint kick off begin coding increment contract failing test red rollback depends slice
macro: append
scope: agent
requires: ["Read", "Bash(cat:*)", "Bash(find:*)", "Bash(wc:*)"]
refire: 0.15
---
<!-- epistemic: convention -->
# Implementation Way

## Do Not Reset Context

The discussion that led to the ADR is valuable context. Stay in the same session. Use the planning tool to transition from deliberation to execution — don't start a fresh conversation.

## The Briefing

Before creating any tasks or writing any code, **present the implementation plan to the human as a briefing.** This is not optional. The briefing serves as alignment, not ceremony.

### What the Briefing Covers

1. **What we're building** — one paragraph restating the decision in implementation terms, not ADR terms
2. **Why this approach** — defend the implementation strategy
3. **Work breakdown** — the discrete tasks, with dependencies made explicit
4. **Parallelization plan** — which tasks can run concurrently and why that's safe
5. **Risk areas** — where collisions, complexity, or unknowns live
6. **Expected outcome** — what the codebase looks like when we're done

### Briefing Style

**Defend the plan.** Commit to a position, state what you intend to do, and explain why it's the right approach. Don't hedge with "we could maybe..." or present a menu of options. The act of defending forces you to surface your reasoning, find weaknesses, and revise before presenting. If the defense doesn't hold up under your own scrutiny, revise the plan — then defend the revised version.

The human's role is to challenge, redirect, or approve. That pushback loop — defend, challenge, revise — is how alignment happens. A plan that can't survive a briefing shouldn't survive implementation either.

### Pushback Goes Both Ways

The human will sometimes suggest approaches that won't work — wrong tool, wrong order, wrong abstraction. **Push back when you see a problem**, but separate the intent from the idea. The human usually has the right goal and the wrong mechanism. Your job is to understand *what they're trying to achieve* and propose a better path to get there.

Don't just say "that won't work" — say "I think you're trying to [intent]. Here's why [specific idea] is risky, and here's what I'd do instead to get the same result." Preserve the intent, fix the approach.

## After the Briefing

Once the human approves (or you adjust based on their feedback), create tasks using `TaskCreate` with enough detail for a subagent or post-compaction agent to execute cold — file paths, ADR reference, dependencies, and isolation strategy.

### The Shape of a Task

Each task description carries four fields:

| Field | What it holds |
|---|---|
| Contract | the requirement or ADR clause this increment implements |
| Failing test | the test written first, red until the increment lands; it authorizes the work |
| Rollback path | how the change comes out: revert, feature flag, or a named reverse migration |
| Depends on | earlier tasks and the libraries or services it relies on; `none` is a valid value and a blank field is a defect |

Tasks are listed in dependency order. A task that depends on a later one is misordered. A task whose test cannot be written from its description as written is re-sliced before anyone starts it.

## See Also

- adr(documentation) — implementation follows ADR decisions
- subagents(meta) — parallelization uses subagents
