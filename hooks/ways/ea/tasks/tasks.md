---
description: personal task management, action items, obligations, task lifecycle, create update complete cleanup tasks
vocabulary: task action item to-do obligation track create complete update overdue pending priority due date assign stale cleanup eisenhower
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: constraint -->
# Task Lifecycle

Personal task management as the persistence layer closest to the user's brain. Tasks track obligations, deadlines, and commitments across all contexts.

## Core Rule: Suggest, Never Auto-Execute

Every task mutation must be suggested and approved. Present the proposal, wait for confirmation, then execute.

## Suggesting Task Creation

When any workflow surfaces a new obligation:

```
Suggested task: "Follow up with [person] on [topic]"
  Priority: medium
  Due: YYYY-MM-DD
  Source: email / meeting / chat / phone
  Client: [if applicable]
  Project: [if applicable]
Create this task?
```

Always populate: **title** (verb + object), **source type and reference** (creates traceability), **priority** (infer from urgency cues), **due date** (from content — don't fabricate if none mentioned).

For multiple items, present as a numbered list and offer bulk creation on approval.

## Suggesting Task Updates

When intelligence indicates properties should change:

```
Suggested update to "[task title]":
  Priority: medium → high (meeting is tomorrow)
  Due: none → YYYY-MM-DD
Update this task?
```

Triggers: email sets/changes a deadline, meeting moves up, blocked task becomes unblocked, user starts working on something.

## Suggesting Task Completion

When intelligence indicates a task is done:

```
Looks resolved: "[task title]"
  Evidence: [what indicates completion — sent email, meeting passed, explicit confirmation]
Mark as complete?
```

Signals: user sent a fulfilling email, tied calendar event has passed, user explicitly says "that's done."

## Suggesting Task Cleanup

During briefings and reviews, identify stale tasks:

```
Possibly stale (no activity in 14+ days):
  "[task title]" — created [date], no related activity since
Archive, reschedule, or keep?
```

Triggers: no due date + 14 days old + no related activity. Due date 7+ days past with no updates. References a project with no recent activity.

## Cross-Reference Pattern

Every intelligence-gathering workflow should cross-reference with tasks:
- What emails relate to open tasks?
- What tasks relate to upcoming meetings?
- What's been resolved but not marked complete?
- What new obligations aren't yet tracked?

## See Also

- trust/delegation(meta) — task mutations are delegated actions
- tasks/time(ea) — time tracking against tasks
