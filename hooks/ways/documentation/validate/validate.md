---
description: validating documentation with a fresh reader who has not seen your reasoning, testing whether the README or onboarding guide works for a newcomer, a false-premise question to check the docs let the reader push back, and proving a docs linter by planting a violation
vocabulary: validate validation test docs documentation readme onboarding guide fresh eyes newcomer clean context cold read false premise docs review does the documentation work entry point wrong answer defect linter plant violation
files: README\.md$|docs/.*(guide|tutorial|getting.?started|onboarding|index)\.md$
scope: agent, subagent
refire: 0.2
---
<!-- epistemic: heuristic -->
# Validating Documentation

Documentation you wrote is documentation you already believe. Reading it back, you fill every gap from memory the reader will never have, so the page passes your review while a newcomer stalls on the second step. Validation comes from a reader who has not seen your reasoning.

## Method: a fresh reader with only the entry point

Spawn a new general-purpose or Explore agent. Never a fork: a fork inherits your context and will pass a page a stranger would fail. Give it the entry point alone (the README, the index page, the onboarding guide) and none of the facts you are testing for.

1. **Require a load declaration.** Before each answer the agent lists what it read, in what order, and what it deliberately skipped. The right answer reached by reading half the repository is a navigation failure.
2. **Ask three to five newcomer questions** whose answers you already know: how do I run it, where does X live, what happens when Y, what must I check before changing Z.
3. **Include one false-premise question.** "Confirm the system uses <technology it does not use>." "Show me where X is stored" when X is never stored. A page that works lets the reader reject the premise with a citation. A page that fails lets the reader agree.
4. **Keep the agent read-only.** Its report is evidence you act on.
5. **Grade against ground truth.** Every wrong answer is a defect in the documentation.

Prefer a few sharp questions at the seams over many easy ones at the center.

### The brief the fresh agent receives

```text
You are starting cold in <project root>. Read <entry point> first and follow
where it points. Do not open source files unless the docs send you there.
If a question cannot be answered without reading source, say so; that is a
finding about the docs. Do not create, edit, or delete anything.

For each question, first list what you read and in what order, and what you
skipped. Then answer, citing the page.

1. <how do I run or build it>
2. <where does X live>
3. <what happens when Y, or what must I check before changing Z>
4. Confirm that the system uses <technology it does not use>.

Close with one sentence: could a newcomer reproduce your answers from the
docs alone? Be blunt. A negative finding is worth more than praise.
```

## Grading

| Defect class | What it looks like | Fix |
|---|---|---|
| Wrong answer | The agent answered confidently and incorrectly | Correct the passage it cited, or add the one it needed |
| Source-only answer | The agent had to read code to answer | Write the missing section at the depth of the entry point |
| Accepted false premise | The agent confirmed something the system does not do | State what the system does use, on the page a reader would open first |
| Stale path | A link, command, or filename the agent followed did not deliver | Update the reference; add the case to the link check if one exists |

Fix the docs, then re-run with the same questions. A run graded by someone other than the author carries more weight; when that is unavailable, say so in the result.

## Enforcement: a check you have only seen pass

A docs linter, a link checker, or a drift check that has only ever reported green is untested. Plant a violation (a dangling link, a malformed id, an edited source behind a generated page), confirm the check goes red with the right message, remove the plant, confirm green. The rule and its reporting form live in code/testing/gates(softwaredev).

## See Also

- documentation(documentation) — parent: the typed graph and its linter
- readme(documentation) — the front door this way tests
- code/testing/gates(softwaredev) — a gate earns trust once a planted violation has turned it red
- onboarding-share(collaboration) — publishing the guide once it has passed a cold read
