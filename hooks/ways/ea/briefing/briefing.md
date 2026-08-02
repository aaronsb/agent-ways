---
description: catch me up morning briefing, what happened overnight while I was away, start of day summary across all inboxes and calendars
vocabulary: catch up morning briefing what's my day start of day need to know overnight summary today agenda priorities
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Catch-Me-Up Briefing

A unified briefing across inboxes, calendar, tasks, and chat — the most comprehensive EA workflow. The runnable procedure (parallel scouts, the priority structure, suggested task mutations) lives in the **briefing** skill. This way is *when* to reach for it, and when a lighter query serves better.

## When to brief

Reach for a full briefing at the start of the day, after time away, or whenever the user asks "catch me up / what did I miss" across accounts. The point is synthesis across sources — not a dump of each inbox, but the cross-referenced picture of what deserves attention now.

## When to use a team (vs. a single query)

The briefing skill can gather with parallel scout subagents or directly. Use the
**team** when the briefing spans multiple accounts AND services AND benefits from cross-referencing — the parallelism hides slow I/O and keeps raw fetches out of the lead's context. **Skip the team** for a single account or service, an interactive task (drafting, scheduling), or a quick lookup — there the orchestration overhead costs more than it saves.

## Surface, don't act

A briefing ends in *suggestions* — a ranked action list and proposed task mutations the user approves, modifies, or dismisses. It never sends, schedules, or mutates on its own. Acting is a separate, approved step.

## See also

- the **briefing** skill — the runnable procedure
- ea / email / calendar / tasks / comms(ea) — per-domain judgment
