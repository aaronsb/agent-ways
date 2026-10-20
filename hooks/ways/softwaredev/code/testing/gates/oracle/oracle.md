---
description: ending a plan with a runnable verification sequence of exact commands and expected results, trusting harness numbers and the diff over a model's summary, never loosening a pass criterion to reach green, rerunning earlier passing cases on the merged tree, piloting a bulk run on a few rows, and preferring guardrails in code over prompt instructions
vocabulary: verification sequence cheapest first expected result exit code command observation plan oracle harness diff summary fluent delegated run loosen criterion threshold green regression rerun merged tree baseline hash refactor pilot first rows bulk run scale up guardrail lint drift check allowlist confinement harden unverified cannot run remote
pattern: verification (sequence|steps|plan)|cheapest first|(loosen|relax|lower) (the )?(pass )?(criteria|criterion|threshold)
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# The Harness Is the Oracle

The gates way says how to report a check. This way covers how to plan the checks and what to trust when they disagree with a summary.

## End every plan with a verification sequence

Close each plan with a numbered "Verification sequence (cheapest first)". Each item is an exact command or observation plus its expected result:

```
1. `npm test` exits 0
2. `grep -c '<aside' decks/*.html` returns 0
3. `curl -sI <url>` returns a 302 to the login page
```

For a data edit, read the changed range back, confirm nothing else changed, and check the content rules the data must follow. "Confirm it works" is not a check.

## Trust the harness over the summary

- A fluent report is not a finished task. Trust the harness numbers and the diff over a model's summary of them.
- Review the diff after every delegated run.
- Never loosen a pass criterion to reach green. Report the number you got and stop.
- Assert which files changed, so a run that edits outside its scope fails.

## Rerun what passed before

- After every change, rerun the cases that passed before it. Run the full suite on the merged tree, not only on the branch.
- Before a refactor, capture a baseline, such as pixel or byte hashes of rendered output or file hashes for a read-only audit. Diff against it afterward.
- Keep offline guard tests green before any live run.
- Before a bulk run, try the first few rows in a scratch directory or a standalone script. Scale up only once those come out right.

## Guardrails belong in code

Prefer lint gates, drift checks, parse checks, path confinement, and empty allowlists over instructions in a prompt. A prompt asks. A check enforces. When a check fails, fix the cause. Do not add the failing case to the allowlist.

## Hardening on request

When asked to harden a project, audit it, add the missing tests, then wire the checks into CI. Fix what needs no design decision now. Raise the rest as decisions for the user.

## Code you cannot run

Code that runs where you cannot reach it, such as a hosted automation platform or a customer's instance, stays unverified until the user tests it there. Hand over the exact steps and the expected result.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "The agent said all tests pass" | Read the harness output and the diff. |
| "The threshold was too strict anyway" | Report the miss. Changing the bar is a separate, reviewed decision. |
| "The branch is green" | Run the suite on the merged tree. |
| "It worked on the sample, run all of it" | Pilot a few rows first, then scale. |

## See Also

- code/testing/gates(softwaredev) — parent: gate states, positive controls, and the report format
- code/testing/gates/assertions(softwaredev) — assertions that are hard to game
- code/security/guards(softwaredev) — never widen a control to clear a symptom
- freshness/groundtruth(softwaredev) — the golden-master baseline before a refactor
- environment/hostparity(softwaredev) — code that runs on a host you cannot reach
