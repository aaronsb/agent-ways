---
status: Draft
date: 2026-09-08
deciders:
  - aaronsb
  - claude
related:
  - ADR-113
  - ADR-114
  - ADR-172
---

# ADR-180: GitHub issues as the shared truth for the session task list

## Context

Claude Code ships a task list: `TaskCreate`, `TaskList`, `TaskGet`,
`TaskUpdate`, with `TaskCreated` and `TaskCompleted` hook events. The list is
persisted to `~/.claude/tasks/session-<first 8 chars of session id>/`, one JSON
file per task, with a `.lock` and a `.highwatermark` beside them. Resumed
sessions keep their tasks. Each task carries `id`, `subject`, `description`,
`activeForm`, `status`, `owner`, `blocks`, `blockedBy`, and a free-form
`metadata` object.

This project tracks work in GitHub issues. Every session that picks up an issue
re-derives its task list from the issue body by hand, and nothing carries
completion back. Feature request anthropics/claude-code#79096 asks for exactly
this bridge and is open with no response. No first-party integration exists:
`claude-code-action` reads issues as prose context for a workflow run, the
GitHub MCP server has no notion of a session task, and the Agent SDK exposes
the task tools as observable blocks without wiring them anywhere.

Three facts, verified in-session on 2026-09-08, shape the design:

1. **The task store is the injection point.** A task JSON file written to the
   session directory by an external process appears in the next `TaskList`
   call. The store is built for multi-process access, since agent teams share
   one directory under a team name, and the lock file is the coordination
   primitive. Nothing needs to touch the session transcript.
2. **Subagents resolve to the parent's store.** A subagent spawned with the
   `Agent` tool inherits `CLAUDE_CODE_SESSION_ID` set to the parent's session
   id and `CLAUDE_CODE_CHILD_SESSION=1`. No child task directory is created. A
   general-purpose subagent gets no Task tools at all, so the store is the
   only task view available to it.
3. **The ID allocator reconciles from disk.** After an externally written
   `2.json` was read, `.highwatermark` advanced from 1 to 2 without a
   `TaskCreate` call.

The file layout is undocumented. The `metadata` field, the `TaskCreated` and
`TaskCompleted` hooks, and the `CLAUDE_CODE_SESSION_ID` variable are documented
or stable surfaces.

An earlier sketch put the issue watcher in `attend` (ADR-113) as a script
sensor and surfaced deltas through the insistent trigger path (ADR-114). That
was set aside: `attend` is a daemon orthogonal to how Claude Code runs, a
subagent cannot reach it, and the one thing it adds here, waking an idle
session, is not worth a second process for a task list.

## Decision

GitHub issues carrying an opt-in label are the shared truth for session tasks.
Each session's task store is a cache of those issues. The bridge is built from
hooks, one executable, one skill, and one way. No daemon.

### Field ownership, not merge

Two-way merge needs a three-way snapshot per task and conflict rules. Ownership
avoids all of it.

| Field | Owner | Direction |
|---|---|---|
| `subject`, `description`, `blockedBy` | GitHub | pull overwrites the task |
| `status` | session | `completed` closes the issue; `in_progress` sets a label |
| session-side edits to the description | session | posted as an issue comment, never a body rewrite |

Issue title maps to `subject`, body to `description`, number to
`metadata.github_issue`, URL to `metadata.github_url`, open/closed to
`pending`/`completed`. `in_progress` maps to a label on the issue. Issue
dependencies map to `blockedBy` where GitHub exposes them; otherwise an
optional fenced `yaml` block at the top of the issue body carries `blockedBy`
by issue number and `activeForm`. That block is deferred until a field needs
it.

### The executable

One command, `gh-tasks`, with four verbs:

- `pull` reads open issues with the label, then writes or updates task files
  under `.lock`, keyed on `metadata.github_issue`, advancing `.highwatermark`
  past any id it allocates. A task whose issue closed elsewhere is marked
  `completed`.
- `push` reads task files with `metadata.github_issue` and reconciles status
  outward: close, label, or comment.
- `whisper` diffs the current issue set against a per-session snapshot and
  prints one line per delta: opened, closed, retitled, relabeled. Silent when
  nothing changed.
- `link <issue>` attaches an existing task to an issue by writing the
  metadata.

