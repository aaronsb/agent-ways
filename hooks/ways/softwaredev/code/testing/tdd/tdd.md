---
description: test-driven development, TDD red-green-refactor cycle, failing test first
vocabulary: tdd red green refactor test first implementation failing
scope: agent, subagent
refire: 0.2
---
<!-- epistemic: heuristic -->
# TDD Way

## The Cycle

1. **Red** — Write a failing test that describes the behavior you want
2. **Green** — Write the minimum code to make the test pass
3. **Refactor** — Clean up without changing behavior (tests still pass)

## When TDD Applies

- New functions with clear input/output contracts
- Bug fixes (write the test that would have caught it, then fix)
- Refactors where you want confidence the behavior is preserved

## When TDD Doesn't Apply

- Exploratory prototyping (write tests after the shape solidifies)
- Pure UI layout (visual testing is better)
- Glue code that only wires dependencies together

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "This is simple, tests aren't needed" | If it's simple, the test is trivial to write. Write it. |
| "I'll add tests later" | Later never comes. The test verifies understanding NOW. |
| "Existing tests cover this" | Prove it. Run them and show they exercise the new path. |
| "Just a refactor, behavior doesn't change" | Then existing tests pass. Run them. If none exist, write them first. |
| "Writing tests would take too long" | Debugging the regression takes longer. |
| "The user didn't ask for tests" | The user asked for working code. Tests prove it works. |
