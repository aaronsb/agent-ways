---
description: code quality, refactoring, SOLID principles, code review standards, technical debt, maintainability
vocabulary: refactor quality solid principle decompose extract method responsibility coupling cohesion maintainability readability
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
| File > 800 lines | Flag as priority — split before adding more code |
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
| "There's no good place to split" | Can't find a split point = coupling problem worth solving. |
| "I'll refactor later" | The file will only grow. Split it now while the logic is fresh. |

## What to Enforce

A validation — a lint rule, an assertion, a schema constraint, a CI check —
**earns its place only if its violation is a real defect** someone would eventually hit. The good ones track genuine failure: a dangling reference, a state that can't be reached, a contract that's silently broken. The trap is the internally-consistent invariant that tracks *nothing* real ("every page must have exactly three links") — it passes, it feels rigorous, and it costs maintenance forever while catching no bug. This matters more, not less, when an agent sustains the checks: an agent will happily maintain a pointless invariant indefinitely, so nothing surfaces that it was never worth adding. Before adding a check, name the defect its failure would represent. If you can't, don't add it.

## See Also

- code/testing(softwaredev) — quality requires test coverage
- code/errors(softwaredev) — error handling is a quality signal
- tooling(softwaredev) — encode enforced conventions in tools, not manual process
- standards(documentation) — standards define quality expectations
