---
description: writing changes into a system of record such as a CRM, finance system, issue tracker, time tracker, or shared tracking spreadsheet, staying read-only until the user names the change, proposing the full batch as one reviewable table, confirming once, executing, and reading the result back from the real API
vocabulary: system of record crm salesforce hubspot finance erp tracker spreadsheet sheet row bulk update batch write records dry-run preview confirm read-back verify totals pause gate irreversible production dns token action tracker item checked-off waiting-on export data drop stale rename audit append
pattern: bulk (update|edit|close)|action (item )?(tracker|sheet)|\bitem #\d+|system of record|read.?back
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: constraint -->
# Writes to Systems of Record

## Read-only by default

Treat CRMs, finance systems, trackers, file storage, and calendars as read-only. Write only when the user names the specific change. "Commit the findings" means the repo, never the CRM. When the ask is analysis, return a report and change nothing. Messages to people follow trust/delegation.

## Propose, confirm, execute, read back

1. Dupe-check against what is already there. People enter records by hand in the UI.
2. Resolve relative dates such as "yesterday" or "Monday" to YYYY-MM-DD. If the source gives no date, do not invent one.
3. Show the complete batch as one table. Give the date, target, amount, description, and the source of each number. Add subtotals.
4. Aim for the fewest changes that capture the request. Add a "Discussed but not added" list with a reason for each.
5. Get one explicit confirmation. Re-read the target records, then execute in one pass.
6. Read back through a real API call, not an auth or status command. State the verified totals before saying done. For documents, re-read after the update.

Use preview and dry-run modes first when the tool has them. For bulk changes, such as retagging records or cleaning a backlog, return an estimate or an audit and iterate on it before running anything.

## Pause gates

Irreversible steps get a marked PAUSE in the plan. Examples are DNS or mail routing changes, production writes, credential swaps, and creating items in a client's tracker. Present a shortlist and create only the approved set. Walk through before-state checks and wait for an explicit go.

## Shared tracking sheets

- Find items by their ID column, never by row index. Rows move when people sort or insert.
- For "item #N", research it and state the minimum really needed.
- Act on the decision in the real channels, such as email, chat, or calendar. Then update the item's progress and waiting-on fields, check it off, and read the sheet back. Done in chat is not done.
- Work open items one at a time. Mention stale or related items as you go.

## New data drops

Check a fresh export's coverage period and balances against the figures the user knows. Rename it to the folder's convention, remove the stale version it replaces, and report what changed before analyzing it.

## Notes and logs

Append to notes and logs. Never rewrite the whole file.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "The API returned 200, so it worked" | Read the records back. Partial writes and silent defaults happen. |
| "I'll confirm each row as I go" | One table and one confirmation lets the user see the whole batch and its totals. |
| "Row 14 is the item they meant" | Rows move. Use the ID column. |
| "The user asked for analysis, a small fix is fine" | Analysis means a report. Name the fix and wait. |

## See Also

- trust/delegation(meta) — every write uses the human's borrowed access
- tasks/time(ea) — time entries follow the same propose and read-back flow
