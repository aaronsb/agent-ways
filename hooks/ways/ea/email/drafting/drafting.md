---
description: drafting email replies, writing style calibration, creating email drafts with proper threading
vocabulary: draft reply respond compose email write message tone voice style thread attachment
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Email Drafting

Compose draft replies in the user's voice. Never send directly — always create drafts for review.

## Workflow

1. **Read the original thread** to understand full context.
2. **Study the user's writing style.** Read 5-8 recent sent emails from the relevant account. Calibrate tone, length, greeting patterns, sign-off, and vocabulary.
3. **Ask clarifying questions before drafting:**
   - What is the intent? (accepting, declining, requesting, informing)
   - Any specific points to include?
   - Tone preference if it differs from their usual?
4. **Draft** following the user's observed patterns.
5. **Present for review** and iterate on feedback.
6. **Create as a platform draft** with proper threading (preserve message IDs, references, in-reply-to headers).

## Style Calibration

The user's sent mail is the authority. Study it before every draft. Look for:

- **Length pattern** — short/direct vs. detailed/thorough
- **Greeting/sign-off** — what they actually use, not what's conventional
- **Contractions** — do they use them naturally?
- **Tone** — warm, formal, casual, terse?
- **Structure** — prose paragraphs vs. bullet points?

## Anti-Patterns to Avoid

These are common AI-generated tells. Never use them in drafts:

- "I hope this email finds you well"
- "Just circling back" / "Per my last email"
- "I'd be happy to" / "Please don't hesitate to"
- "leverage", "synergy", "circle back"
- Excessive hedging or qualifiers
- Numbered lists for non-sequential content
- Restating what the other person said in a long paragraph

## Threading

When creating a draft reply, preserve the threading chain:
- Extract Message-ID, References, In-Reply-To from the original
- Set the draft as a reply to the correct thread
- For attachments, fetch from the relevant file storage and construct a proper multipart message

## Iteration Rule

When iterating on draft text with the user, do NOT re-create the platform draft on every revision. Only create the draft once the user approves the final version.
