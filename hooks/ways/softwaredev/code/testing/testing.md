---
description: test coverage, test structure, assertions, fixtures, what and how to test
vocabulary: test coverage assertion framework spec fixture describe expect verify unit integration
commands: npm\ test|yarn\ test|jest|pytest|cargo\ test|go\ test|rspec
scope: agent, subagent
refire: 0.2
---
<!-- epistemic: convention -->
# Testing Way

## What to Cover

For each function under test:
1. **Happy path** — expected input produces expected output
2. **Empty/null input** — handles absence gracefully
3. **Boundary values** — min, max, off-by-one, empty collections
4. **Error conditions** — invalid input, dependency failures

## Structure

- Arrange-Act-Assert: setup, call, verify
- Name tests: `should [behavior] when [condition]`
- One logical assertion per test — test one behavior, not one line
- Tests must be independent — no shared mutable state between tests

## What to Assert

- Observable outputs and side effects only
- Never assert on method call counts or internal variable values
- If you need to reach into private state, the design needs rethinking

## Project Detection

Detect the test framework from project files (package.json, requirements.txt, Cargo.toml, go.mod). Follow its conventions for file placement and naming.

## See Also

- code/testing/mocking(softwaredev) — when and how to mock
- code/testing/tdd(softwaredev) — test-driven development cycle
- code/quality(softwaredev) — tests enforce quality thresholds
