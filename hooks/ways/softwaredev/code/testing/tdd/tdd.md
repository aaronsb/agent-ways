---
description: test-driven development, TDD red-green-refactor cycle, failing test first, characterization tests on untested or legacy code and when they expire, proving an inherited green suite by reintroducing the defect
vocabulary: tdd red green refactor test first implementation failing characterize characterization legacy inherited untested suite mutation defect regression exploratory prototyping glue unproven
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

## Untested Code First

The cycle assumes you can state what the code should do. On inherited code with no test and no spec, a test asserting the correct answer encodes a guess. Pin what the code does today instead, defects included.

1. Write a test that asserts the current behavior. Name it with the `characterizes_` prefix. The name marks it as a description of present behavior. Note in its body any behavior you believe is wrong, with the issue that tracks the fix.
2. Run it. It passes.
3. Make the change. The characterization test now fails in the way you intended. That failure is the RED, and the cycle resumes.

A `characterizes_` test expires once the behavior it pins is fixed or decided. Then rename it to state the decided behavior and assert the correct answer, or delete it when another test already covers that behavior. A characterization test left in place after the decision keeps asserting a guess.

An inherited suite that is green when you arrive is unproven until a planted defect has turned it red, by the procedure in code/testing/gates. On inherited code, the plant to choose is the past defect that a test claims to guard against.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "This is simple, tests aren't needed" | If it's simple, the test is trivial to write. Write it. |
| "I'll add tests later" | Later never comes. The test verifies understanding NOW. |
| "Existing tests cover this" | Prove it with a planted defect (code/testing/gates). |
| "Just a refactor, behavior doesn't change" | Then existing tests pass. Run them. If none exist, write them first. |
| "Writing tests would take too long" | Debugging the regression takes longer. |
| "The user didn't ask for tests" | The user asked for working code. Tests prove it works. |

## See Also

- code/testing(softwaredev) — parent: what to cover, test levels, test data
- code/testing/gates(softwaredev) — a gate earns trust once a planted defect has turned it red
- freshness/groundtruth(softwaredev) — the golden-master baseline captured before a refactor or migration
