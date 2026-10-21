---
description: logging billable hours to a timesheet, putting hours on a client project for a day, time entries by client and project written through the time system's tool, catching up on unlogged time, preparing an invoice
vocabulary: timesheet timesheets billable hours time entry time entries unlogged time hours worked per client invoice invoicing client engagement kantata harvest toggl clockify billing unbilled uninvoiced utilization auto-log weekly put fill in week yesterday submit duplicate
pattern: \blog (my )?(time|hours)\b|\bput \d+(\.\d+)? ?(hours|hrs|h)\b|\btime entr(y|ies)\b|\btimesheets?\b
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Time Tracking & Billing

Log billable hours against clients and projects. Always confirm before logging.

## Logging Workflow

1. **Review what was worked on.** Check the session's activity:
   - Emails drafted or sent (which client/project?)
   - Meetings attended (from calendar)
   - Tasks completed or worked on
   - Other work discussed in conversation
2. **Match to clients and projects.** Map each work item to a client and optionally a project.
3. **Propose time entries.** Present a summary:

```
Suggested time entries for today:
  [Client A] / [Project] — 1.5h — "Description of work"
  [Client B] / [Project] — 2.0h — "Description of work"
  Personal / Admin — 0.5h — "Email triage, calendar management"
Log these? Adjust any?
```

4. **Log on approval only.** Never auto-log.

## Writing Entries Through a Tool

When the time system is reached through an API, an MCP server or a CLI:

- Resolve each date to YYYY-MM-DD in the time zone the system uses.
- List the existing entries for a date before writing to it. Hand-entered time and leave are common, and a duplicate costs a correction in the system.
- Dry run the write where the tool offers one, then commit, then list the date again and report what landed. A commit can partly fail.
- Submitting a week is its own step, taken only on the human's explicit ask. List the whole period first and flag weekdays with gaps. Submission often locks the period.
- When the tool cannot update, delete or submit, say so and point to the system's own interface. Pass account-policy errors to the human.

## Weekly Time Review

When doing a weekly review, include:
- Total hours by client and project
- Comparison to typical week (if historical data available)
- Hours logged vs. calendar time (utilization)
- Uninvoiced time that may need billing

## Invoicing

Invoice creation requires explicit confirmation. Always present:
- Billing period
- Total hours
- Line items by project
- Calculated amount

before creating any invoice.
