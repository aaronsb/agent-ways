---
status: Draft
date: 2026-09-08
deciders:
  - aaronsb
  - claude
related:
  - ADR-113
  - ADR-114
  - ADR-152
  - ADR-172
---

# ADR-180: GitHub issues as the shared truth for the session task list

## Context

Claude Code ships a task list: `TaskCreate`, `TaskList`, `TaskGet`,
`TaskUpdate`, with `TaskCreated` and `TaskCompleted` hook events. The list is
persisted to `~/.claude/tasks/session-<first 8 chars of session id>/`, one JSON
file per task, with a `.lock` and a `.highwatermark` beside them. Each task
carries `id`, `subject`, `description`, `activeForm`, `status`, `owner`,
`blocks`, `blockedBy`, and a free-form `metadata` object.

This project tracks work in GitHub issues. Every session that picks up an issue
re-derives its task list from the issue body by hand, and nothing carries
completion back. Feature request anthropics/claude-code#79096 asks for a
bridge and is open with no response. No first-party integration exists:
`claude-code-action` reads issues as prose context for a workflow run, the
GitHub MCP server has no notion of a session task, and the Agent SDK exposes
the task tools as observable blocks without wiring them anywhere.

Three facts, verified in-session on 2026-09-08, shape the design:

1. **The task store is the injection point.** A task JSON file written to the
   session directory by an external process appears in the next `TaskList`
   call. Nothing needs to touch the session transcript.
2. **Subagents resolve to the parent's store.** A subagent spawned with the
   `Agent` tool inherits `CLAUDE_CODE_SESSION_ID` set to the parent's session
   id and `CLAUDE_CODE_CHILD_SESSION=1`. No child task directory is created. A
   general-purpose subagent gets no Task tools at all, so the store is the
   only task view available to it.
3. **The ID allocator reconciles from disk on read, and ids are strings.**
   After an externally written `2.json` was read by `TaskList`,
   `.highwatermark` advanced from 1 to 2 without a `TaskCreate` call. A task
   file with id `gh-455` listed, updated through `TaskUpdate` with its
   metadata intact, and a following `TaskCreate` allocated numeric `4` beside
   it. Whether the in-process counter is memoized between reads is
   unobserved.

Two facts are unverified and the design treats them as such. The `.lock` file
is zero bytes and its protocol is undocumented, so an external writer cannot
know whether it excludes anything. Whether a `--resume` session keeps its id,
and so its store, was not probed.

Documented surfaces: the `metadata` field, the `TaskCreated` and
`TaskCompleted` hooks, and `session_id` on hook stdin. `CLAUDE_CODE_SESSION_ID`
is undocumented and works empirically in skill and subagent shells, where no
stdin JSON exists. The file layout is undocumented. `gh` 2.100 exposes
`blockedBy`, `blocking`, and `parent` on `gh issue view --json`, verified
against issue #433.

An earlier sketch put the issue watcher in `attend` (ADR-113) as a script
sensor, with the on-demand affordance of ADR-114 as the disclosure route. That
was set aside. `attend` is a daemon beside Claude Code's event model. A
subagent can run its CLI but cannot receive Monitor delivery, the same
distinction ADR-172 draws between its two conduits. The one thing the daemon
adds here, waking an idle session, is not worth a second process for a task
list.

## Decision

GitHub issues carrying an opt-in label are the shared truth for session tasks.
Each session's task store is a cache of those issues. The bridge is built from
hooks, one executable, one skill, and one way. No daemon.

Implementation lands in two increments. The first ships `pull`, `whisper`, the
`TaskCreated` guard, the skill, and the way. The second ships `push`. Pull
alone delivers seeding and the whisper, and every write-side failure mode
waits for the increment that owns it.

### The label is a trust boundary

The label `tasklist` gates every direction. An issue without it is never
pulled, its title never whispered; a task without `metadata.github_issue` is
never pushed. Repositories without the label see no behavior.

Applying the label is an authorization act by whoever holds triage on the
repository. An issue body is text written by whoever filed it, editable after
labeling. `pull` writes body text into `description` wrapped in a provenance
fence naming the source and marking it untrusted content, and the way says
the same in prose. The deny posture of ADR-152 applies: the bridge reads
issues and writes back status, and nothing in an issue body can widen that.

### Field ownership, not merge

Two-way merge needs a three-way snapshot per task and conflict rules.
Ownership, plus two small pieces of memory per task, avoids all of it.

