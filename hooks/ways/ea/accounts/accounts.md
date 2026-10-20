---
description: which of the human's accounts to use for an email, calendar, or chat task, and checking that every account's login works before a multi-service briefing, with re-authentication when one account's token has expired or returns 401
vocabulary: account accounts identity which-account auth authentication preflight login re-auth reauth oauth token credentials expired 401 unauthorized consent status-table multi-account tenant workspace address
pattern: \bpreflight\b|\bre-?auth(enticate)?\b|no credentials provided|which (google |gmail |email |work )?account|(auth|login|token|credentials?) (is |are |looks |seems )?(stale|expired|broken|failing)
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Accounts and Auth

## Pick the account

- Use the account that owns the work. A work thread uses the work account. A personal matter uses the personal account. A client matter uses that client's tenant, if the user has one.
- Name the account on every call. Do not rely on a tool's default account, because the default is often the wrong one.
- A sweep, such as a briefing or a triage, covers every configured account unless the user names some.
- A single reply or lookup uses the account the thread lives on. Reply from the address that received the mail.
- When a contact has two addresses, write to the one that belongs to the business at hand.

## Preflight before multi-service work

A briefing that touches several accounts and services fails slowly when one login is stale. Check them all first, in parallel.

| Service | Cheap check |
|---|---|
| Each mail account | List one message |
| Each calendar | Read today's first event |
| Chat platform | The tool's auth or status call |
| Ticket tracker | A small search that the main work needs anyway, such as the user's tickets updated this week |

Pick checks that do real work where you can. Then the check doubles as data and costs nothing extra.

Show the results as a status table with one row per account and service. Start the main work only when the services it needs are green. If a service the task does not need is down, note it and go on.

## Fix an expired login yourself

When a mail or calendar tool's OAuth token has expired, run the re-auth flow yourself. Hand the user only the step that needs a human.

1. Start the tool's login command for that one account in the background. It often needs keychain access and a localhost callback listener, so run it outside any sandbox that blocks them.
2. Read its output for the consent URL.
3. Open the URL in the user's browser. Ask the user to finish consent on that account.
4. Wait for the login command to exit with success.
5. Verify with a real API call, such as listing one message. A tool's "auth status" output is not proof that calls work.

## Read the failure pattern

- One account returns 401 while the others work. The problem is that account's stored credentials, not the network or the tool. Confirm by running a cheap call on another account. Then re-auth the failing account.
- Clearing a token cache does not fix credentials that no longer decrypt. Re-auth is the fix.
- Every account fails at once. Suspect the tool, the network, or a shared config before you re-auth anything.
- A chat or ticket MCP server that stops answering may need a restart or reconnect rather than a new login.

## See Also

- ea(ea) — account iteration and safety rules
- briefing(ea) — the multi-service workflow that needs the preflight
- comms(ea) — browser sessions on chat platforms that expire
