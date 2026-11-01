---
name: briefing
description: Produce a "catch me up" briefing — a synthesized start-of-day / overnight summary across email, calendar, tasks, and chat, ending in suggested next actions. Use when the user says "catch me up", "what did I miss", "morning briefing", "what's my day", "start of day". Not for acting on findings (sending, scheduling, creating tasks) — it surfaces and suggests; you approve. Not for single-source lookups or deep person/meeting prep.
---

# Catch-Me-Up Briefing

Synthesize one prioritized brief across the user's inboxes, calendar, tasks, and chat. The job is **surface and suggest** — present what needs attention and a ranked list of suggested actions; the user decides. Never send, schedule, or mutate anything without approval.

This skill is the runnable procedure; the **briefing** way carries *when* to reach for it and when a single-source query is enough instead.

## Tool-agnostic by design

Map the scout *domains* below to whatever accounts and services are actually connected this session — email, calendar, task, and chat tools vary per setup, so discover them rather than assuming a provider. If a domain has no connected tool, **say so in the brief** rather than skipping it silently.

This skill deliberately sets **no `allowed-tools`**: it must reach whatever EA tools are connected and spawn scout subagents, which can't be enumerated ahead of time. Its tight description is what keeps it in its lane.

## Gather in parallel

For a multi-account, multi-service briefing, spawn parallel scout subagents so the slow I/O overlaps — and so each scout's raw fetch stays out of the lead's context:

| Scout | Domain | Shares with |
|---|---|---|
| inbox-scout | email (all accounts) — fetch, classify, filter | people/topics → ops-scout |
| ops-scout | calendar + tasks — schedule, status, cross-ref vs. inbox | the lead |
| + specialists | chat, meeting recaps, files — as connected | the lead |

Keep it to 3–5 scouts. Scouts share cross-reference-worthy findings as they go; each returns one structured report; the lead waits for **all** reports before synthesizing. **Skip the team** for a single account/service or a quick query — just gather directly (that judgment lives in the **briefing** way).

## Synthesize (priority order)

1. **Overdue** — past-due tasks, priority-ranked; broken commitments first.
2. **Today's schedule** — events, each with related task count + email context.
3. **Due today** — tasks due today, not yet complete.
4. **Action required** — emails awaiting reply, urgency-ranked; note cross-refs.
5. **Already addressed** — replied items; flag any that resolve open tasks.
6. **Open tasks in play** — pending tasks linked to today's mail/calendar.
7. **Eisenhower snapshot** — one line (X do-first, Y to schedule, Z in inbox).
8. **Cross-references** — key connections found across mail, tasks, calendar.
9. **Suggested actions** — what to tackle first, given calendar constraints.

## Then: suggested task mutations

From the cross-references, surface a numbered list the user can **approve, modify, or dismiss** — never applied automatically:

- **Create** — obligations from email not yet tracked.
- **Complete** — tasks the sent mail appears to resolve.
- **Update** — tasks whose priority/due date shifted.
- **Stale?** — tasks not referenced in recent comms.

## Not for

- Acting on findings — sending replies, scheduling, creating/closing tasks. It surfaces and suggests; the user approves each.
- Single-source lookups — just query that inbox/calendar directly.
- Deep person/topic/meeting preparation — that's the **intelligence** way's cross-referencing research, a separate workflow.

## See also

- the **briefing** way — when to brief vs. when a single query suffices
- the **ea** / **email** / **calendar** / **tasks** / **comms** ways — per-domain judgment
