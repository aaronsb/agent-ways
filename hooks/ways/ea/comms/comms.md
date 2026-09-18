---
description: workplace chat platforms reached through their own tools — Slack and Microsoft Teams workspaces, channels and threads, unread direct messages on those platforms, posting there on the human's behalf
vocabulary: slack teams microsoft-teams workspace platform channel thread unread dm huddle post workspace-chat platform-login
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Communications — Chat Platforms

**Scope.** This way covers human chat platforms reached through their own tools: Slack, Microsoft Teams, and the like. Peer messaging between Claude sessions over `attend` is a different surface with its own way, environment/attend(softwaredev). Replies there need no approval, and nothing below applies to them.

Access those platforms for reading conversations, checking unread threads, and composing posts.

## Reading is Safe, Posting Requires Approval

- **Always safe:** reading chats, browsing channels, checking unread indicators, viewing history.
- **Requires explicit approval:** posting anything to the platform. Draft it, present it with the target workspace and channel, and wait for the human to approve.

## Chat Triage

When scanning a platform:
1. Check unread indicators across every account and workspace.
2. Prioritize 1:1 threads from real people over group chats and channels.
3. Note meeting-related threads. They often carry pre-meeting context or post-meeting follow-ups.
4. Cross-reference participants with email threads and calendar events.

## As a Context Layer

A chat platform is not just another inbox. Use it to enrich other workflows:

- **Before a meeting:** check the meeting's group thread for discussion and shared links.
- **During email triage:** if someone emailed and also posted, note the parallel conversation.
- **After a meeting:** check the thread for follow-up items, shared files, and action items that never reached email.

## Platform Authentication

Some platforms use browser-based sessions that expire. If a session has expired (redirected to login), tell the human and guide them through re-authentication.

## See Also

- environment/attend(softwaredev) — peer messaging between Claude sessions; autonomous replies live there
- email(ea) — the inbox side of the same triage
