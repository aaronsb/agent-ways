---
description: debugging, troubleshooting failures, investigating broken behavior
vocabulary: debug breakpoint stacktrace investigate troubleshoot regression bisect crash crashes crashing error fail bug log trace exception segfault hang timeout step broken
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Debugging Way

## Before Changing Any Code

1. Read the full error message and stack trace
2. Search the codebase for the error string
3. Check recent changes: `git log --oneline -10` and `git diff HEAD~3`
4. Reproduce the issue — if you can't trigger it, you can't verify a fix

## Do Not

- Change code based on guessing — verify the root cause first
- Fix multiple things at once — one change, one test
- Assume the bug is where the error appears — trace back to the source

## When Stuck

- `git bisect start` / `git bisect bad` / `git bisect good <ref>` to find the introducing commit
- Add targeted logging at function boundaries, not scattered everywhere
- Check the obvious: typos, wrong file, stale cache, wrong branch, missing env var
