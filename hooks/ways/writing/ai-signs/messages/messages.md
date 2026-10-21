---
description: Generic and chatbot-shaped lines in messages to people, in email, Slack, Teams, DMs, Jira and PR comments, and chat replies. Cut chatbot framing and offers, clerk-style openers and assurances, vague apologies, open requests for feedback, and canned change summaries. Leave headings out of short messages, never paste a subject line into the body, use markup the channel renders, and skip the recap closer.
vocabulary: message reply respond email slack teams dm comment recipient inbox certainly hope-this-helps apologize inconvenience assure canned boilerplate opener closer offer sycophancy subject-line followup chatbot recap
pattern: \b(certainly!|i hope this helps|i apologi[sz]e for any inconvenience|i am writing to|i understand your concern|please don.?t hesitate)\b|\b(slack|teams|pr|jira|ticket) (reply|comment|message|dm)s? (sounds?|reads?) (like|too)\b
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Generic phrasing in messages

The reader learns nothing from these lines. Keep any phrase from the user's own sent mail, because their sent mail outranks this list.

## Cut these lines from the message text itself

| Kind | Examples |
|---|---|
| Chatbot framing | "Certainly!", "Here is a draft", "I hope this helps" |
| Clerk talk | "I am writing to", "I understand your concern", "I assure you", "I have carefully reviewed", "to ensure alignment with your guidelines" |
| Vague apology | "I apologize for any inconvenience" |
| Open asks | "If there are specific areas", "I welcome any feedback" |
| Change summaries | "refined for clarity and flow", "while preserving the original meaning" |

One polite closing is fine. Do not stack several.

## State the fact instead

- Replace an assurance with the action and date the user gave. If the user gave none, cut it.
- A light "Sorry for the delay" is fine. Name a real mistake and what changed, using only facts the user gave.
- Ask one specific question. If someone in the thread already gave the reason, quote it.
- Answer pushback with facts. Do not call it speculation or unsubstantiated.
- In a PR or Jira comment, say what changed and why, such as "Renamed the field to match the API."

## Format for the channel

- Leave headings out of short messages. Keep them only in a recap the user files.
- Never paste a "Subject:" line into a body.
- Use markup the channel renders. The reader sees raw `##` and `**` as literal text.
- Stop once you have asked. Skip the closing recap.

## See Also

- writing(writing) — audience and revision passes
- email/drafting(ea) — style calibration from sent mail
- comms(ea) — chat platforms and posting approval
- trust/voice(meta) — whose voice the message speaks in
