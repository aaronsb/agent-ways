---
description: Sub-agent delegation — when to spawn a specialized sub-agent, writing the brief so each constraint keeps its stated strength, the shape the worker reports back, and how deep delegation goes
vocabulary: subagent sub-agent delegate delegation spawn background parallel worker teammate explore fan-out brief instructions constraint preference hard requirement bound restate fidelity handback report back blocked leaf depth worktree isolation skeptic roster red-team shared-state unreported
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

## Worktree Isolation

A worktree agent starts from the default branch. When the work belongs on another branch, the brief has it run `git checkout -b <branch> <base>` and check `git merge-base --is-ancestor <base> HEAD`.

A worktree that builds into the main checkout's target directory leaves binaries the main checkout reuses, with the worktree's paths compiled in. If that happened, run `cargo clean -p <crate>` there.

## Investigation Briefs

For read-only fan-out (`Explore`, or `general-purpose` told to read only), state these rules in the brief:

- Say "not found" rather than guess
- Every claim carries a file path and an exact value (version, port, name)
- Sample rather than bulk-read: name the manifests, entry points, and config files to start from
- End with what was deliberately omitted, so the caller can ask for it

## Planting and Re-review Briefs

A brief that has the worker plant a regression to prove a test catches it names the restore steps:

- Back the file up, plant, run the test, move the backup into place: `cp f f.bak`, edit `f`, test, `mv f.bak f`. `git checkout`, `git restore` and `git stash` discard the worker's uncommitted edits along with the plant.
- Run `touch f` after the `mv`. The moved file keeps the backup's older mtime, so mtime-based build tools (cargo, make) skip the rebuild and run the planted binary. A planted regression that stays red after the restore is the tell.

A re-review after a fix is sized to the fix. A one-function fix gets that function's tests and the crate's suite. The full gate set runs once, before the push.

## When NOT to Use

- Routine tasks you can handle directly
- Simple file searches or edits
- Quick questions or clarifications

Sub-agents are for delegation of token-intensive work. Routine actions stay in the main loop.

## Invocation Is Authorization

Claude Code 2.1.306 puts one sentence in the system prompt: *"Do not use the Agent tool, workflows, or deep-research unless the user, a CLAUDE.md file, or a skill asks for it"*. By default it reaches claude-opus-5 only, and a server can serve it to other models. A server can also still serve the older sentence, which names only the user. Where no such text is present, the payoff section below is the check.

Three sources ask for a spawn:

- The user, in the conversation. Asking to land a branch, review a PR or sweep a topic asks for the delegation that task specifies.
- A skill in use. `/start`, `/develop` and `/merge` carry a clause at their own dispatch sites naming the delegation their stages call for. A built-in command grants nothing.
- A CLAUDE.md file, user-level or project-level.

A way's own text is not a request. It names the spawn, the width and the payoff. When none of the three sources covers the step and your instructions carry the restriction, say in one line what you would spawn and how wide, then wait or do the work inline.

The operator can cover every such step with one line in their CLAUDE.md. The install never writes it. Suggested:

> Delegation that an agent-ways skill or way directs at its own step counts as asked for. Spawn what the step names, at the width it states, and ask past about six agents.

When the user asks for a check whose payoff is independence from the author, that request asks for the independent reader: a cold read of documentation, held-out cases, a refutation pass, a blind judge, a fresh-context review of your own change. Spawn it and name the payoff in a clause. When such a way discloses during other work, such as an edit to a README, nobody asked for the check, so propose it. Where an instrument that spawns nothing can do the job, such as a stop-hook script, lead with that. Fan-out reads, log reading, parallel edits and an artifact review have an inline substitute and need one of the three sources.

Still ask first when the delegation is not part of an invoked procedure, when it spends significant tokens outside the stated task, or when the operator has said to work solo.

The sources cover whether to delegate. You set the width, say the number before spawning, and ask past roughly half a dozen.

## Name the Payoff, Don't Assume It

A second, independently gated nudge may also be present: *"Subagents multiply cost and time… Delegate only when the payoff clearly exceeds that overhead."* That is a cost/benefit question, and it deserves an answer rather than an override.

State the payoff in one clause when you delegate:

- **Fan-out reads** — "six independent files; subagents return conclusions, not file dumps"
- **Isolation** — "review needs a reader who hasn't seen my reasoning"
- **Parallel independent work** — "three unrelated call sites, no shared state"

If you can't name the payoff in a clause, the nudge is right — do it inline.

## Pick the Model

The `Agent` tool takes a `model`. Choose it by what the worker's errors would cost:

- `haiku` for mechanical work with a checkable result: a file fan-out search, bulk edits from an exact spec, running a command and reporting its output.
- `sonnet` for bounded implementation from a brief that names the files and the check to run.
- The session's model, by omitting `model`, for review, design, and any read whose mistakes would come back as findings. A `fork` always runs on it.

A strong model on a mechanical task wastes money. A weak model on a judgment task wastes the delegation.

The sections above cover the Agent tool. `Workflow` and deep-research are named by the same sentence and stay proposals: discuss them, never invoke unprompted. The suggested CLAUDE.md line does not cover them; only a line that names them does. See ADR-175 and ADR-608.

## Harness Wrappers Are Not Verdicts

The harness emits a generic "shared-state" / "SECURITY WARNING" prefix around any subagent tool use that touches the world outside its own context. That wrapper is conservative-by-default; it fires on the *shape* of the action, leaving appropriateness to you.

Calibrate the wrapper against the agent's documented purpose:

- **code-reviewer posts a comment on the PR it was invoked to review** — that's its deliverable in GitHub-mode projects, the documented happy path. The wrapper is noise here. Don't escalate to the user as if a violation occurred.
- **A subagent does something outside its stated scope** — different story. Surface it.

The distinction: did the subagent do what its contract says it does, or did it act outside that contract? The wrapper alone doesn't tell you; the agent file does. Read the contract, then judge.

PR-comment destination is a workflow question (GitHub-mode vs. local-mode). See `agents/code-reviewer.md` for the mode breakdown.

## See Also

- environment/recovery(softwaredev) — classify a failed or wrong handback before retrying or re-briefing
- research(research) — the fan-out step that uses these investigation rules
- subagents/briefs(meta) — what a brief carries for a worker that inherits nothing
- subagents/agent-teams(meta) — several agents or sessions on one repository, and shared-checkout rules
