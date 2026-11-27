---
description: mocking dependencies, test doubles, fakes, stubs, spies, dependency injection for tests, and when a mock's call count may be asserted
vocabulary: mock fake stub spy double dependency inject external isolate test double flaky non-deterministic randomness third-party filesystem call count called times toHaveBeenCalledTimes invocation count
scope: agent, subagent
refire: 0.2
---
<!-- epistemic: heuristic -->
# Mocking Way

## What to Mock

- **External dependencies** — network, filesystem, databases, third-party APIs
- **Slow or non-deterministic operations** — time, randomness, system calls

## What NOT to Mock

- The code under test or its internal helpers
- Value objects or simple data structures
- Anything you control and can test directly

## Prefer Fakes Over Mocks

In-memory implementations (fakes) are more reliable than mock libraries:
- Fakes exercise real behavior, mocks only verify method calls
- Fakes don't break when implementation details change
- Use mock libraries only when fakes would be too complex to write

## Mock Hygiene

- Reset mocks between tests — shared mock state causes flaky tests
- If you need more than 3 mocks in a test, the code has too many dependencies — refactor first

## Call Counts

Assert a mock's call count only when the count is the behavior the code promises, such as exactly one charge per order, or no retry after a permanent error. Any other count passes for the right call and keeps passing when the code calls the collaborator the same number of times for a wrong reason. Assert on the destination, the content, or the argument the code derived instead. What to assert in its place, and the change-detector test that copies hardcoded arguments into the test, are in code/testing/gates/assertions.

## See Also

- code/testing(softwaredev) — parent: what to cover and what to assert
- code/testing/gates/assertions(softwaredev) — mock assertions that prove something, and change-detector tests