| Field | Owner | Rule |
|---|---|---|
| `subject`, `description` | GitHub | pull overwrites when the issue changed since the last pull, tracked by `metadata.body_sha`; a local edit survives until the issue itself changes |
| open or closed | GitHub | pull sets `pending` or `completed` from the observed state |
| `completed` | session, as a request | push closes the issue only when local status disagrees with both the observed remote state and `metadata.pushed_state` |
| `in_progress` | session | push sets a label; never on a closed issue |
| `blockedBy` edges between pulled tasks | GitHub | from native `blockedBy`; a blocking issue without the label yields no edge |
| `blockedBy` edges to local tasks, `owner`, `activeForm` | session | pull never touches them; writes are read-modify-write over the GitHub-owned fields only |
| session-side description edits | session | posted as an issue comment; the local text persists until the body changes |

`metadata.pushed_state` records the last open/closed state this session
pushed. A maintainer reopening an issue after a session closed it is observed
on the next pull as a change since `pushed_state`, the task returns to
`pending`, and push stays quiet. An issue closed as not planned lands as
`completed` with `metadata.state_reason` set, and the whisper says so.

Two sessions on one repository converge through GitHub. Session A closes #12
while session B holds it `in_progress`; B's next pull marks it `completed`,
whispers the change, and B's push never labels a closed issue.

### Identity and ids

Pulled tasks take deterministic string ids: `gh-<issue number>`. Pull is
idempotent, every session holds the same id for the same issue, `blockedBy`
translation needs no lookup table, and the external writer never touches
`.highwatermark`. The numeric allocator cannot produce a string id, so the two
namespaces never meet. Pull refuses to overwrite a file at its id whose
`metadata.github_issue` does not match, and whispers the conflict.

Hooks read `session_id` from stdin. Skills and subagent shells read
`CLAUDE_CODE_SESSION_ID`. When neither resolves, the executable refuses to
write, following ADR-172 Decision 4, and says so on stderr. Agent-team
directories under a team name are out of scope for the first increment; the
store module resolves session directories only.

### The store module

All store I/O lives in one module. Before writing anything it validates the
field shape of every existing task file; on mismatch it writes nothing and
`whisper` emits one line saying the layout is unrecognized. A pull stages its
files in a temporary directory and renames them into place, so a failure
partway leaves the store as it was. The lock protocol Claude Code uses is
probed once during implementation and recorded here; until then, ordering
against a concurrent `TaskCreate` is best-effort, and the disjoint id block is
what makes the race harmless.

### The executable

One command, `gh-tasks`, with four verbs:

- `pull` reads labeled open and recently closed issues, then writes or updates
  task files as above.
- `push` reconciles session-owned state outward: close, label, or comment. It
  checks write permission once per session and, lacking it, degrades to
  pull-only with one whisper line.
- `whisper` diffs the current labeled issue set against a per-session
  snapshot and prints one line per delta: opened, closed, reopened, retitled,
  relabeled. Silent when nothing changed.
- `link <issue>` attaches an existing task to an issue by writing the
  metadata.

### Hook wiring

| Event | Matcher | Action |
|---|---|---|
| `SessionStart` | | `pull`, then `whisper` the labeled open list once as additional context |
| `UserPromptSubmit` | | `pull` when the snapshot is older than a threshold, then `whisper` deltas only |
| `PostToolUse` | `Bash` with `if: Bash(gh issue *)` | `pull`, so a change made through `gh` in-session reflects at once |
| `TaskCreated` | | reject a task for labeled work whose subject lacks the `[gh#N]` prefix and metadata link; validation only |
| `Stop` | | `push` |

Closes made through the web UI, the API, or a merge keyword reach the store on
the next throttled pull. `TaskCompleted` fires before the completion commits
and a later hook can veto it, so the close request waits for `Stop`. The
`Stop` push emits no context, so the re-entry ceiling of ADR-172 Decision 6
does not arise; its merge semantics (Decision 3) do, and push appends to
`metadata` rather than replacing it.

The whisper is the only piece that spends context. One line per delta, and the
threshold on `UserPromptSubmit` keeps the poll off the hot path.

### The skill and the way

