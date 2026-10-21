---
description: sending from the right one of the human's addresses, work gmail or personal, finding which inbox a message landed in, and checking that each mailbox and calendar login still works
vocabulary: address addresses gmail outlook inbox inboxes mailbox mailboxes work personal landed arrived received from send-as logins login expired reauth unauthorized 401 consulting which
pattern: which (of my )?(gmail|email|mail|outlook) (account|inbox|address)s?\b|which of my (inboxes|mailboxes|calendars|accounts)\b
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Accounts and Logins

The human often has several accounts for one service: work and personal mail, a calendar per role, more than one chat workspace. Every call goes to one of them.

## Choose the account

- Act from the account that owns the work. A work thread uses the work account, a personal matter the personal one, and a client matter that client's tenant when the human has one.
- Reply from the address that received the message. Send an invite from the calendar the meeting belongs to.
- Name the account on every call. A tool's default account is often the wrong one.
- When the request leaves the account open and the choice changes who sees the result, ask once before the first write. A read can search each candidate account.
- When a contact has two addresses, write to the one that belongs to the matter at hand.

## Check logins first

Before work that spans several accounts or services, check each login it needs, in parallel, with a cheap call that returns real data: one message from each mailbox, today's first event from each calendar. A tool's auth-status output does not prove that calls work. Report which accounts answered. Start once the accounts the work needs are working, and note any other account that is down.

## When a login fails

- One account fails while the others work. The stored credentials for that account are the cause. Confirm with a cheap call on another account.
- Every account fails at once. Suspect the tool, the network or shared configuration before any login.
- An expired token is fixed by signing in again. Clearing a cache does not restore credentials that no longer decrypt. Start the tool's own login flow for that account, hand the human the consent step, and verify afterwards with a real call.
- When a sandbox, tenant policy or missing admin consent stops the login flow, give the human the error and stop there.
- A chat server that stops answering may need a reconnect rather than a new login.

## See Also

- ea(ea) — parent: read-only by default, every write approved
- comms(ea) — chat platforms whose browser session expires, a different fix from an expired token
- briefing(ea) — the multi-account work that needs the login check
