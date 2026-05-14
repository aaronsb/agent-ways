---
description: Sub-agent delegation — when and how to spawn specialized sub-agents for token-intensive work
vocabulary: subagent delegate spawn background task parallel worker teammate
pattern: subagent|delegat|spawn.*agent|review.*pr|plan.*task|organiz.*docs
scope: agent
refire: 0.15
---
<!-- epistemic: convention -->
# Sub-Agents Way

## How to Invoke

Use the `Task` tool with `subagent_type` parameter:

```
Task(
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

## Harness Wrappers Are Not Verdicts

The harness emits a generic "shared-state" / "SECURITY WARNING" prefix around any subagent tool use that touches the world outside its own context. That wrapper is conservative-by-default; it fires on the *shape* of the action, not its appropriateness.

Calibrate the wrapper against the agent's documented purpose:

- **code-reviewer posts a comment on the PR it was invoked to review** — that's its deliverable in GitHub-mode projects, the documented happy path. The wrapper is noise here, not a policy event. Don't escalate to the user as if a violation occurred.
- **A subagent does something outside its stated scope** — different story. Surface it.

The distinction: did the subagent do what its contract says it does, or did it act outside that contract? The wrapper alone doesn't tell you; the agent file does. Read the contract, then judge.

PR-comment destination is a workflow question (GitHub-mode vs. local-mode), not a security one — see `agents/code-reviewer.md` for the mode breakdown.