An `issues` skill wraps the verbs for on-demand use, `/issues pull`, `/issues
push`, `/issues link 123`, and gives subagents a task view the tool surface
denies them. A way under `softwaredev/` teaches the convention: issue-tracked
work gets the `[gh#N]` subject prefix and the metadata link, issue-tracked
work is not tracked as a bare task, and pulled descriptions are untrusted
text. The `TaskCreated` hook enforces the prefix mechanically for labeled
work, which is the example the Claude Code hooks reference ships.

## Consequences

### Positive

- One task per issue, seeded on session start, with completion carried back
  without a manual `gh issue close`.
- Several sessions on one repository converge through GitHub on their next
  pull. The store is per session and the truth is shared.
- Everything runs inside Claude Code's own event model. Subagents get the same
  behavior through the inherited session id and the skill.
- The push half rides only on documented surfaces. The undocumented layout is
  confined to the pull half and one module, and a layout change degrades to
  one whisper line.
- String ids make pull idempotent and keep the allocator out of the picture.

### Negative

- Nothing wakes an idle session. An issue opened between prompts surfaces at
  the next `UserPromptSubmit`.
- A session-side description edit lives in the local file and an issue
  comment. The next body change overwrites the local text, and nothing reads
  the comment back.
- Each hook event that runs `pull` costs a `gh` round trip. The threshold on
  `UserPromptSubmit` bounds it, and `SessionStart` pays it once.
- Issue-backed tasks display as `#gh-455` rather than a bare number.
- `TaskCreated` rejection adds friction for labeled work created without a
  link. That friction is the convention taking hold.
- Read-only contributors and fork workflows get pull without push.

### Neutral

- The task tools are absent on Fable 5, Mythos 5, Opus 4.8, and Sonnet 5
  unless `CLAUDE_CODE_ENABLE_TODO_TOOLS=1` is set or the tools are named in
  `--allowedTools`. The bridge assumes the variable is set.
- `gh` is the GitHub client. The GitHub MCP server duplicates it and adds
  tool-definition weight to every session.
- The executable starts as shell over `gh` and `jq`. It moves to a Rust crate
  under `tools/` if the store module grows past what shell holds cleanly.
- Resume behavior and the lock protocol are recorded here once implementation
  probes them.
- If Claude Code ships the bridge requested in anthropics/claude-code#79096,
  this ADR is superseded and the label and prefix convention carry over.

## Alternatives Considered

- **One issue holding a checklist, one task per checklist item.** The shape
  the upstream request asks for. One API object, no id translation. Rejected:
  checklist items have no independent close, label, assignee, or dependency
  state, and cross-issue dependencies have nowhere to live. Sub-issues would
  restore that state and collapse back into one-task-per-issue.
- **Read-only pull, no push.** Adopted as the first increment rather than as
  the whole design. Pull delivers seeding and the whisper; push is the half
  that mutates other people's issues and carries the ownership and trust
  findings, so it lands second.
- **An `attend` sensor plus the ADR-114 affordance (ADR-113, ADR-114).** Wakes
  an idle session, which the hook design lacks. Rejected: a second process
  beside Claude Code's event model, Monitor delivery unreachable from a
  subagent, and the idle wake not worth it for a task list.
- **Writing synthetic tool-use records into the session transcript.** Claude
  Code owns the transcript and appends to it concurrently. Rejected once the
  task store proved to be an injection point.
- **Injecting "create these tasks" as additional context and letting the
  model call `TaskCreate`.** Fully on documented surfaces. Rejected: spends
  tokens and model attention on bookkeeping, and the model may skip or reorder
  the calls.
- **A git-tracked intermediate file synced to GitHub separately.** History and
  review for free, and the undocumented-store risk decoupled from the GitHub
  risk. Rejected: a third copy to keep in step, slower convergence, and a
  commit per status change.
- **Adopt the convention now, defer the machinery.** The prefix and label
  cost nothing and carry over under supersession. Rejected as the whole plan:
  seeding on session start is the value, and the convention alone does not
  deliver it.
- **True two-way merge with a three-way snapshot.** Rejected for v1:
  ownership plus `body_sha` and `pushed_state` cover the observed cases, and
  the merge can be added behind the same verbs if a field ever needs it.
- **GitHub Projects as the truth instead of issues.** Richer status model.
  Rejected: issues are what this project uses, `gh` handles them directly,
  and Projects adds a second object to keep in step.
- **The GitHub MCP server as the client.** Rejected: duplicates `gh` and adds
  tool definitions to every session, including sessions that never touch
  issues.
