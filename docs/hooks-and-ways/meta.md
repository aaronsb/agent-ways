# Meta Ways

Guidance for the system that manages guidance. These ways govern how ways themselves are created, how skills differ from ways, how sub-agents are delegated to, and how work state persists across context boundaries.

## Knowledge

**Triggers**: Prompt mentions "way", "ways", "knowledge", "guidance", "context injection"; editing `.claude/ways/*.md`

This is the self-referential way - it explains the ways system itself. It fires when someone is creating or modifying ways, ensuring they have the full specification at hand.

Covers:
- Way file format and all frontmatter fields
- Matching modes (regex, semantic, model)
- State triggers (context-threshold, file-exists, session-start)
- The marker state machine
- Project-local way creation and override semantics
- Domain enable/disable via `ways.json` (global, user scope)
- Per-way enable/disable via `.claude/ways.yaml` (project scope, ADR-131) — `ways disable <name>`

The knowledge way also draws the line between **ways** and **skills**:

| | Skills | Ways |
|--|--------|------|
| **Discovery** | Semantic (Claude decides) | Triggered (patterns, tools, state) |
| **Activation** | Claude matches user intent to description | Hook event fires, pattern matches |
| **Use case** | Specialized knowledge domains | Workflow guardrails, conventions |
| **Can detect** | User intent | Tool execution, file edits, session state |

They complement each other. Skills handle "the user wants to do X" (intent). Ways handle "Claude is about to do Y" (action). A skill can't detect that `git commit` is about to run. A way can't determine that the user's vague request is really about API design.

## Skills

**Triggers**: Prompt mentions "skill", "SKILL.md", "skill creation", "author a skill", "claude code skill"

Guides the creation of SKILL.md files - the mechanism for teaching Claude specialized capabilities that it discovers and applies automatically.

Key distinctions from ways:
- Skills live in `~/.claude/skills/` or `.claude/skills/` (not under `hooks/ways/`)
- Skills are discovered by semantic matching against the `description:` field
- Skills can restrict their tool access via `allowed-tools:`
- Skills can override the model via `model:`

The way provides the SKILL.md structure (frontmatter fields, progressive disclosure for large skills), location precedence (Enterprise > Personal > Project > Plugin), and guidance on writing effective descriptions - the description is how Claude decides when to use the skill, so it needs to capture both what the skill does and when it should be applied.

## Sub-Agents

**Triggers**: Prompt mentions "subagent", "delegate", "spawn agent", "review PR", "plan task", "organize docs"

Documents the available sub-agent types and when to delegate to them:

| Agent | Purpose |
|-------|---------|
| requirements-analyst | Capture complex requirements as GitHub issues |
| system-architect | Draft ADRs, evaluate design trade-offs |
| task-planner | Plan multi-branch implementations |
| code-reviewer | Review PRs for quality and SOLID compliance |
| workflow-orchestrator | Coordinate project phases |
| workspace-curator | Organize docs/ and .claude/ structure |

The key principle: sub-agents are for **delegation of token-intensive work**, not every action. A code review that requires reading a 500-line diff benefits from a dedicated agent with fresh context. A simple file search does not.

The way covers context passing (include specific file paths and line ranges, state what you want back) and anti-patterns (don't delegate routine tasks, don't use agents for quick questions).

## Todos

**Triggers**: State trigger at 75% context threshold

The enforcement mechanism for task list continuity across compaction. This way is unusual in two respects:

1. **It's a state trigger**, not a pattern match. It fires based on how full the context window is, not what the user said.
2. **It repeats** until the condition is resolved. Most ways fire once per session. This one nags on every prompt until `TaskCreate` is used.

The rationale: compaction is the biggest risk to work continuity. When the context window fills up, Claude Code compresses the conversation history. If there's no task list, the compressed context may lose track of what was being worked on, what's been completed, and what remains.

The way fires at 75% to give Claude time to create the task list before compaction occurs (typically at ~90-95%). The repeating behavior exists because earlier versions fired once and Claude routinely ignored it - a single system-reminder is easy to deprioritize when focused on a task.

The nag stops when `TaskCreate` is used, which triggers a `PreToolUse:TaskCreate` hook that creates a marker file. This is the only way in the system with this repeat-until-resolved behavior.

## Teams

**Triggers**: `session-start` (scope: teammate only)

The coordination handbook for team members. When a teammate's session begins, this way fires once and injects the norms that keep a multi-agent team from stepping on itself:

- Check TaskList after completing each task to find next work
- Use SendMessage to report progress and blockers to the lead
- Mark tasks completed via TaskUpdate — don't just say you're done
- Prefer Edit over Write to reduce merge conflicts with other teammates
- Read before editing — another teammate may have changed the file
- Don't commit to git unless the task explicitly says to
- Don't stall silently — message the lead immediately if blocked

These norms exist because teammates are long-lived and collaborative, unlike subagents which do one thing and exit. Coordination failures in a team compound: one teammate that stalls silently or overwrites another's work can derail the whole effort.

The way is gated to `scope: teammate` — the main agent never sees it, and quick subagents don't need it.

See [teams.md](teams.md) for the full three-scope model and detection mechanism.

## Memory

**Triggers**: `session-start` (scope: agent only)

The memory checkpoint way fires at session start to remind the agent about MEMORY.md — the persistent memory that survives across conversations. It's gated to `scope: agent` because only the main session should read and write MEMORY.md. If three teammates all tried to update it simultaneously, the file would get corrupted.

This way works in tandem with the todos way: todos handles in-session task continuity, memory handles cross-session knowledge continuity.

## Tracking

**Triggers**: Prompt mentions "tracking file", "cross-session", "multi-session", "picking up where we left off"; editing `.claude/todo-*.md`

Guides the creation and maintenance of persistent tracking files for work that spans multiple sessions.

While `TaskCreate`/`TaskList` provides in-session task visibility (and survives compaction within a session), tracking files in `.claude/todo-*.md` survive across sessions entirely. They're plain markdown files on disk.

Naming convention: `.claude/todo-{context}.md` - e.g., `todo-adr-005.md`, `todo-pr-42.md`, `todo-issue-17.md`. The context links the tracking file to the work it tracks.

The way prescribes:
- Markdown format with completed/remaining items
- Cleanup when work is done (delete or archive the file)
- Reading tracking files when resuming work after a session break

This complements the todos way: todos ensures in-session task lists exist before compaction; tracking ensures cross-session state persists in files that Claude can read on the next startup.
