---
description: saving the full text of a call or meeting into a project exactly as it was said, following an AI notes message to the full transcript, catching speaker labels that got mixed up, and citing transcript lines for what was decided and who owns each follow-up
vocabulary: verbatim word-for-word exactly full text call kickoff save folder ingest script retype cleanup caption vtt diarization labels mixed misattributed swapped talking attribution link tab line cite citation decided owns
pattern: \bdiariz|misattribut|speaker labels?\b|\.vtt\b|transcript tab
files: \.vtt$|(^|/)meetings?[-_]?transcripts?/
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Saving and Attributing Transcripts

On a project that keeps records, the transcript is the primary record of what was decided. Other documents cite it by path and line.

## Getting the full transcript

- An AI notes message sent after a meeting often holds only the summary, which is interpretation. Follow its link to the full transcript. The linked document may keep the transcript on its own tab or section, so fetch every part of it, with the account that attended.
- If no transcript exists, transcription was off for that meeting. Say so, and offer a local transcription tool if one is set up.

## Saving it

- Take only the transcript. Leave out the AI summary and notes, because they are interpretation.
- Copy the text through a script that writes a file. Never retype it.
- Save it where the project keeps transcripts, with the project's naming, such as date and meeting type.
- Convert caption files such as `.vtt` to `.md`. Collapse them so only speaker changes are marked.
- Check the saved file. It should have speaker turns and run the length of the meeting. A few short paragraphs means the notes came back instead.
- The raw copy stays verbatim. Do not rewrite, summarize, re-punctuate, or fix errors in it. Put corrections in a separate, labeled copy.

## Attribution

Speaker labels are often wrong, even for the person who presented, and speakers change mid-topic. Before attributing a statement, check the label at that line against the flow of the talk, the attendee list, and related chat and email. Ask targeted questions where it stays unclear. Do not create items from remarks about a different client or project. Flag internal or sensitive remarks before anything reaches an outside reader.

## Mining

When asked, extract decisions, agreed and not agreed, and per-person action items, each citing transcript lines. After a new transcript is saved, say what it adds, what it changes in the existing record, and which documents it refers to that are missing.

## See Also

- comms/recap(ea) — parent: what meeting platforms provide after a call
- lookup(ea) — dates written as calendar dates
- tasks(ea) — new obligations from a meeting become suggested tasks
