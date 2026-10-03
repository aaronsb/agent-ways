# Meta Ways

The `meta` domain holds guidance about the system itself and about how Claude works with the human: how ways and skills are written, how work is delegated, how state survives compaction, and how trust is handled. This page lists the top-level meta ways. The way files under `hooks/ways/meta/` are the source; `ways session ways` shows which fired in a session, and `ways author graph` exports the whole tree.

| Way | Covers | Fires on |
|-----|--------|----------|
| `attend/*` | Responding to attend signals: a finished build, context pressure, an overdue reflection | attend signals |
| `choices` | Putting real decisions to the human as explicit choices | prompt pattern ("which option", "let me decide") |
| `compaction-checkpoint` | Summarizing and checking in with the human before compaction | context 85% full |
| `deployment` | How agent-ways installs, updates and reconciles into `~/.claude` | prompt pattern (agent-ways, `ways update`, `~/.claude`) |
| `develop` | Routing a piece of work through the development loop and the `/develop` skill | prompt pattern ("how should we approach") |
| `goals` | When to set a `/goal` and what goal mode changes | semantic |
| `governance` | Citing the real controls behind a practice | semantic |
| `introspection` | Looking back over the session at pull-request time for guidance worth keeping | `gh pr create`, prompt pattern |
| `knowledge` | The ways system: ways, skills and hooks, domains, matching. Children cover authoring, frontmatter, refire, trees, the keyword lane, locale stubs and vocabulary tuning. | prompt pattern ("ways"), editing way files |
| `memory` | The persistent memory files: what to record and when | context 80% full, "save to memory", editing memory files |
| `skills` | House conventions for writing Claude Code skills | prompt pattern (`SKILL.md`, "author a skill") |
| `start` | Recognizing session start and routing to `/start` | prompt pattern ("pick up where we left off") |
| `subagents` | When to delegate to a sub-agent, writing the brief, what comes back | prompt pattern ("subagent", "delegate") |
| `think` | Structured reasoning for hard decisions and the `/think` skill | prompt pattern ("trade-off", "I'm stuck") |
| `todos` | Capturing unfinished work as tasks before compaction | context 75% full with no task list |
| `trust/*` | Trust between Claude and the human: earned autonomy, acting through the human's accounts, whose voice to write in, long-form prose | semantic, prompt pattern |
| `workflows` | When to reach for the Workflow tool for multi-agent orchestration | prompt pattern ("fan out", "pipeline") |
| `wrap` | Recognizing session end and routing to `/wrap` | prompt pattern ("wrap up") |

## Ways and skills

The `knowledge` way draws the line between the two:

| | Skills | Ways |
|--|--------|------|
| **Discovery** | Semantic (Claude decides) | Triggered (patterns, tools, state) |
| **Activation** | Claude matches user intent to the description | A hook event fires and a trigger matches |
| **Use case** | Specialized procedures | Workflow guardrails, conventions |
| **Can detect** | User intent | Tool execution, file edits, session state |

They complement each other. Skills handle "the user wants to do X". Ways handle "Claude is about to do Y". A skill cannot see that `git commit` is about to run; a way cannot tell that a vague request is really about API design.

## Sub-agents

The `subagents` way covers delegating to the agents shipped in `agents/`:

| Agent | Purpose |
|-------|---------|
| requirements-analyst | Capture requirements as GitHub issues |
| system-architect | Draft ADRs and evaluate design trade-offs |
| task-planner | Plan multi-branch implementations |
| code-reviewer | Review PRs for quality and SOLID compliance |
| workflow-orchestrator | Coordinate the ADR-driven workflow |
| workspace-curator | Organize `docs/` and `.claude/` |
| skeptic | Try to refute a finished deliverable's claims from primary sources, read-only |

Sub-agents are for delegating token-heavy work, such as reviewing a long diff with fresh context, not for every action.

Team coordination for teammates is the `collaboration/teams` way; see [teams.md](teams.md).
