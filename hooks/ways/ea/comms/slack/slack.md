---
description: Writing Slack messages and channel or thread replies for the user in native mrkdwn. Real user-ID mentions at the top so tagged people get pinged, bullets that render when sent through the API, hyperlinked phrases instead of bare URLs, short posts with no headers, greetings, or emojis, the right thread, and ghostwriting by default.
vocabulary: slack message post dm channel thread reply mention at-mention user-id bullets mrkdwn markdown ping teammates colleague hyperlink here-mention format render tag tagging pinged reviewers channel-thread thread-reply link-the-pr
pattern: \bslack (messages?|msgs?|dms?|posts?|reply|replies|thread|update)\b|\bslack (him|her|them)\b|\b(in|on|to|via|through) slack\b|\b(send|draft|write) (a|him a|her a|them a) slack\b|\bat[- ]?mention(ing)?\b|<@u[a-z0-9]+>|<!here>|post (it |this )?in #|\bmrkdwn\b
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Slack Messages

Apply this format to every Slack message without waiting to be told.

## Format

- Write Slack mrkdwn, not standard Markdown. Use `*bold*`, `_italic_`, `~strike~`, `<url|phrase>` for links, and `>` for quotes.
- Never use `##` headers, tables, nested lists, emojis, greetings, or sign-offs. A Slack message should never read like an email.
- Make bullets render. A leading `- ` sent through the API can show up as a literal hyphen. Use the `•` character or Slack's own list formatting. Get this right before sending.
- Keep bullets short and flat. Bold the words readers scan for.

## Mentions

- Put real `<@USERID>` mentions of the people addressed at the top. A typed name or a "Hey Name" opener notifies nobody.
- Use `<!here>` to address a group. For a meeting follow-up, mention every attendee up front.
- Look up user IDs with the Slack tool's user or conversation search. Store the IDs of regular contacts in the project instructions so later sessions skip the lookup.

## Links and content

- Hyperlink a phrase. Do not paste bare URLs or write "see attached".
- Link ADRs, PRs, commits, and existing review comments. Do not restate what they say.
- Open every link and confirm it lands on the exact page, not a redirect to somewhere else.
- Leave out stats and context sources that mean nothing to the reader.

## Voice

- Ghostwrite in the user's voice by default.
- Identify yourself at the top, next to the mentions, when the user asks for it or when the post is your own report or analysis. An example is "Posting as Sam's assistant". When you relay a report, link the hosted artifact.
- Write to peers as a collaborator working toward alignment, not as someone giving orders. Keep it matter-of-fact, not accusatory. Apologize lightly for a delay or a last-minute ask where that fits.
- For a sensitive workplace issue, start gently with the narrowest audience, such as the manager only. Still get the key facts in writing.
- Apply the user's tone edits literally, even one that breaks a rule above, such as a light emoji in a sensitive DM.

## Thread or channel

Follow the user's explicit cue. If the conversation is already a thread, reply in it. With no cue, ask. Read the latest replies before drafting so the message fits. After the user's reply goes out, mark that thread read if the tool supports it.

## Before showing the draft

Check the draft against the user's register, such as any banned punctuation. Post only on the user's go word.

## See Also

- comms(ea) — reading is safe, and posting needs approval
- trust/voice(meta) — ghostwriting versus attributed posts
- email/drafting(ea) — the email register that Slack must not imitate
