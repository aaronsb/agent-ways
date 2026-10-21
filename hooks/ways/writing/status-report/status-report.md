---
description: writing a recurring progress report such as a weekly status for a client, a steering committee or leadership update, a work journal, or a stakeholder summary of what shipped since a date; read the latest meeting notes and commits first and follow the structure and length of the last approved report
vocabulary: weekly status weekly update weekly report friday update status report progress report program report steering committee leadership vps stakeholder summary red amber green rag work journal what moved this week what shipped since kickoff last week's report same layout sponsor
pattern: status (report|update)|weekly (update|report)|stakeholder summary|git history (report|summary)|what did we (do|accomplish)|features (developed|built|shipped) since
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Recurring Status Reports

A recurring report is read against the previous one. Before drafting, open the last approved instance and follow its structure, length, depth and markdown. Change the format when the user asks, and then update the template or skill the report is built from so the next one follows the change.

## Gather first

- Read the records since the last report before writing: meeting transcripts, messages, tracker changes and the git log for the window. Memory misses the latest meeting.
- Refresh cumulative chart data before redrawing the chart (writing/deliverables).
- Where a point needs proof, add a short parenthetical link to the transcript, decision record or commit behind it (writing/provenance).

## What to report

- What changed, what is blocked and by whom, and the next action with its owner.
- Capability delivered and its adoption over time. Lines of code, seat counts and days active tell the reader little.
- For readers outside the team, plain statements of outcome, with no internal tool names or tracking sheets.

## Summaries from git history

- Rebuild the account from the full log for the window: commit messages, dates, branches and merges. For "everything since" a date, list every feature since that date. For the whole project, cover the whole history unless the user names a window.
- Return it in chat unless the user asks for a file.
- When asked for the story, explain the decisions behind the changes and how they connect.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "A fresh format would read better" | The reader expects last week's shape. Change it when the user asks. |
| "I remember what happened this week" | Read the transcripts and messages first. |
| "Lines of code shows progress" | Readers cannot judge it. Report capability and adoption. |

## See Also

- writing(writing) — parent: the status update genre
- writing/provenance(writing) — linking each point to its record
- comms/recap(ea) — the transcripts a status report draws on
