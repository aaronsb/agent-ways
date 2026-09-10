---
description: secrets management, credential hygiene, .env files, API keys, password storage, logging or filing a credential by name without its value, and rotation order after a secret is committed
vocabulary: secret credential password token api key env .env dotenv rotate rotation expose exposed leaked committed hardcoded log mask redact scan gitignore bcrypt argon2 hash encrypt vault
scope: agent, subagent
refire: 0.2
---
<!-- epistemic: constraint -->
# Secrets Way

## Never Commit

- `.env` files with real secrets
- API keys, tokens, passwords
- Private keys, certificates

When creating `.env`, also create `.env.example` with placeholder values. Verify `.env` is in `.gitignore`.

## Hardcoded Secrets

If you see a hardcoded secret in source:
1. Extract to environment variable
2. Flag it in your response
3. Check if the secret has been committed to git history (if so, it is already leaked and rotation is needed)

## Recording a Credential

Any artifact (a log, a diagnostic, a ticket, a chat reply) records a credential by key name and location only. A partially masked value still leaks shape, length, and prefix into a permanent searchable record. Establish that a secret exists by metadata (a filename, a scanner hit) without opening it; reading it puts live bytes into a transcript.

## Committed Means Compromised

A secret that reached version control is compromised the moment it lands. Removing it from the file or the history is hygiene, and a history rewrite is an owner decision. Rotation is the remedy, in descending blast-radius order:

1. Signing keys
2. Datastore credentials
3. Third-party tokens

Rotate under explicit confirmation, and re-run the secret scan as each rotation's exit gate.

## Password Storage

- Hash with bcrypt or argon2; never store plain text
- Use a work factor appropriate for the platform (bcrypt cost 12+ for servers)
- Never roll your own hashing; use the library
