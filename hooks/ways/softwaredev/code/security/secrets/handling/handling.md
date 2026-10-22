---
description: using a credential in a session without exposing its value; feed a pasted API key to the command on stdin without echoing it, confirm with a names-only listing, keep tokens out of URLs, query strings and browser code, debug auth with status codes only, keep keys in gitignored local stores, and scan the diff for leaked tokens before publishing
vocabulary: pasted api key echo stdin names-only gh secret list actions secrets keychain envrc dev.vars check-ignore query string referrer browser bundle client-side 401 403 leaked token planted fake
scope: agent, subagent
refire: 0.2
---
<!-- epistemic: constraint -->
# Handling a Live Credential

## Where Secrets Live

- Keep credentials in gitignored local stores only: a per-project `.env` rather than a global environment variable, `.dev.vars`, `.envrc`, the platform's secret store, the CI repository secrets, or the OS keychain.
- Run `git check-ignore <file>` on each one before any commit.

## Use a Value Without Showing It

- Never cat, echo, log, or quote a value. Write "set in .env" instead.
- Pipe a value into the command that needs it without echoing it. Verify with a names-only listing such as `gh secret list`, or a `--check` mode.
- Load credentials inside a helper script rather than inlining them in a `curl` line.
- When debugging auth, show status codes and error types only. Leave out client ids, client secrets, tokens, and request bodies.
- When the user pastes a key into the chat, check that it belongs to the right account and project, set it yourself piped without echo, and confirm it saved.

## Keep Keys Off the Client

- A key never goes in a URL path or query string. URLs land in logs, history, and referrer headers.
- A key never goes into code the browser loads. Let the backend hold the keys.
- End users never paste a service credential into the UI.

## Before Commit or Publish

Run the project's secret scanner over the diff, once you have seen it flag a planted fake token. Check the diff as well for real internal URLs, account-specific ids, and machine or account paths. Use placeholders such as `PROJ-123` and fictional names.

## See Also

- code/security/secrets(softwaredev) — parent: what never gets committed, recording a credential by name, rotation order
- code/testing/gates(softwaredev) — a scanner earns trust once a planted violation has turned it red
