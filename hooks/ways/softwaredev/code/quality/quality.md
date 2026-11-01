---
description: code quality, refactoring, SOLID principles, code review standards, technical debt, maintainability
vocabulary: refactor quality solid principle decompose extract method responsibility coupling cohesion maintainability readability idioms ecosystem invariant
pattern: solid.?principle|refactor|code.?review|code.?quality|clean.?up|simplify|decompos|extract.?method|tech.?debt
refire: 0.2
macro: append
scan_exclude: \.md$|\.lock$|\.min\.(js|css)$|\.generated\.|\.bundle\.|vendor/|node_modules/|dist/|build/|__pycache__/
scope: agent, subagent
requires: ["Read", "Bash(awk:*)", "Bash(dirname:*)", "Bash(file:*)", "Bash(git:*)", "Bash(grep:*)", "Bash(head:*)", "Bash(sort:*)", "Bash(wc:*)"]
---
<!-- epistemic: heuristic -->
# Code Quality Way

## Quality Flags — Act on These

| Signal | Action |
|--------|--------|
| File > 500 lines | Propose a split with specific module boundaries |
| File > 800 lines | Flag as priority and propose the split. Split when asked. |
| Function > 3 nesting levels | Extract inner logic into named helper functions |
| Class > 7 public methods | Decompose — likely violating Single Responsibility |
| Function > 30-50 lines | Break into steps with descriptive names |

When the file length scan (macro output) shows priority files, call them out explicitly before proceeding with the task.

## Ecosystem Conventions

- Don't introduce patterns foreign to the language/ecosystem
- Examples to avoid:
  - Rust-style Result/Option in TypeScript
  - Monadic error handling where exceptions are standard
  - Custom implementations of what libraries already provide

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "Splitting this file would make it harder to follow" | A 900-line file is already hard to follow. Split at natural seam lines. |
| "There's no good place to split" | When no split point exists, the code is too tightly coupled, and that is worth fixing. |
| "I'll refactor later" | The file will only grow. Propose the split now, while the logic is fresh. |

## What to Enforce

**Add a validation, such as a lint rule, an assertion, a schema constraint or a CI check, only when breaking it is a real defect** that someone would eventually hit. A useful check catches a real failure: a reference to something that does not exist, a state the code cannot reach, a contract that breaks without an error. A rule such as "every page must have exactly three links" can be consistent and always pass, yet it catches no bug and still costs maintenance. An agent that maintains the checks keeps such a rule alive indefinitely, so nobody notices that it was never worth adding. Before adding a check, name the defect its failure would represent. If you can't, don't add it.

## See Also

- code/quality/integration(softwaredev) — a change integrates into the whole file; forbidden patch moves, the class sweep
- code/testing(softwaredev) — quality requires test coverage
- code/errors(softwaredev) — error handling is a quality signal
- tooling(softwaredev) — encode enforced conventions in tooling
- standards(documentation) — standards define quality expectations
- code/quality/edit-safety(softwaredev) — edit the source of generated output; files someone else is editing
