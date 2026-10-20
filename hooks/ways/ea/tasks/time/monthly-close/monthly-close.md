---
description: a monthly time-tracking close that reconciles two systems, subtracting the hours already logged in one system from a monthly target, logging the remainder as worklogs on tracker tickets by cost category, and checking that both totals add up to the target
vocabulary: monthly close month-end month end worklog worklogs reconcile reconciliation remainder target hours leave subtraction capitalize capitalizable capex opex cost category account field jira tickets finance audit two systems billable internal hours full-time
pattern: month(ly)?[- ]?(end )?close|\bworklogs?\b|\bcapex\b|\bcapitali[sz](e|able|ation)\b
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: constraint -->
# Monthly Time Close Across Two Systems

Some teams track time in two systems. Billable client hours go into a time-tracking system. The rest of the month goes into worklogs on tracker tickets, sorted by cost category. Finance closes the month from both, and the close may be audited. The two systems must add up to the target.

## The subtraction

1. Set the target. It is the expected hours for the month minus leave. Find leave in the calendar and email. Ask for the base figure if no one has stated it.
2. Read the month's hours already logged in the first system. Read them from the system, not from memory.
3. The remainder goes into the second system.
4. Check that the first system plus the second system equals the target. Check it in the proposal and again after logging.

On request, write a plain explanation of this subtraction for a manager or for finance.

## Pick tickets

- Search the user's tickets that were updated in the month.
- A category field on each ticket decides where its hours land. If the field is empty, propose a value. Set it only after the user approves.
- Follow the organization's written definition of each category. A common split puts work that builds the product in the capitalizable category. That covers code, infrastructure, CI/CD, technical research and feasibility, internal developer tools, and technical docs written during the build. Training development and general admin usually do not qualify.
- Categories get retired. Check which ones are open before logging to any of them.
- Place hours only on dates inside the target month. Base each estimate on evidence of the work, such as commits and ticket activity.

## Log

- Show one table with ticket, summary, category, hours, date, and rationale. Add a subtotal per category and a grand total.
- Get one confirmation for the whole table.
- If the connector has no worklog tool, use the tracker's REST API. Some worklog APIs take the numeric issue ID, not the key. Resolve each key to its ID first.
- Read the worklogs back. Confirm the total and the equation from step 4.
- Offer a short update to finance as a draft. Send it only when the user says to.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "I remember roughly what was logged in the first system" | Read it. The remainder is only as right as that number. |
| "The ticket's category is obvious" | The field decides. Propose a value and let the user approve it. |
| "The totals were right in the proposal" | Partial failures happen. Read back after logging and check again. |

## See Also

- tasks/time(ea) — logging gates and evidence for the first system
- trust/delegation(meta) — worklogs are writes in the human's name
