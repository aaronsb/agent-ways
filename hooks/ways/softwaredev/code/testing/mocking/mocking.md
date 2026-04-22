---
description: mocking dependencies, test doubles, fakes, stubs, spies, dependency injection for tests
vocabulary: mock fake stub spy double dependency inject external isolate test double
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
- Don't assert on call counts unless the count IS the behavior
- If you need more than 3 mocks in a test, the code has too many dependencies — refactor first
