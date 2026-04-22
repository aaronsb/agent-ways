---
description: Git branch awareness and branching guidance when editing files
vocabulary: branch checkout worktree main trunk feature fix refactor
files: \.(md|rs|sh|py|js|ts|json|yaml|yml|toml|go|rb|java|c|cpp|h|hpp|css|html|sql)$
refire: 0.1
macro: prepend
scope: agent, subagent
requires: ["Bash(cut:*)", "Bash(git:*)", "Bash(sed:*)"]
---
<!-- epistemic: heuristic -->
# Branching Context

## Where You Are Matters

The macro above shows the current git branch and state. Glance at it before writing — if you're on `main` and about to make a non-trivial change, consider branching first.

## When to Branch

This is guidance, not a gate. Use judgment:

| Situation | Branch? |
|-----------|---------|
| Exploration, temp edits, quick config tweaks | Main is fine |
| Bug fix that'll become a PR | Yes — `fix/description` |
| New feature or capability | Yes — `feat/description` |
| Refactoring across multiple files | Yes — `refactor/description` |
| Documentation updates | Judgment call — `docs/description` if substantial |

## Why It Matters

- Branches make work reversible without `git stash` gymnastics
- A branch is a PR draft — you can push it and walk away
- Committing to main means force-push is the only undo for public repos
- Branches let you context-switch cleanly between tasks

## Branch Naming

Use prefixes that match conventional commit types:

- `fix/` — bug fixes
- `feat/` — new features
- `docs/` — documentation
- `refactor/` — restructuring without behavior change
- `adr-NNN-topic` — ADR implementation work

Keep names short, lowercase, hyphen-separated. The branch name often becomes the PR title.
