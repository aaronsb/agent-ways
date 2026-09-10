---
description: verification gates before merge, whether the tests and checks actually ran, reporting each gate as executed, discovered, or absent, positive controls for empty results, gate depth by change class
vocabulary: gate gates ci check checks verification verify verified tests ran coverage scanner scan lint pass green ready merge executed discovered absent positive control null result zero findings clean instrument report
commands: (npm|pnpm|yarn)\ test|pytest|cargo\ test|go\ test|make\ test|make\ check
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Verification Gates

A change gets called verified when the checks that cover it ran and reported. The failure this way prevents is quieter than a red build: a suite that was never invoked, a step that skipped itself and exited zero, a scanner aimed at the wrong path, and a report that lists none of it. The reader takes the silence for a pass.

## Every gate reports one of three states

A gate is any check that stands between a change and merge: formatter, linter, type check, unit or integration suite, contract test, build, security scan, smoke test, manual review. Each gate you considered lands in exactly one state, and the report says which.

| State | Meaning | What the line carries |
|---|---|---|
| **executed** | Run on this change, prerequisites held, assertions ran | The command and the result |
| **discovered** | Known to exist, deliberately left unrun | Why it was skipped |
| **absent** | No such gate exists | The date and the reason |

An exit code of zero counts as executed only when the check did its work. A step that skipped itself, a suite that collected zero tests, a tool configured but inert, a run whose test count came back short: record these as discovered, whatever the runner printed. A failing gate is fixed or handed back. Never record a pass that did not happen. A new gate earns trust once a planted violation has turned it red and the plant's removal has turned it green again.

An inherited codebase with no test infrastructure still gets a full report: each standard gate on its own line as absent with a date. A blank report reads the same as one nobody wrote.

## A zero needs a positive control

A scan with no findings, a grep with no hits, a discovery run that collected nothing, and a port check that saw nothing open all return exactly what a broken probe returns. The result alone cannot tell the two apart.

Report a zero only alongside a positive control the same run detected. Point the scanner at a fixture known to trip it, grep for a string you know is present, assert the suite collected the tests you know exist. Then state the coverage the control establishes. "No secrets found by <tool> across <paths>, control fixture detected" is a finding. "No secrets" is a hope with a command history.

A check you author counts what it examined and fails when that count is zero inside its own perimeter. A glob that matched nothing is an environment fault. Reserve the empty result for a genuine empty success.

## Gate depth follows the change class

| Change class | Minimum gate depth |
|---|---|
| Trivial edit, no behavior or contract surface | The one focused check that covers it (formatter, linter, build) |
| Leaf logic, contracts unchanged, affected path known | Cheap static gates plus the focused unit or integration tests on that path |
| Shared module | Static gates, the module's own suite, and the suites of its callers |
| Public interface or persisted format | The full battery on the boundary: contract tests, integration, neighboring regression suites |
| Auth, security, data migration, concurrency, central abstraction | Broad system gates: the full suite, security scan, end-to-end on critical flows, manual review |
| Affected scope uncertain | The row above your best guess. Uncertainty buys breadth. |

Escalate one row the moment a local change turns out to touch a shared surface. Run the cheap syntactic gates first and proceed to slower ones only when they pass. A broad battery on a provably local change spends attention and wall clock for nothing.

## The instrument is the first suspect

When a check and its subject disagree, read the raw source at the cited location before recording the finding. Two contradictory measurements of your own indict the method. A line-oriented scan over a multi-line construct produces a number that looks like evidence. A tool that produced a false positive is fixed or deleted in the same change that retracts its output. A discredited tool left in place will be quoted again.

Never satisfy a check by changing what it measures. Do not remove the value it observes, do not edit the declaration it reads, and do not revert the change that tripped it. When an assertion fails because the product deliberately changed, update the assertion. When it fails because it found a defect, fix the defect or pin the broken behavior under a named marker (see the assertions child way).

A gate measures the effective state, read back through the path a real client takes. A check that reaches the service by a shortcut cannot see faults in the path it skipped. A control's presence in the code says nothing about its wiring. Verify the flag value on every path that can produce the outcome.

## Report format

One line per gate, state first:

```
- Linter: executed, `make lint`, PASS (2 warnings, both in docs/)
- Unit tests: executed, `cargo test`, PASS (47 cases)
- Secret scan: executed, `gitleaks detect`, 0 findings, control fixture detected
- End-to-end: discovered, left unrun, suite exists under e2e/, out of scope for this change
- Smoke test: absent (2026-09-09), no deploy target yet
```

Close the report with what was deliberately left unverified, blocked distinguished from skipped. A partial pass must never read as completion.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "All gates green, I just disabled the flaky one" | Fix the flake or record the gate as discovered with the reason. Silent disabling is a hidden absent. |
| "Tests pass locally, CI can wait" | Record where it ran. A gate that ran nowhere is a hope. |
| "No time for the full suite this round" | Merge a smaller change. Depth follows the change class. |
| "The scan came back clean" | Clean against what? Without a control the zero is unread. |
| "The check was wrong, so I fixed the file it reads" | The declaration now records a bug. Fix the instrument. |
| "The step exited zero" | An exit code says a process ended. Did its assertions run? |

## See Also

- code/testing/gates/assertions(softwaredev) — whether an executed assertion can have proved anything
- code/testing(softwaredev) — parent: what to cover and what to assert
- delivery/merge(softwaredev) — review depth and gate depth are chosen together
- environment/recovery(softwaredev) — a gate red twice means the increment is wrong-sized
