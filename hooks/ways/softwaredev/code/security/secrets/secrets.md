---
description: secrets management, credential hygiene, .env files, API keys, password storage
vocabulary: secret credential password token key env api-key rotate expose exposed .env gitignore bcrypt hash encrypt
scope: agent, subagent
curve:
  type: Exponential
  half_life: 200000
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
3. Check if the secret has been committed to git history (if so, it's already leaked — rotation needed)

## Password Storage

- Hash with bcrypt or argon2 — never store plain text
- Use a work factor appropriate for the platform (bcrypt cost 12+ for servers)
- Never roll your own hashing — use the library
