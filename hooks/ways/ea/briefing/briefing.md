---
description: catch me up morning briefing, what happened overnight while I was away, start of day summary across all inboxes and calendars, or catching up on a project since the last session or commit
vocabulary: catch up morning briefing what's my day start of day need to know overnight summary today agenda priorities catch-me-up cross-source unified scouts preflight window cutoff since last session last commit ingest transcripts awaiting slipped
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Catch-Me-Up Briefing

A unified briefing across inboxes, calendar, tasks, and chat — the most comprehensive EA workflow. The runnable procedure (parallel scouts, the priority structure, suggested task mutations) lives in the **briefing** skill. This way is *when* to reach for it, and when a lighter query serves better.

## When to brief

Reach for a full briefing at the start of the day, after time away, or whenever the user asks "catch me up / what did I miss" across accounts. The point is synthesis across sources — not a dump of each inbox, but the cross-referenced picture of what deserves attention now.

## When to use a team (vs. a single query)

The briefing skill can gather with parallel scout subagents or directly. Use the
**team** when the briefing spans multiple accounts AND services AND benefits from cross-referencing — the parallelism hides slow I/O and keeps raw fetches out of the lead's context. **Skip the team** for a single account or service, an interactive task (drafting, scheduling), or a quick lookup — there the orchestration overhead costs more than it saves.

## Running a briefing

1. **Preflight auth first.** Check every account and service the briefing needs, in parallel, and show a status table. A stale login found halfway through wastes the whole run.
2. **Set the window.** Use the time since the last briefing, or roughly the last half day if there is no record. Compute today's start and end in the user's timezone.
3. **Split the scouts by source.** A typical split is one scout for every mail account, one for chat, and one for every calendar. Scouts only read. Let them share the people and topics they find, so the calendar scout can match a meeting to the thread the inbox scout found.
4. **Keep cheap data in the lead.** If a preflight check already returned useful data, such as the user's recent tickets, the lead keeps it and no scout fetches it again.
5. **Wait for every scout** before writing the briefing. A briefing built from half the sources misses the cross-references.

## Order of the briefing

1. Today's meetings, each with its context.
2. Mail and messages awaiting a response from the user.
3. Ticket and project activity.
4. Chat highlights.
5. Items the user already handled, one line each.
6. Cross-references between sources.
7. Suggested actions.

## Check before calling anything open

Before you list an item as open, look for the user's own reply in sent mail or the chat thread. An item the user already answered is not open. Flag items that slipped through, such as a direct question with no reply after several days.

After the briefing, work open items one at a time. Suggest replies. Send nothing.

## Catching up on a project

Inside a project or engagement folder, "catch me up" or "pull in anything new" means a scoped catch-up.

- Find the date of the last session or the last commit. That date is the cutoff.
- Scan chat (threaded replies included), the related inbox, and the calendar from the cutoff on. Pull any new meeting transcripts.
- Fold new material into the project's documents, such as the decision log, commitments, and timeline. Do not just drop raw files in the folder. Do this before action items unless the user says otherwise. Some sessions are ingest only.
- Check earlier sessions for unfinished or uncommitted work. Leave out work streams that another session owns.
- Return a flat bullet list that reads in a minute: what happened, and what needs the user.

## Surface, don't act

A briefing ends in *suggestions* — a ranked action list and proposed task mutations the user approves, modifies, or dismisses. It never sends, schedules, or mutates on its own. Acting is a separate, approved step.

## See also

- the **briefing** skill — the runnable procedure
- ea(ea) — the domain root: shared judgment
- email(ea) — per-domain judgment for mail
- calendar(ea) — per-domain judgment for the calendar
- tasks(ea) — per-domain judgment for tasks
- comms(ea) — per-domain judgment for chat
- comms/recap(ea) — pulling transcripts into a project catch-up
- subagents(meta) — briefing the scouts
