---
description: catch me up morning briefing, what happened overnight while I was away, start of day summary across all inboxes and calendars
vocabulary: catch up morning briefing what's my day start of day need to know overnight summary today agenda priorities
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Catch-Me-Up Briefing

Unified morning briefing using parallel data gathering for speed. This is the most comprehensive EA workflow — it combines email triage, calendar review, task status, and chat scanning into a single synthesized brief.

## When to Use Agent Teams

For multi-account, multi-service briefings, spawn parallel agents to work simultaneously. Each agent handles a domain and shares cross-reference-worthy findings with others in real-time.

**Use a team when:** workflow spans multiple accounts AND services AND benefits from cross-referencing.

**Skip teams for:** single-account lookups, interactive workflows (drafting, scheduling), quick queries.

## Standard Roles

| Role | Domain | Responsibility |
|------|--------|----------------|
| **inbox-scout** | Email (all accounts) | Fetch, classify, filter; share people/topics with ops-scout |
| **ops-scout** | Calendar + Tasks | Schedule, task status, cross-reference against inbox findings |
| **Lead (you)** | Synthesis | Combine reports, present to user, suggest actions |

For workflows involving additional platforms (chat, file storage, meeting recaps), add specialist scouts. Keep total to 3-5.

## Communication Rules

- Scouts proactively share cross-reference-worthy findings as they work — don't wait until done.
- Each scout sends a structured final report to the lead.
- Lead does NOT start synthesis until all scouts have reported.

## Briefing Structure

Present the synthesized briefing in priority order:

1. **Overdue** — Tasks past due, ranked by priority. Broken commitments come first.
2. **Today's Schedule** — Calendar events with related task count and email context per meeting.
3. **Due Today** — Tasks due today not yet complete.
4. **Action Required** — Emails awaiting response, ranked by urgency. Note task/calendar cross-refs.
5. **Already Addressed** — Messages already responded to. Flag any that resolve open tasks.
6. **Open Tasks in Play** — Pending tasks with connections to today's emails and calendar.
7. **Eisenhower Snapshot** — One-line summary (X do-first, Y to schedule, Z in inbox).
8. **Cross-References** — Key connections found between emails, tasks, and calendar.
9. **Suggested Actions** — Prioritized list of what to tackle first, considering calendar constraints.

## After the Briefing

Surface suggested task mutations based on cross-reference findings:
- **Create:** New obligations from emails not yet tracked
- **Complete:** Tasks that sent emails appear to resolve
- **Update:** Tasks whose priority or due date changed
- **Stale?** Tasks not referenced in recent communications

Present as a numbered list the user can approve, modify, or dismiss.
