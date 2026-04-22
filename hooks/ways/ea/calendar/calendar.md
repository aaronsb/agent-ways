---
description: schedule a meeting, check my availability, block time on my calendar, create calendar events, find a free slot
vocabulary: schedule calendar availability block time event meeting invite reminder reschedule free busy slot book appointment timezone
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Calendar Management

Create, check, and manage calendar events across accounts.

## Workflow

1. **Check availability** before creating any event. Query all relevant accounts for conflicts in the proposed time range.
2. **Surface related tasks.** When reviewing upcoming events, match open tasks by client, project, or attendee names. "You have N open tasks related to [Client] before your 2pm with them."
3. **Present event details** before creating. Always confirm: summary, start/end with timezone, description, attendees, reminders.
4. **After meeting creation or review**, suggest task capture. When a meeting just ended or was created, ask if action items should be captured.

## Reminders for Future Deadlines

For important deadlines, set layered reminders:
- 1 week before
- 1 day before
- Morning of (if applicable)

## Timezone

Default to the user's local timezone unless specified otherwise. Always include timezone in event creation.

## Cross-Account Awareness

When scheduling, check ALL accounts for conflicts — the user may have personal events that block professional availability and vice versa.
