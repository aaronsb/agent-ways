---
description: checking what a delegated run actually did; check the list of files a subagent's run changed against its scope and fail it when it touched files outside that scope, believe the harness numbers over its summary, and pilot a bulk change on a few rows in a scratch copy before running all of them
vocabulary: diff stat changed files list files touched outside scope fluent summary says finished claims done harness numbers helper agent pilot few rows scratch copy standalone script every record in one go thousands bulk change
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Delegated and Bulk Runs

- A fluent report is not a finished task. Trust the harness numbers and the list of changed files over a model's summary of them.
- After a delegated run, check the files it changed against its scope, for example with `git diff --stat` against the base it started from. This is a quick ambient check of the file list, not a full read of the diff (code/testing/gates/attention). A run that edited outside its scope fails, whatever its summary says.
- Before a bulk run, run the first few rows in a scratch location or a standalone script and check them. Scale up once those come out right.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "The agent said all tests pass" | Read the harness output and the list of changed files. |
| "It worked on the sample, run all of it" | Pilot a few rows first, then scale. |

## See Also

- code/testing/gates(softwaredev) — parent: gate states, positive controls, and the report format
- meta/subagents(meta) — the return shape a delegated run hands back
- subagents/lead(meta) — the lead's ambient attention, which does not read diffs
