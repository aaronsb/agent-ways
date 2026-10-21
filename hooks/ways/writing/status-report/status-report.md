---
description: recurring progress artifacts such as a weekly client status report, a daily work journal, a stakeholder summary built from git history, or a git history report returned in chat, each patterned on the last approved instance
vocabulary: status report weekly status weekly update client update stakeholder summary progress report daily journal work log git history git log accomplishments burnup burndown leadership update team share features since what did we accomplish mermaid branch diagram recurring last-approved template
pattern: status (report|update)|weekly (update|report)|stakeholder summary|git history (report|summary)|(full|complete|entire) git history|what did we (do|accomplish)|features (developed|built|shipped) since
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Status Reports and History

Before drafting, open the last approved instance and match its structure, length, depth, and markdown. Do not invent a new format.

## Weekly client status

1. Ingest new transcripts and messages first, so the report reflects the latest meetings.
2. Pattern the report on last week's.
3. Refresh any cumulative chart data, such as a burnup, and put the visual at the top.
4. Write short bullets of plain declarative sentences that follow the user's register rules. Where a point needs proof, add a parenthetical link to the transcript or decision record behind it.
5. Commit, push, or post the link only when the user or the routine names that step. Posting to a channel follows trust/delegation.

When the user changes the standard, update the report skill or template so the next report follows it by default. Stale instructions produce stale reports.

## Daily work journal

Review the day's agent sessions, messages, and any pasted transcripts. Match the earlier entries in the journal folder in shape and depth. Commit only when the user or the routine names it.

## Stakeholder summaries

Build them from git history. Cover the full project lifecycle unless the user asks about one day.

- Write 3 to 8 flat, single-sentence plain statements for readers who do not know the tools.
- Use no `**Label:** text` prefixes.
- For an internal team share, write one lead line, such as "Used <tool> to:", then one bullet per outcome. Do not reference internal tracking sheets.

## Git history reports

- Rebuild progress from the full log for the requested window. Use commit messages, timestamps, branches, merges, and code volume. For a "features since" request, list every feature developed since that date. Return it in chat and edit no files.
- Add a Mermaid branch diagram when it helps, or a `.mmd` file if asked. Render it and check it before delivering. On GitHub, use `<br>` rather than `\n` in labels and escape angle brackets.
- When asked for the story of the history, explain the decisions behind the changes and how they connect to earlier and later ones. Close with a one-paragraph status that names the current branch.

## Metrics

| Use | Avoid |
|---|---|
| Adoption over time | Seat counts |
| Before-and-after capability | Median days active, lines of code |

External audiences get percents and relative timelines. Internal records still use literal YYYY-MM-DD dates.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "A fresh format would read better" | The reader expects last week's shape. Change it only when the user asks. |
| "I remember what happened this week" | Ingest the transcripts and messages first. Memory misses the latest meeting. |
| "Lines of code shows progress" | Readers cannot judge it. Report capability and adoption. |

## See Also

- writing(writing) — genre rules for status updates
- comms/recap(ea) — the transcripts a status report draws on
- mermaid(documentation) — diagram syntax and rendering
