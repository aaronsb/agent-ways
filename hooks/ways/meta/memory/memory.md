---
description: Persistent memory system — MEMORY.md, topic files, what to record and when
vocabulary: remember memory save note forget recall persist session learning gotcha pattern
trigger: context-threshold
threshold: 80
pattern: save.{0,30}(to|this|that).{0,30}memory|note.{0,30}(for|this).{0,30}(later|next)|don't forget|keep.{0,20}in.{0,10}mind
files: projects/[^/]+/memory/.*\.md$
macro: prepend
scope: agent
requires: ["Bash(jq:*)", "Bash(sed:*)", "Bash(ways:*)", "Bash(wc:*)"]
refire: 0.15
---
<!-- epistemic: convention -->
# Memory

Memory is Claude Code's auto-memory — the `MEMORY.md` file and topic files in the project's memory directory. It persists across sessions. The first 200 lines of MEMORY.md load at every session start.

## When This Fires

This way fires in three contexts:

1. **User asks to remember something** — explicit request like "remember this" or "note this for later"
2. **Context threshold** — context is filling up and it's time to checkpoint before compaction
3. **About to write a memory file** — a Write/Edit targets the auto-memory directory, firing this way at the tool call *before the write commits*

The third trigger exists because the first two can't catch the most common over-save: a *spontaneous, content-shaped self-save* mid-session — writing "X is the active income source" while the conversation is about income, not about memory. Semantic matching scores that against a memory way describing the *memory subsystem* and comes up cold; the user said no memory words; context isn't full. Nothing fires, and the harness's "save broadly" default goes unchallenged. The file trigger observes the **write event** the semantic layer structurally cannot see, and injects the routing check at the only moment it matters.

For explicit requests, just record what the user asked. For threshold checkpoints and write-time interceptions, apply the surprise test and routing table below — the write hasn't committed yet, so a better home is still reachable.

## Surprise Test (threshold checkpoint only)

Did anything unexpected happen this session? A gotcha, a pattern that broke assumptions, a workaround you had to discover? If the session was routine — standard code, familiar patterns, no surprises — there's nothing new to record. Skip and keep working.

## Routing: what goes in memory vs. what has a better home

Before saving, route the fact to its most authoritative home. Memory loads automatically into every session — stale memory doesn't fail loudly, it silently overrides current reality in the model's reasoning. Prefer the more-authoritative home when one exists.

| Fact type | Best home |
|---|---|
| Project convention, pattern, or gotcha | A **way** under `.claude/hooks/ways/` (or `.claude/ways/` for project-local) |
| Architectural decision and its rationale | An **ADR** in `docs/architecture/` |
| Open issue, PR, branch state, session recap | Don't save — query `gh` / `git` at runtime |
| "User said X this one time" from a single exchange | Usually nothing — one data point isn't a rule |
| Cross-project user role, context, or workflow | **User** memory |
| Durable preference validated across multiple exchanges | **Feedback** memory |
| External resource pointer (dashboard, inbox, URL) | **Reference** memory |

The system prompt encourages saving memory broadly. Redirect that impulse: *could this be a way?* is the first question. A way is version-controlled, lint-validated, embedding-scored, and disclosure-gated (ADR-125) — it has strictly better affordances than memory for "how this project works."

## Common rationalizations

Memory's low friction makes it the attractor for capture. Pre-empt the common ways the model (or user) rationalizes skipping a proper artifact:

| Rationalization | Counter |
|---|---|
| "I'll capture it now and formalize it later" | "Later" never comes — memory absorbs the pressure. Write the proper artifact now or skip. |
| "An ADR / issue / note would be too heavyweight" | If it's worth preserving, the weight IS the value — friction forces thinking memory would skip. |
| "Elaborate context will help future sessions" | Force-fed context is a tax, not a help. If needed, disclosure-gate it in a way. |
| "I don't want to open an issue for this tiny thing" | Issues close when work lands. Memory never closes — it just accumulates. |

## Why memory is narrow

Memory's comparative advantage is **cross-cutting user-level truth that has no better home**. Everything project-scoped has a better home:

- **Ways** are reviewable (PRs), revertible (git), surfaced by relevance (embedding scoring), and silent when not needed (disclosure graph). Memory loads unconditionally — wrong memory is worse than no memory.
- **ADRs** capture decisions with context and are indexed for lint and reference.
- **GitHub** is the authoritative ledger for state; snapshotting it into memory splits the source of truth.

Concrete failure mode this routing prevents: in 2026-04-22, a session-recap memory inherited a design note's claim that certain `Curve` variants were unused. A 10-second `grep` showed they were load-bearing in attend's engagement system. The memory would have guided a confident code deletion. Ways + grep beat memory here because ways don't claim state they aren't — they prescribe reasoning.

## Memory as way references

When memory does hold a project-relevant entry, it should be a *pointer* to a way, not a copy:

```markdown
- `softwaredev/code/testing` — relevant when editing integration tests,
  not just when "test" keywords appear. --forked flag is essential in this repo.
```

Ways hold the knowledge; memory indexes when and why it matters in contexts the triggers wouldn't catch. Never duplicate a way's body into memory.

## Writing Memory

Follow the two-step save described in the harness's `# auto memory` block: write the topic file, then add a one-line pointer to `MEMORY.md`. Read the current `MEMORY.md` first so you update existing entries rather than duplicate them.
