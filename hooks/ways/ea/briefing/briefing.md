---
description: catch me up morning briefing, what happened overnight while I was away, start of day summary across all inboxes and calendars
vocabulary: catch up morning briefing what's my day start of day need to know overnight summary today agenda priorities catch-me-up cross-source unified
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Catch-Me-Up Briefing

A briefing is one unified summary across the user's inboxes, calendar, tasks and chat, and it is the most comprehensive of the EA workflows. The **briefing** skill runs the procedure: parallel scouts, the priority structure and the suggested task changes. This way says when to run it, and when a smaller query does the job.

## When to brief

Reach for a full briefing at the start of the day, after time away, or whenever the user asks "catch me up / what did I miss" across accounts. The point is synthesis across sources — not a dump of each inbox, but the cross-referenced picture of what deserves attention now.

## When to use a team (vs. a single query)

The briefing skill can gather with parallel scout subagents or directly. Use the **team** when the briefing spans multiple accounts AND services AND benefits from cross-referencing — the scouts fetch in parallel, so a slow service does not hold up the rest, and the raw results stay out of the lead's context. **Skip the team** for a single account or service, an interactive task (drafting, scheduling), or a quick lookup. For those, starting scouts costs more time and tokens than it saves.

## Suggest, then wait for approval

A briefing ends in suggestions: a ranked list of actions and proposed task changes, which the user approves, changes or dismisses. The briefing itself never sends, schedules or changes anything. Each action happens later, after the user approves it.

## See also

- the **briefing** skill — the runnable procedure
- ea(ea) — the domain root: shared judgment
- email(ea) — per-domain judgment for mail
- calendar(ea) — per-domain judgment for the calendar
- tasks(ea) — per-domain judgment for tasks
- comms(ea) — per-domain judgment for chat
- briefing/open-items(ea) — checking for the user's own reply before listing an item as open
