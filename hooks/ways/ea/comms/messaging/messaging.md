---
description: a slack or teams post, thread reply or direct message to coworkers, with at-mentions and links instead of pasted urls
vocabulary: dm post channel thread reply tell ping mention tag notify coworker coworkers late standup heads up let them know
pattern: \bdm [a-z]+ (that|to|about)\b|(^|\s)(tell|ping|message|post in|post to|post it in|send to|send it to|let) #[a-z][a-z0-9_-]*
pattern_strict: true
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Messaging

When composing a chat message or post for the human through a platform's tool.

## Compose for the platform

- Write in the platform's own markup, as its tool describes it. Standard Markdown often arrives as literal characters.
- Address people with the platform's mention syntax, looked up through the tool. A typed name or a greeting notifies nobody.
- Keep it short and flat. A chat message carries no headers, greeting or sign-off.
- Link a phrase rather than pasting a bare URL. Link the PR, record or document instead of restating it, and open each link to confirm where it lands.

## Place it

- Follow the human's cue for thread or channel. When the conversation is already a thread, reply in it. When the destination is unclear, ask.
- Read the latest replies before drafting, so the message fits where the conversation stands.
- For a sensitive workplace matter, start with the narrowest audience that can act on it.

## Voice

- In a conversation the human is part of, write as the assistant, in your own voice. Write as the human only when they direct it.
- When the human directs you to write as them, match their voice and apply their tone edits as given.

## Send

- Draft by default. Show the draft with its workspace and destination, and send on approval.
- When the human asks for autonomous replies, send without per-message approval, within the conversations and the period they named.
- A send that the tool blocks, holds for approval or refuses has not gone out. Tell the human. Do not rework the content or route it another way to get it through.

## See Also

- comms(ea) — parent: reading is safe, posting needs approval
- trust/voice(meta) — ghostwriting versus attributed writing
- email/drafting(ea) — the email register
