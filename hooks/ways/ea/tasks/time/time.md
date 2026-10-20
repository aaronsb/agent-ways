---
description: logging billable hours to a timesheet, time entries by client and project, rebuilding hours from calendar, commit, and session evidence, catching up on unlogged time, submitting a week, preparing an invoice
vocabulary: timesheet timesheets billable hours time entry time entries unlogged time hours worked per client invoice invoicing client engagement kantata harvest toggl clockify billing unbilled uninvoiced utilization auto-log weekly reconstruct evidence attended meetings git commits session timestamps rounding submit lock week leave memo
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Time Tracking & Billing

Log billable hours against clients and projects. Always confirm before logging. Submitted time is hard to undo.

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

## Rebuilding Past Days From Evidence

When the user asks to log a past day or week, rebuild it from evidence. Do not rely on their memory or yours.

- Resolve "yesterday", "Monday", or "last week" to literal YYYY-MM-DD dates before any query or write.
- Count meetings the user attended. An invite or an accepted RSVP is not attendance. Make a best guess on attendance and do not ask meeting by meeting.
- Use chat activity, clusters of agent session timestamps, and commits in the repos that belong to each client.
- Find leave and sick days in the calendar and email.
- Decide which repos and folders count as work for which client. A naming convention for project folders makes this mechanical. Add only the exceptions the user names, and leave out side projects.
- When the user reclassifies work, accept it, say what changes, and update the totals.

Rules for the proposal:

- Every entry needs a description. If no evidence supports one, ask.
- Log only the projects the user named.
- Apply the rounding rule the user sets, such as rounding each day up to the whole hour.
- Flag implausible days, such as a single day well past a full workday, and cap per-day totals when scaling.

## Gates

- Never start a timesheet or invoicing routine on your own. The user starts it.
- On approval, dry run if the system supports it, log for real, then read the entries back.
- Leave logged entries unsubmitted. Submit or lock a period only on an explicit word such as "submit". "Log and submit" counts, but still propose first when the hours are estimates.
- A request to hold means stop until the user comes back.

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

- Every line item needs evidence from meetings, commits, or session timestamps in the client's repos.
- Apply the client's billing rules, such as a minimum charge per meeting. Do not count work inside a billed meeting window a second time.
- Draft the delivery email in the thread of the last invoice email. Match its format, recipients, and CCs. It stays a draft until the user says to send it.

## See Also

- tasks(ea) — tasks are the work items time is logged against
- calendar(ea) — the main source of attended meetings
- trust/delegation(meta) — submitted time and sent invoices carry the human's name
