---
description: schedule a meeting, check my availability, block time on my calendar, create calendar events, find a free slot
vocabulary: schedule calendar availability block time event meeting invite reminder reschedule free busy slot book appointment timezone deadlines cross-account multi-account scheduled colleague coworker shared-meetings attendee private hold move delete rsvp focus-time window hold-a-window
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

## Reading Other People's Calendars

- In a shared workspace, such as one Google Workspace or Microsoft 365 tenant, you can often read a colleague's calendar by using their email address as the calendar ID. Use it to see their day, find open slots, check whether an invite already exists, or review past meetings.
- Event search often matches attendee addresses as well as titles. Open one person's calendar and search for a second person's address to list the meetings they share.
- Private events show only as busy. Do not guess what they are.

## Reading the Record Carefully

- An untitled busy block is a hold, not a meeting. Do not report it as one.
- An invite, or even an acceptance, does not prove the user attended. Confirm attendance from a transcript, notes, or a follow-up before you treat a meeting as held.

## Changing Events

- Never move or delete an event without an explicit yes for that event.
- An RSVP sends a message to the organizer. Treat it like any other send and wait for approval.

## Reminders for Future Deadlines

For important deadlines, set layered reminders:
- 1 week before
- 1 day before
- Morning of (if applicable)

## Timezone

Default to the user's local timezone unless specified otherwise. Always include timezone in event creation.

## Cross-Account Awareness

When scheduling, check ALL accounts for conflicts — the user may have personal events that block professional availability and vice versa.

## See Also

- intelligence(ea) — meeting prep built on the calendar
- comms/recap(ea) — transcripts that show whether a meeting happened
