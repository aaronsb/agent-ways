---
description: logging time through a time-tracking system's API, MCP server, or CLI, mapping each client to its project and task, dupe-checking the date, proposing a per-day table and confirming it, dry running, committing, reading entries back, and submitting a week only on an explicit ask
vocabulary: time-tracking api connector mcp cli log time time entry time entries timesheet workspace project task mapping table client dupe-check duplicate plan dry-run commit read back submit week submission period gap days memo minutes notes sso login expired session policy error transcript duration
pattern: \blog (my )?(time|hours)\b|\blog (yesterday|today|last week|monday|tuesday|wednesday|thursday|friday)\b|what have i logged|submit (my |the )?(timesheet|week)|\btime entr(y|ies)\b
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Logging Time Through an API

tasks/time holds the gates and the evidence sources. This way holds the mechanics of writing entries through a time-tracking API, MCP server, or CLI.

## Before anything

- Read the project's notes for the time system first. Look for a table that maps each client to its project and task, any split rules, and a troubleshooting list. If no table exists, start one as you resolve clients, and keep it.
- Many connectors ride a browser login. On an auth error, start the login flow yourself, let the user finish single sign-on, and retry.
- Resolve every date to YYYY-MM-DD in the time zone the time system uses.

## Evidence for a day

- Meeting transcripts are the best evidence. A transcript's header names the meeting, and its end line or timestamps give the real length. Prefer that length over the calendar slot.
- Doc commits right after a meeting count as documentation time, from the meeting's end to the commit time.
- Ignore untitled busy holds on the calendar unless something else backs them up.
- Internal chat about a client is not billable to that client. Use it to rule time out.

## Propose, then confirm

Show one table per day with date, project, hours (rounded by the user's rule), memo, and the evidence for each row. Get one confirmation for the batch before any write.

## Log

1. Map each client to its project and task from the table. For an unknown client, search the system's accounts and tasks, then add the result to the table.
2. List existing entries for the date. The user may have entered some by hand, including leave.
3. Plan each entry with a duration and a note.
4. Dry run the write, then commit it.
5. List the date again and report what landed.

Apply any split rule in the table, such as one client split evenly across two projects. Log only time that belongs in this system.

## Review and submit

- A review groups entries by client and task, with billable and submission status. Flag weekdays with gaps, duplicates, and stale placeholders.
- Know the tool's limits. If the connector cannot update, delete, or submit, say so. Point the user to the UI or to the documented API call.
- Submit only on an explicit ask. Use the system's submission period, such as Sunday to Saturday, and submit once per project or workspace if the system requires it. First list the whole week, flag gap days that show evidence of unlogged work, and confirm. Afterward, read the entries back and check that they show as submitted.
- Pass account-policy errors to the user. Some need an admin.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "The date is clear, no need to list first" | Hand-entered time is common. A duplicate costs a correction in the UI. |
| "The dry run looked fine, skip the read back" | The commit can partly fail. List the date and report what landed. |
| "The week looks complete, submit it" | Submitting locks the week. Wait for the explicit word. |

## See Also

- tasks/time(ea) — logging gates, evidence sources, and rounding
- trust/delegation(meta) — time entries are writes in the human's name
