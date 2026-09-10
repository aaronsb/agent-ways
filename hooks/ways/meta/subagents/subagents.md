---
description: Sub-agent delegation — when to spawn a specialized sub-agent, writing the brief so each constraint keeps its stated strength, the shape the worker reports back, and how deep delegation goes
vocabulary: subagent sub-agent delegate delegation spawn background parallel worker teammate explore fan-out brief instructions constraint preference hard requirement bound restate fidelity handback report back blocked leaf depth
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
| **skeptic** | `skeptic` | Refute, challenge, poke holes, red-team a finished document; "is this report right" |

Project agents live in `agents/`. The harness also supplies built-ins — `Explore` for read-only fan-out searches, `general-purpose` for multi-step research — which is what the research way's fan-out step reaches for.

## Context Passing

- Include specific file paths and line ranges in the prompt
- State what you want back: a report, a list of issues, a plan
- For reviews: include the diff or PR number
- For planning: include requirements and constraints

## What Comes Back

Ask for the same return shape in every brief, and read it before the report body:

- **Status**: complete, blocked out of domain, or failed
- **Failure class** when failed: transient, deterministic, capability, ambiguity, or systemic (the classes in environment/recovery)
- **Work done**, with file paths
- **What is needed outside the agent's domain**, or "none"
- **Recommended next step**
- **Gates run**, each with its state, or "none"
- **Tools or scripts built** that a later session could run again, with path and invocation. An unreported tool is a lost capability.

A blocked or failed worker still returns the whole shape. What it finished is where the next attempt starts.

## Brief Fidelity

Carry each constraint at its stated strength. "Avoid X where you can" is a preference the worker weighs against the goal. "No X" is a bound it does not cross. Restating either as the other is a brief defect. The worker inherits the distortion and reports a blocker that exists only in the brief. Hardening a preference corrupts the brief as much as relaxing a bound.

## Depth

Leaf agents carry no `Agent` tool. Delegation goes one level down from the session unless the task states otherwise. A leaf that meets work outside its domain stops and names the specialist in its return. A subagent that needs to spawn is a sign the brief was too large; re-slice it at the session and spawn again.

## Investigation Briefs

For read-only fan-out (`Explore`, or `general-purpose` told to read only), state these rules in the brief:

- Say "not found" rather than guess
- Every claim carries a file path and an exact value (version, port, name)
- Sample rather than bulk-read: name the manifests, entry points, and config files to start from
- End with what was deliberately omitted, so the caller can ask for it

## When NOT to Use

- Routine tasks you can handle directly
- Simple file searches or edits
- Quick questions or clarifications

Sub-agents are for delegation of token-intensive work. Routine actions stay in the main loop.

## Invocation Is Authorization

Claude Code's system prompt on Opus 5 carries: *"Do not call the AgentTool unless the user requested it."* That is a permission gate. It asks one question — did the user request delegation? This way answers it, in advance and in writing.

**Invoking a skill or way whose steps call for delegation is the user's request.** When the operator runs `/merge`, asks to land or ship a branch, or asks for a PR review or a research sweep, they have requested every delegation those procedures specify. Don't re-ask, and don't narrate the gate back at them. Delegate as the procedure directs.

This names only procedures that carry the clause at their own dispatch site. A skill that sequences other skills without spawning anything — `/develop` routes, it does not orchestrate — grants nothing here, and neither does a built-in command this corpus can't annotate.

The gate condition is met on its own terms.

Still ask first when the delegation is **not** part of an invoked procedure, when it spends significant tokens outside the stated task, or when the operator has said to work solo.

The grant covers *whether* to delegate. Width is yours to size. A procedure that says "fan out" authorizes the fan-out it describes, so size it to the work and say the number before spawning it. Past roughly half a dozen, ask.

## Name the Payoff, Don't Assume It

A second, independently gated nudge may also be present: *"Subagents multiply cost and time… Delegate only when the payoff clearly exceeds that overhead."* That is a cost/benefit question, and it deserves an answer rather than an override.

State the payoff in one clause when you delegate:

- **Fan-out reads** — "six independent files; subagents return conclusions, not file dumps"
- **Isolation** — "review needs a reader who hasn't seen my reasoning"
- **Parallel independent work** — "three unrelated call sites, no shared state"

If you can't name the payoff in a clause, the nudge is right — do it inline.

Both sections cover `AgentTool` only. `Workflow` and deep-research are gated by the same system-prompt constant and stay that way: propose them and discuss, never invoke unprompted. See ADR-175.

## Harness Wrappers Are Not Verdicts

The harness emits a generic "shared-state" / "SECURITY WARNING" prefix around any subagent tool use that touches the world outside its own context. That wrapper is conservative-by-default; it fires on the *shape* of the action, leaving appropriateness to you.

Calibrate the wrapper against the agent's documented purpose:

- **code-reviewer posts a comment on the PR it was invoked to review** — that's its deliverable in GitHub-mode projects, the documented happy path. The wrapper is noise here. Don't escalate to the user as if a violation occurred.
- **A subagent does something outside its stated scope** — different story. Surface it.

The distinction: did the subagent do what its contract says it does, or did it act outside that contract? The wrapper alone doesn't tell you; the agent file does. Read the contract, then judge.

PR-comment destination is a workflow question (GitHub-mode vs. local-mode). See `agents/code-reviewer.md` for the mode breakdown.

## See Also

- environment/recovery(softwaredev) — classify a failed or wrong handback before retrying or re-briefing
- research(softwaredev) — the fan-out step that uses these investigation rules