The store path resolves from `CLAUDE_CODE_SESSION_ID`, present in hook
environments, skill invocations, and subagent shells alike. All store I/O lives
in one module that validates field shape on read and does nothing when the
layout does not match, so a Claude Code change to the undocumented layout
degrades to a no-op rather than a corrupted list.

### Hook wiring

| Event | Action |
|---|---|
| `SessionStart` | `pull`, then `whisper` the full open list once as additional context |
| `UserPromptSubmit` | `pull` when the snapshot is older than a threshold, then `whisper` deltas only |
| `PostToolUse` on `Bash(gh issue *)` | `pull` immediately, so a close or edit made in-session reflects at once |
| `TaskCreated` | reject a task for labeled work whose subject lacks the `[gh#N]` prefix and metadata link |
| `TaskCompleted` | `push` |
| `Stop` | `push` status drift accumulated during the turn, following the drain pattern of ADR-172 |

The whisper is the only piece that spends context. One line per delta, and the
threshold on `UserPromptSubmit` keeps the poll off the hot path.

### The skill and the way

An `issues` skill wraps the verbs for on-demand use, `/issues pull`, `/issues
push`, `/issues link 123`, and gives subagents a task view the tool surface
denies them. A way under `softwaredev/` teaches the convention: issue-tracked
work gets the `[gh#N]` subject prefix and the metadata link, and issue-tracked
work is not tracked as a bare task. The `TaskCreated` hook enforces the
convention mechanically for labeled work, which is the example the Claude Code
hooks reference ships.

### Opt-in

The label `tasklist` gates every direction. An issue without it is never pulled;
a task without `metadata.github_issue` is never pushed. Repositories without
the label see no behavior.

## Consequences

### Positive

- One task list per issue, seeded on session start, with completion carried
  back without a manual `gh issue close`.
- Several sessions on one repository converge through GitHub on their next
  pull. The store is per session and the truth is shared.
- Everything runs inside Claude Code's own event model. Subagents get the same
  behavior through the inherited session id and the skill.
- The push half rides only on documented surfaces. The undocumented layout is
  confined to the pull half and one module.

### Negative

- Nothing wakes an idle session. An issue opened while the session sits
  between prompts surfaces at the next `UserPromptSubmit`.
- The pull half depends on an undocumented file layout. The fail-closed reader
  turns a layout change into silence, and silence has to be noticed.
- Each hook event that runs `pull` costs a `gh` round trip. The threshold on
  `UserPromptSubmit` bounds it, and `SessionStart` pays it once.
- `TaskCreated` rejection adds friction for labeled work created without a
  link. That friction is the convention taking hold.

### Neutral

- The task tools are absent on Fable 5, Mythos 5, Opus 4.8, and Sonnet 5 unless
  `CLAUDE_CODE_ENABLE_TODO_TOOLS=1` is set or the tools are named in
  `--allowedTools`. The bridge assumes the variable is set and does nothing
  useful without it.
- `gh` is the GitHub client. The GitHub MCP server duplicates it and adds
  tool-definition weight to every session.
- The executable starts as shell over `gh` and `jq`. It moves to a Rust crate
  under `tools/` if the store module or the diff grows past what shell holds
  cleanly.
- If Claude Code ships the bridge requested in anthropics/claude-code#79096,
  this ADR is superseded and the label convention carries over.

## Alternatives Considered

- **An `attend` sensor plus an insistent way trigger (ADR-113, ADR-114).**
  Wakes an idle session, which is the one capability the hook design lacks.
  Rejected: a second process orthogonal to Claude Code's event model, out of
  reach for subagents, and the idle wake is not worth it for a task list.
- **Writing synthetic tool-use records into the session transcript.** Claude
  Code owns the transcript and appends to it concurrently. Rejected once the
  task store proved to be a supported multi-process injection point.
- **Injecting "create these tasks" as additional context and letting the model
  call `TaskCreate`.** Fully on documented surfaces. Rejected: spends tokens and
  model attention on bookkeeping, and the model may skip or reorder the calls.
- **True two-way merge with a three-way snapshot.** Handles concurrent edits
  to the same field. Rejected for v1: ownership covers the real cases, and the
  merge can be added behind the same verbs if a field ever needs it.
- **GitHub Projects as the truth instead of issues.** Richer status model.
  Rejected: issues are what this project already uses, `gh` handles them
  directly, and Projects adds a second object to keep in step.
- **The GitHub MCP server as the client.** Rejected: duplicates `gh` and adds
  tool definitions to every session, including sessions that never touch
  issues.
