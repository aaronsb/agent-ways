---
description: Covers building a helper CLI that agents call from a skill, such as a read-only wrapper over a CRM, finance, or ticketing API. Make it an executable called by full path, enforce read-only in the request layer, keep business definitions in one place, return the standard figure before cleaned views, keep machine output clean, turn API errors into one line, never print secrets, and smoke-test every command and flag.
vocabulary: helper-script skill-helper agent-helper api-wrapper read-only-wrapper cli-for-agents replace-curl executable shebang full-path smoke-test json-output canonical-filters get-only oauth-token one-line-errors
pattern: (helper|wrapper) (script|cli) (for|that) (the |an? |our )?(agents?|skills?|api)\b|read.?only (cli|wrapper|helper)\b|\bcli (that )?(the |an? )?(agents?|skills?) (can )?(calls?|uses?)\b|replace (the )?curl (snippets|calls) (with|in)|\bcurl (snippets|calls|commands)\b
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Helper CLIs for Agents

## Why a helper

Curl snippets in a skill make every agent rediscover auth, pagination, quoting, and field names. One helper command does that work in one call. In one skill rebuild, a helper cut tool calls per question by about a third and nearly removed failed calls.

## Calling it

- Make the script executable with a shebang, such as `#!/usr/bin/env -S python3 -I`. Document every call by full path.
- Never document `X="python3 script.py"` followed by `$X cmd`. Shell state does not persist between agent Bash calls, and zsh does not split an unquoted variable into words. Agents in one eval lost about two calls each to that pattern.
- `--help` lists every command and flag on one screen.

## Safety lives in code

- Route every data call through one GET function. The auth token request is the only other call. Audit it with `grep -n 'urlopen\|method=' script.py`.
- Load credentials from the project's env file inside the script. Never print tokens or secrets. On auth failure, print only the status and the error type.

## One home for each definition

- Put each business filter in one named constant, such as what counts as open, won, lost, test, or a vendor program record. Build every query from those constants.
- Defaults return the standard figure, so the helper matches a report in the source system. Cleaned views print as labeled second lines with the count and amount removed.
- Print the caveats a number needs next to it, such as excluded rows or placeholder costs.

## Output

- Print human tables by default. `--json` and `--csv` print clean output with nothing else on stdout.
- Turn API errors into one line with the error code and message. Validate dates and flag values up front.
- Label blank group keys "(blank)". Do not put thousands separators on years.
- Build known query-language traps into the helper. For SOQL these are no aliases in non-aggregate queries, a duplicate `Name` alias when grouping two `.Name` fields, `AND (NOT ...)` instead of `AND NOT (...)`, and datetime literals on datetime fields.

## Test every path

- Keep a smoke test that runs every command with its default and with each flag value, against live data. It fails on any error string or traceback, and it checks that `--json` parses.
- Run it after every edit, before any eval round. The default path matters most, because most calls take it.

## See Also

- tooling(softwaredev) — when a repeated operation earns a tool
