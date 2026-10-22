---
description: drafting email replies, writing style calibration, creating email drafts with proper threading
vocabulary: draft reply respond compose email write message tone voice style thread attachment multipart message-id in-reply-to ai-generated
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Email Drafting

Compose draft replies in the user's voice, show the draft, and ask whether to send it. Send it on a yes. Without one, leave it as a platform draft (trust/delegation, what counts as a request to send).

## Workflow

1. Read the original thread: to understand full context.
2. Study the user's writing style. Read 5-8 recent sent emails from the relevant account. Calibrate tone, length, greeting patterns, sign-off, and vocabulary.
3. Ask only when the thread leaves the intent open (accepting, declining, requesting, informing), under the asking bar in core.md.
4. Draft: following the user's observed patterns.
5. Present for review: iterate on feedback, then ask whether to send.
6. Send it on a yes, or create it as a platform draft. Either way, keep the threading (message IDs, references, in-reply-to headers).

## Style Calibration

The user's sent mail is the authority. Study it before every draft. Look for:

- Length pattern: short/direct vs. detailed/thorough
- Greeting/sign-off: the ones they actually use
- Contractions: do they use them naturally?
- Tone: warm, formal, casual, terse?
- Structure: prose paragraphs vs. bullet points?

## Generated-Writing Tells

The draft goes out under the user's name. Before presenting it, check it against writing/ai-signs: stock words, chatbot framing, clerk phrasing and the shapes that mark generated text. Three email habits read as generated too: the opener "I hope this email finds you well", the follow-up "just circling back", and filler words such as "leverage" and "synergy". Where the user's sent mail uses a phrase that list flags, the sent mail wins.

## Threading

When creating a draft reply, preserve the threading chain:
- Extract Message-ID, References, In-Reply-To from the original
- Set the draft as a reply to the correct thread
- For attachments, fetch from the relevant file storage and construct a proper multipart message

## Iteration Rule

When iterating on draft text with the user, do NOT re-create the platform draft on every revision. Only create the draft once the user approves the final version.

## See Also

- email/drafting/composition(ea) — the shape, tone and handover of a routine email
- writing/ai-signs(writing) — the phrases and shapes that make a draft read as generated
