---
description: team chat and messaging platforms, reading chat messages, sending messages with approval, communication channels
vocabulary: teams chat message slack channel unread conversation direct message group chat send reply mention notification
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Communications — Chat & Messaging

Access team chat and messaging platforms (Teams, Slack, etc.) for reading conversations, checking unread messages, and composing replies.

## Reading is Safe, Sending Requires Approval

- **Always safe:** Reading chats, browsing channels, checking unread indicators, viewing message history.
- **Requires explicit approval:** Sending or posting any message. Draft it, present it with the target chat context, and wait for the user to approve.

## Chat Triage

When scanning chats:
1. Check for unread indicators across all accounts/platforms.
2. Prioritize 1:1 messages from real people over group chats and channels.
3. Note meeting-related chats — they often contain pre-meeting context or post-meeting follow-ups.
4. Cross-reference chat participants with email threads and calendar events.

## As a Context Layer

Chat platforms are not just another inbox. Use them proactively to enrich other workflows:

- **Before a meeting:** Check the meeting's group chat for pre-meeting discussion and shared links.
- **During email triage:** If someone emailed AND messaged, note the parallel conversation.
- **After a meeting:** Check the chat for follow-up items, shared files, and action items that didn't make it to email.

## Platform Authentication

Some platforms require browser-based session authentication that expires periodically. If a session has expired (redirected to login), inform the user and guide them through re-authentication.
