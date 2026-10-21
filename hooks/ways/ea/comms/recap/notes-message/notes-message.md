---
description: pulling a full meeting transcript out of an auto-generated meeting-notes email, following the message's link to the notes document, fetching every tab of that document, and copying only the transcript tab verbatim into the repo instead of settling for the AI summary in the email
vocabulary: meeting-notes notes email auto-generated ai notes google meet google docs notes document transcript tab tabs includeTabsContent html body document id link paragraphs textRun verbatim transcripts folder ingest summary-only conference record
pattern: (notes|summary) email|meeting.?notes (email|message|doc)|transcript tab|google meet (notes|transcript)|\bmeet transcripts?\b
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Transcripts From a Meeting-Notes Message

Some meeting tools email an AI-written notes message after each call. The email holds only the summary. The full transcript often lives in a linked document, on a separate tab. Do not settle for the summary. Do not search file storage for a separate transcript file first, because one often does not exist.

Google Meet's note-taking feature works this way. It emails a link to a Google Doc, and the transcript sits in a tab titled `Transcript`. The steps below use that case. Map them to your own tool.

## Steps

1. Find the email on the account that attended the meeting. Search by the notes service's sender address plus the meeting name or an `after:` date.
2. Fetch the full message. Decode the HTML part (often base64url) and take the notes document link from it.
3. Fetch the document with every tab included. In the Google Docs API, that is `documents.get` with `includeTabsContent` set to true. Without it you get only the first tab.
4. Find the transcript tab. Walk its body content and join the text runs of each paragraph. Do this in a script that writes a file. Never retype the text.
5. Save it as `.md` in the repo's transcripts folder with the repo's naming, such as `YYYY-MM-DD_description.md`. Mark internal-only meetings if the repo does. Save the text as it came, with no added markdown.
6. Check the file. It should have speaker turns and run the length of the meeting. A few short paragraphs means you got the notes tab instead.

## When it fails

- No transcript tab means transcription was off for that meeting. Say so. Offer the platform's own recap transcript or a local transcription tool if one is set up.
- If the document fetch fails, check that you used the account the email went to. The document is often shared only with attendees.
- The meeting platform's API may expose transcripts directly. In Google Meet that is the conference records transcripts endpoint. Use it as a fallback when the document route fails.

After saving, follow comms/recap for verbatim rules, attribution, and mining.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "The summary covers what was decided" | The summary is interpretation. Decisions get cited by transcript line. |
| "I'll paste the text in by hand" | Retyping changes words. Copy through a script. |
| "The document came back, so it worked" | Check the tab. The default tab is the notes, not the transcript. |

## See Also

- comms/recap(ea) — verbatim rules, attribution, and mining after the transcript is saved
- email(ea) — searching the inbox on the right account
