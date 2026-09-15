---
description: separating what a pull request, issue, or agent report says about itself from what it demonstrates, treating the description as a claim and asking what would show it wrong
vocabulary: claim evidence self-report writeup narrative pr body issue body summary says it does polished description convincing plausible falsify disprove shown wrong take their word
scope: agent, subagent
refire: 0.2
---
<!-- epistemic: heuristic -->
# Contributions as Claims

Every contribution arrives with its own account of its value. A PR description says what the change does. An issue says what is broken and that it matters. An agent's closing message says the tests are thorough. Each of these is a **claim**, and the artifact does not become evidence for it by being well written.

We grade the evidence, not the description. A polished writeup tells us how much care the author took. It tells us nothing about whether the change works.

## Three questions for every item

1. **What does this claim?** State it in one sentence, in our words.
2. **What would show the claim wrong, and does the contribution include that?** A test that fails if the change is reverted. A reproduction that goes green. A measured number with the command that produced it.
3. **Who other than the author has run it?** A claim nobody has tried to break has not been reviewed; it has been read.

An item that can answer all three is a different kind of object from one that answers only the first. Sort by that, not by prose quality.

## Our own reports are claims too

The agent saying "I wrote comprehensive tests" is the same class of statement as a contributor saying it. Report what ran and what it would have caught (see `testing/gates`). A test written by copying the current output proves the code does what it does; if the requirement and the implementation disagree, we have preserved a bug very efficiently. Say which one the test encodes.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "The description is thorough and clearly written" | Care is not correctness. Find the line that could fail. |
| "The author is experienced" | Then their evidence should be easy to find. Ask for it, not for trust. |
| "CI is green" | Green says the intended path ran. It does not say the claim is true. |
| "It's just an issue, not code" | An issue asserts a defect exists and matters. Both are checkable. |
| "The agent said the tests are good" | Let someone else conduct its performance review. |

## See Also

- code/security/contributions(softwaredev) — the adversarial lens: does the diff do *more* than it says
- code/testing/gates(softwaredev) — reporting what actually ran
- code/testing/gates/assertions(softwaredev) — whether an executed assertion could have proved anything
- delivery/groundwork(softwaredev) — parent: readiness before change
