---
description: writing a batch of changes into a system of record on the user's behalf, such as updating every contact in a CRM, shifting the due dates on all open tasks in a project tracker, or editing rows in a finance system or shared tracking spreadsheet, proposing the whole batch as one table, confirming it once, and reading the records back before reporting done
vocabulary: crm salesforce hubspot asana jira monday airtable tracker spreadsheet sheet records rows contacts tasks deals bulk batch mass update edit import every all open set change field owner status stage due date preview dry run confirm read back verify totals duplicates
pattern: \bsystems? of record\b|\bbulk (update|edit)s?\b|\bread.?back\b
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: constraint -->
# Writes to Systems of Record

A CRM, finance system, tracker or shared sheet belongs to the user and their team, and each write carries the user's name (trust/delegation). These rules apply when one request changes many records. GitHub pull requests and issues are outside this way; ADR-606 governs them.

## Read-only until the change is named

When the ask is analysis, return the report and change nothing. Write only the change the user named.

## One batch, one confirmation, one read-back

1. Read the target records first, and check for duplicates of anything you would create. People also enter records by hand.
2. Resolve relative dates such as "Monday" or "next sprint" to calendar dates. Where the source gives no date, leave the field empty.
3. Show the whole batch as one table: each record, the field, its old and new value, and where the new value came from. Give totals where the batch carries amounts. List anything discussed but left out, with the reason.
4. Use the tool's preview or dry-run mode when it has one.
5. Get one confirmation for the batch, then run it in one pass.
6. Read the written records back with a real read of those records, not a status or auth call. Report the verified count and totals.

## Shared sheets

Find a row by its ID column, never by its row number. Rows move when people sort or insert.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "The API returned 200, so it worked" | Read the records back. Partial writes and silent defaults happen. |
| "I'll confirm each row as I go" | One table and one confirmation shows the whole batch and its totals. |
| "Row 14 is the item they meant" | Rows move. Use the ID column. |
| "They asked for analysis, but a small fix is fine" | Analysis means a report. Name the fix in it. |

## See Also

- trust/delegation(meta) — parent: every write uses the human's borrowed access
- tasks/time(ea) — time entries follow the same table and read-back
