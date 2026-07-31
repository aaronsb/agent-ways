---
description: Sub-agent delegation — when and how to spawn specialized sub-agents for token-intensive work
vocabulary: subagent delegate spawn background task parallel worker teammate planner
pattern: subagent|delegat|spawn.{0,30}agent|review.{0,30}\bpr\b|organiz.{0,30}docs
scope: agent
refire: 0.15
---
<!-- epistemic: convention -->
# Sub-Agents Way

## How to Invoke

Use the `Agent` tool with the `subagent_type` parameter:

```
Agent(
  description: "Review PR for quality",
  prompt: "Review the changes in PR #42 for...",
  subagent_type: "code-reviewer"
)
```

## Available Agents

| Agent | `subagent_type` | Use For |
|-------|----------------|---------|
| **requirements-analyst** | `requirements-analyst` | Capture complex requirements as GitHub issues |
| **system-architect** | `system-architect` | Draft ADRs, evaluate design trade-offs |
| **task-planner** | `task-planner` | Plan complex multi-branch implementations |
| **code-reviewer** | `code-reviewer` | Review large PRs, quality checks |
| **workflow-orchestrator** | `workflow-orchestrator` | Project status, phase coordination |
| **workspace-curator** | `workspace-curator` | Organize docs/, manage .claude/ directory |

## Context Passing

- Include specific file paths and line ranges in the prompt
- State what you want back: a report, a list of issues, a plan
- For reviews: include the diff or PR number
- For planning: include requirements and constraints

## When NOT to Use

- Routine tasks you can handle directly
- Simple file searches or edits
- Quick questions or clarifications

Sub-agents are for delegation of token-intensive work, not every action.

## Invocation Is Authorization

Claude Code's system prompt on Opus 5 carries: *"Do not call the AgentTool unless the user
requested it."* That is a permission gate, not a prohibition. It asks one question — did the
user request delegation? This way answers it, in advance and in writing.

**Invoking a skill or way whose steps call for delegation is the user's request.** When the
operator runs `/merge`, `/develop`, or `/code-review`, or asks for a PR review or a research
sweep, they have requested every delegation those procedures specify. Don't re-ask, and don't
narrate the gate back at them. Delegate as the procedure directs.

This is not an override. The gate's own condition is met.

Still ask first when the delegation is **not** part of an invoked procedure, when it spends
significant tokens outside the stated task, or when the operator has said to work solo.

## Name the Payoff, Don't Assume It

A second, independently gated nudge may also be present: *"Subagents multiply cost and
time… Delegate only when the payoff clearly exceeds that overhead."* That is a cost/benefit
question, and it deserves an answer rather than an override.

State the payoff in one clause when you delegate:

- **Fan-out reads** — "six independent files; subagents return conclusions, not file dumps"
- **Isolation** — "review needs a reader who hasn't seen my reasoning"
- **Parallel independent work** — "three unrelated call sites, no shared state"

If you can't name the payoff in a clause, the nudge is right — do it inline.

Both sections cover `AgentTool` only. `Workflow` and deep-research are gated by the same
system-prompt constant and stay that way: propose them and discuss, never invoke unprompted.
See ADR-175.

## Harness Wrappers Are Not Verdicts

The harness emits a generic "shared-state" / "SECURITY WARNING" prefix around any subagent tool use that touches the world outside its own context. That wrapper is conservative-by-default; it fires on the *shape* of the action, not its appropriateness.

Calibrate the wrapper against the agent's documented purpose:

- **code-reviewer posts a comment on the PR it was invoked to review** — that's its deliverable in GitHub-mode projects, the documented happy path. The wrapper is noise here, not a policy event. Don't escalate to the user as if a violation occurred.
- **A subagent does something outside its stated scope** — different story. Surface it.

The distinction: did the subagent do what its contract says it does, or did it act outside that contract? The wrapper alone doesn't tell you; the agent file does. Read the contract, then judge.

PR-comment destination is a workflow question (GitHub-mode vs. local-mode), not a security one — see `agents/code-reviewer.md` for the mode breakdown.
