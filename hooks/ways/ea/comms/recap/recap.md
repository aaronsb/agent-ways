---
description: meeting recaps, catching up on a meeting you missed and what was decided and who owns which follow-up, transcripts, AI-generated meeting summaries, action items from meetings, recording playback, saving a transcript verbatim into a project repo, fixing speaker attribution, mining decisions with line citations, and keeping records append-only
vocabulary: recap missed skipped sync standup decided owner who-owns-what transcript summary meeting notes recording action items discussed what happened review minutes speaker chapters follow-up speaker-attributed mentions lazy-loading scroll recurring verbatim ingest diarization speaker-labels misattribution vtt decisions commitments register next-steps append-only supersede line-citation
pattern: what did (they|we) (actually )?(decide|agree)|pull (down )?the transcript|transcript tab|\bdiariz|misattribut|speaker labels
files: (^|/)transcripts?/|\.vtt$
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Meeting Recaps & Transcripts

Access meeting recaps, AI-generated summaries, transcripts, and extracted action items from past meetings.

## What's Available

Meeting platforms often provide structured post-meeting content:

| Content | Description |
|---------|-------------|
| **AI Summary** | Chapter-by-chapter summary with key discussion points |
| **Follow-up Tasks** | Auto-extracted action items with assignees |
| **Transcript** | Full speaker-attributed text with timestamps |
| **Notes** | Shared meeting notes |
| **Mentions** | @-mentions from the meeting |
| **Recording** | Video/audio with playback controls |

## Using Transcripts

Transcripts are gold for understanding what was actually discussed vs. what made it into email. When someone references "what we talked about in the meeting," pull the transcript.

- Transcripts are speaker-attributed with timestamps.
- Long transcripts may lazy-load — scroll/paginate to get complete content.
- Search within transcripts when looking for specific topics.

## Saving Transcripts Into a Project

On a project that keeps records, the transcript is the primary record of decisions. Other documents cite it by path and line.

- Take only the transcript. Leave out the AI summary and notes, because they are interpretation.
- Copy the text through a script that writes a file. Never retype it.
- Save it as `.md` in the repo's transcripts folder with the repo's naming, such as date and meeting type. Mark internal-only meetings in the name if the repo does.
- Convert caption files such as `.vtt` to `.md`. Collapse them so only speaker changes are marked.
- If no transcript exists, say so and offer a local transcription tool if one is set up.
- The raw copy stays verbatim. Do not rewrite, summarize, re-punctuate, or fix errors in it. Put corrections in a separate, labeled copy.

## Attribution

Assume speaker labels are wrong. Labels often swap speakers, even for the person who presented. Infer speakers from the flow of the talk, the AI summary, related chat and email, and the known attendee list. Ask targeted questions where it stays unclear. Do not create items from remarks about a different client or project. Flag internal or sensitive remarks before anything reaches an outside reader.

## Mining

When asked, extract decisions (agreed and not agreed) and per-person action items, each citing transcript lines. Post-ingest impact analysis has four parts:

1. What was discussed, including decisions and commitments.
2. New information to record.
3. Existing information to update, and how the picture changes.
4. Referenced artifacts that are missing.

Propose concrete record changes and update only after approval.

## Append-Only Records

| Record | Rule |
|---|---|
| Raw inputs, frozen or submitted copies, originals | Never edit. Label frozen copies as frozen |
| Registers | Add the next numbered entry and cite its source |
| Analyses | Add a new dated update block at the top |
| Superseded text | Strike it through and state what supersedes it |

Resolve relative dates to literal YYYY-MM-DD before recording them.

## Cross-Referencing

AI summaries provide structured action items. Cross-reference with the task system to identify:
- Action items already tracked as tasks
- New obligations not yet captured
- Commitments made by others that the user should follow up on

## Meeting Continuity

For recurring meetings, pull the recap of the previous occurrence to establish continuity — what was discussed last time, what action items were assigned, what's still open.

## See Also

- tasks(ea) — new obligations from a meeting become suggested tasks
- intelligence(ea) — meeting prep pulls the last occurrence's transcript
