---
description: drafting email replies, writing style calibration, the shape and tone of a routine email, creating email drafts with proper threading, and what to attach or report
vocabulary: draft reply respond compose email write message tone voice style thread attachment multipart message-id in-reply-to ai-generated sign-off greeting peer recap recurring invoice cc mailbox length
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

Match how the user writes to this recipient in this context. Real sent mail outranks any style guide. AI tics found in AI-assisted documents are not the user's style. Keep the email register separate from chat.

## Shape

- Routine mail runs 2 to 5 sentences with one ask and the next step. Then it stops.
- Write for an ordinary mail client. Use simple bullets at most. Do not use tables, ASCII art, or heavy markdown.
- Use the recipient's own wording and questions, and include any action the recipient must take. Do not repeat back data they already stated.
- When there is a lot of detail, send a short agenda email and put the detail in an attached document.
- To explain a fix, say what was wrong and what changed, one sentence each.
- A recap email that serves as a record may use numbered bold headings.

## Tone

- Keep it short, direct, and warm without gushing.
- Write as a peer focused on alignment, not as a boss. Apologize lightly for a delay where it fits.
- Stay matter-of-fact, never accusatory. Raise a sensitive issue with the narrowest audience first, such as the manager only, and still put the key facts in writing.
- Add no commitments or polish the user did not state. Keep every thread participant on the reply.

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

## Delivery

- Create the draft in the correct account's mailbox. Tell the user where it lives.
- Attach nothing unless asked. When files need to go out, put them in one folder the user can attach by hand.
- For recurring emails such as invoices or weekly updates, mirror the last instance's wording, addressees, and CCs. When the user changes the standard, update the template or skill that produces it.
- Do not resend a draft that the user's sent mail has already superseded.
- If the user sends an email themselves and a chat follow-up depends on it, wait for their confirmation before posting the follow-up.
- Confirm the exact time and timezone before scheduling a send.
- Before presenting a draft, check it against the user's register, such as banned punctuation and phrases.

## Iteration Rule

When iterating on draft text with the user, do NOT re-create the platform draft on every revision. Only create the draft once the user approves the final version.
