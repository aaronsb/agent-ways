---
description: reading and drafting in Microsoft 365 web apps, such as Outlook mail, Teams chats and meeting recaps, and SharePoint, by driving the human's own signed-in browser when no API access exists for that tenant
vocabulary: outlook outlook-web teams microsoft m365 office365 sharepoint onedrive tenant sso federated browser-automation playwright attach extension signed-in chrome session eval snapshot selector draft paste html recap transcript detach
pattern: \boutlook (web|mail|inbox|draft)\b|\bteams (chat|meeting|recap|transcript|message)s?\b|\bsharepoint\b|\bonedrive\b|microsoft (365|mail|tenant)|playwright-cli (attach|eval|goto|detach)
commands: playwright-cli\s+(attach|goto|eval|drop|press|detach|close)\b
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Microsoft 365 Through the Browser

Some Microsoft 365 tenants give you no API access. A client's tenant with federated single sign-on is the common case. There, the working path is to drive the human's own signed-in browser.

## Connect

- Attach to the user's existing browser session, for example with playwright-cli and its browser extension. Do not launch a fresh browser and try to log in. A fresh browser cannot pass federated SSO or the tenant's device checks.
- After you navigate, check the page hostname and title. A Microsoft or SSO login host means the user is signed out in that browser. Ask them to sign in there, then retry.
- Check the attach prerequisites only when attach fails. These are a recent CLI version, the extension installed in the signed-in profile, and the extension token in the environment.

## Read cheaply

- Navigate straight to a folder URL, such as the inbox or a named folder, instead of clicking through the app.
- Read with a targeted script or selector that returns only the strings you need. A full page snapshot of Outlook or Teams runs to hundreds of lines.
- Wait a few seconds after navigation before you read. These apps render late.
- Find a message by matching the folder list with a regular expression. Outlook search covers the whole mailbox, and its first hit is often unrelated.

## Draft

- Build the body as HTML first. Insert it with a synthetic `text/html` paste event, so bold text and bullets survive. Typing the text loses the formatting.
- Add images by dropping a resized file onto the body. A file chooser is often refused over a browser bridge. Then check where the image landed by dumping the body structure.
- Save the draft and confirm the "Draft saved" notice. Reopen it from Drafts, check the text, and take one screenshot as proof.
- Never click Send. Only the user's explicit go word for that one message allows a send.

## Teams

- Reading chats, recaps, and transcripts is always safe. A chat message goes out only after the user approves its exact text and the target chat.
- A past meeting's recap often opens from its calendar popup, and the popup closes on its own. Click and read quickly.
- Take the Transcript tab, not the AI summary. Long transcripts load as you scroll, so scroll and read again until you reach the end.
- Before a meeting with people in that tenant, check the meeting chat and the last occurrence's recap.

## Clean up

- Detach from the session when you finish. Do not close the browser. Closing it ends the user's own signed-in session.
- Some tools leave a working folder, such as `.playwright-cli/`, in the current directory. Delete it so it does not land in a repo.

## See Also

- comms(ea) — chat platforms, read freely and post on approval
- comms/recap(ea) — what a recap holds and how to use the transcript
- email/drafting(ea) — writing the draft in the user's voice
