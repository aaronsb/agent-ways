---
description: a small command-line script that wraps an API for a skill to call, holding the auth, pagination, curl details and JSON payloads in one place, such as a read-only CLI over an internal or third-party API; call it by full path, keep it read-only in code, keep each business definition in one place, keep machine output clean, turn API errors into one line, never print secrets, and smoke-test every command
vocabulary: wrap the api in a command curl calls from scratch hand-rolled curl json payloads api wrapper cli run by agents rather than humans bots read only get only full path executable shebang json output one line errors smoke test every flag canonical filters
pattern: (helper|wrapper) (script|cli) (for|that) (the |an? |our )?(agents?|skills?|bots?)\b|read.?only (cli|wrapper|helper)\b|\bcurl (snippets|calls|commands)\b
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# API Wrapper CLIs

Raw request snippets in a skill make each run rediscover auth, pagination, quoting, and field names, and each rediscovery costs calls and errors. A wrapper command does that work once, in code that can be tested.

## Calling it

- Make the script executable with a shebang and document every call by its full path.
- Do not document `X="python3 script.py"` followed by `$X cmd`. Shell state does not persist between agent Bash calls, and zsh does not split an unquoted variable into words.
- `--help` lists every command and flag on one screen.

## Safety lives in code

- Route every data call through one read function. The auth token request is the only other call. A grep for the request method then audits the whole script.
- Load credentials inside the script from the project's env file. Never print a token or secret. On auth failure, print the status and the error type only.

## One home for each definition

- Put each business filter, such as what counts as open, closed, or a test record, in one named constant, and build every query from those constants.
- The default output matches the figure the source system reports. Print any cleaned view as a separate labeled line with what it removed.
- Print the caveats a number needs next to it.
- Encode the source query language's known traps in the helper once, instead of in every agent's prompt.

## Output

- Print human-readable tables by default. `--json` and `--csv` print only the data on stdout.
- Turn an API error into one line with its code and message. Validate dates and flag values before the request.

## Test every path

Keep a smoke test that runs every command with its defaults and with each flag value. It fails on any error string or traceback and checks that `--json` parses. Run it after every edit. The default path matters most, because most calls take it.

## See Also

- tooling(softwaredev) — when a repeated operation earns a tool
