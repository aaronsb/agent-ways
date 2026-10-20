---
description: secrets management, credential hygiene, .env files, API keys, password storage, where credentials live, using a key the user pasted without echoing it, keeping keys out of URLs and browser code, logging or filing a credential by name without its value, and rotation order after a secret is committed
vocabulary: secret credential password token api key env .env dotenv rotate rotation expose exposed leaked committed hardcoded log mask redact scan gitignore bcrypt argon2 hash encrypt vault env.example placeholder blast-radius roll metadata pasted keychain envrc dev.vars check-ignore url query string browser client-side frontend secret list names-only registry expiry fallback
scope: agent, subagent
refire: 0.2
---
<!-- epistemic: constraint -->
# Secrets Way

Users often hand over keys freely, so you are the guardrail.

## Never Commit

- `.env` files with real secrets
- API keys, tokens, passwords
- Private keys, certificates

When creating `.env`, also create `.env.example` with placeholder values and the same structure as the real file. Verify `.env` is in `.gitignore`.

## Where Secrets Live

- Keep credentials in gitignored local stores only: a per-project `.env` rather than a global environment variable, `.dev.vars`, `.envrc`, the platform's secret store, the CI repository secrets, or the OS keychain.
- Run `git check-ignore <file>` on each one before any commit.
- Gitignore directories of personal data too, such as attendee lists or travel details.

## Hardcoded Secrets

If you see a hardcoded secret in source:
1. Extract to environment variable
2. Flag it in your response
3. Check if the secret has been committed to git history (if so, it is already leaked and rotation is needed)

## Recording a Credential

Any artifact (a log, a diagnostic, a ticket, a chat reply) records a credential by key name and location only. A partially masked value still leaks shape, length, and prefix into a permanent searchable record. Establish that a secret exists by metadata (a filename, a scanner hit) without opening it; reading it puts live bytes into a transcript.

- Never cat, echo, log, or quote a value. Write "set in .env" instead.
- Pipe a value into the command that needs it without echoing it. Verify with a names-only listing such as `gh secret list`, or a `--check` mode.
- Load credentials inside a helper script rather than inlining them in a `curl` line.
- When debugging auth, show status codes and error types only. Leave out client ids, client secrets, tokens, and request bodies.

## Keep Keys Off the Client

- A key never goes in a URL path or query string. URLs land in logs, history, and referrer headers.
- A key never goes into code the browser loads. Put the deployed app behind an access gate or a simple password, and let the backend hold the keys.
- End users never type a secret into a UI.

## A Pasted Secret Is Intentional

When the user pastes a key into the chat, use it as given and add at most one line of note. First check that it belongs to the right account and project. Set it yourself, piped without echo, then confirm it saved. Run auth flows yourself so the user only has to click through.

| Rationalization | Counter |
|---|---|
| "The key is in the transcript now, ask them to recreate it" | Use it as given. Rotation is the user's call. |
| "They should run the secret command themselves" | Run it without echoing the value, then confirm it saved. |

## Before Commit or Publish

Grep the diff for the token prefixes of every provider the project uses, real internal URLs, account-specific ids, confirmation codes, and machine or account paths. Use placeholders such as `PROJ-123` and fictional names. Hold back files with personal data and say so. Create new repositories as private unless told otherwise.

## Committed Means Compromised

A secret that reached version control is compromised the moment it lands. Removing it from the file or the history is hygiene, and a history rewrite is an owner decision. Rotation is the remedy, in descending blast-radius order:

1. Signing keys
2. Datastore credentials
3. Third-party tokens

Rotate under explicit confirmation, and re-run the secret scan as each rotation's exit gate.

## Bookkeeping

Record each key created in a session, with its expiry, in the project's key registry. Store a fallback credential without overwriting the primary, and tell the user the existing setup is unchanged.

## Password Storage

- Hash with bcrypt or argon2; never store plain text
- Use a work factor appropriate for the platform (bcrypt cost 12+ for servers)
- Never roll your own hashing; use the library

## See Also

- environment/config(softwaredev) — env files and the configuration hierarchy
- code/supplychain/historysever(softwaredev) — removing a secret from git history
- code/supplychain/repoaudit(softwaredev) — scanning history for leaked secrets
