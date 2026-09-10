---
description: test coverage, test structure, assertions, fixtures, what and how to test, choosing the lowest test level that proves the contract, and synthetic fixtures instead of a copy of production data
vocabulary: test coverage assertion framework spec fixture describe expect verify unit integration contract end-to-end e2e golden snapshot property evaluation test level mock synthetic seed data production copy sample anonymize mask
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

## Test Levels

Pick the lowest level that proves the contract.

| Level | What it proves |
|---|---|
| Unit | Pure logic (a transformation, parser, or validator) gives the right output |
| Integration | The code crosses a real adapter (database, file, network, SDK, model) correctly |
| Contract | An endpoint, message, or structured output matches its published schema |
| End-to-end | A critical user flow works through the whole stack; one or two per flow |
| Golden | A deterministic transform, prompt, or renderer still produces the reviewed baseline |
| Property | An algorithm holds an invariant across generated inputs |
| Evaluation | Model behavior meets a rubric at a stated pass threshold |

A unit test that mocks three layers belongs one level up. An end-to-end test of a pure function belongs several levels down.

## Test Data

Fixtures, seed data, demo environments, and examples in prompts are synthetic: generated to match the shape and constraints of real records without being any real record. Masking is not anonymization. A production sample copied to reproduce a bug is a disclosure.

## Project Detection

Detect the test framework from project files (package.json, requirements.txt, Cargo.toml, go.mod). Follow its conventions for file placement and naming.

## See Also

- code/testing/mocking(softwaredev) — when and how to mock
- code/testing/tdd(softwaredev) — test-driven development cycle
- code/testing/gates(softwaredev) — every gate reports executed, discovered, or absent; a zero needs a positive control
- code/quality(softwaredev) — tests enforce quality thresholds
- freshness/groundtruth(softwaredev) — the golden-master baseline captured before a refactor or migration
