---
description: checking what a delegated run actually did; read the diff of a subagent's work and fail it when it touched files outside its scope, believe the harness numbers over its summary, and pilot a bulk change on a few rows in a scratch copy before running all of them
vocabulary: diff files touched outside scope fluent summary says finished claims done harness numbers helper agent pilot few rows scratch copy standalone script every record in one go thousands bulk change
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Delegated and Bulk Runs

- A fluent report is not a finished task. Trust the harness numbers and the diff over a model's summary of them.
- After a delegated run, read the diff and check which files it changed. A run that edited outside its scope fails, whatever its summary says.
- Before a bulk run, run the first few rows in a scratch location or a standalone script and check them. Scale up once those come out right.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "The agent said all tests pass" | Read the harness output and the diff. |
| "It worked on the sample, run all of it" | Pilot a few rows first, then scale. |

## See Also

- code/testing/gates(softwaredev) — parent: gate states, positive controls, and the report format
- meta/subagents(meta) — the return shape a delegated run hands back
