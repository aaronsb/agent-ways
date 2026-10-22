---
description: acting for the user inside their Harvest, Toggl, Clockify or Kantata account through its connector, mcp or cli; looking at what is already on a day before adding to it, dry running, reading back what landed, and locking in the week only once they have had a look
vocabulary: connector mcp cli on my behalf for me in my account log my time book my time fill in my week from my calendar go ahead once I've had a look dry-run read back what have i logged hand-entered pto leave submit it for me approver
refire: 0.15
scope: agent, subagent
pattern: \b(harvest|toggl|clockify|kantata)\b
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
