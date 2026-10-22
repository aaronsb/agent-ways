---
description: writing time entries through the time system's api, connector or cli, checking a date for entries already there, dry running and reading back what landed, and submitting or locking a timesheet period only once the human confirms the hours
vocabulary: api connector mcp cli programmatically post push write fill in my hours toggl harvest clockify dry-run commit read back list existing duplicate duplicates hand-entered leave lock locks submit submitted submission pay period policy error
pattern: \bsubmit (my |the |this )?(timesheet|time ?sheet|week|hours)\b|\btime[- ]?(tracking|sheet)? ?(api|connector|cli)\b|\bput \d+(\.\d+)? ?(hours|hrs|h)\b|\bfill (in|out) (my |the )?(week|timesheet|hours)\b|\blog (my )?(time|hours)\b|what have i logged
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Writing Time Entries Through a Tool

When the time system is reached through an API, an MCP server or a CLI:

- Resolve each date to YYYY-MM-DD in the time zone the system uses.
- List the existing entries for a date before writing to it. Hand-entered time and leave are common, and a duplicate costs a correction in the system.
- Dry run the write where the tool offers one, then commit, then list the date again and report what landed. A commit can partly fail.
- When the tool cannot update, delete or submit, say so and point to the system's own interface. Pass account-policy errors to the human.

## Submitting

Submitted time is hard to undo, and submission often locks the period. Leave logged entries unsubmitted until the human confirms the hours. A request to log and submit covers both steps, but propose the entries first when the hours are estimates. Before submitting a week, list the whole period and flag weekdays with gaps.

## See Also

- tasks/time(ea) — parent: proposing entries and logging on approval
- trust/delegation(meta) — submitted time carries the human's name
