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
