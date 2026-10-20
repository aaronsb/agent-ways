---
description: Git branch awareness when editing files, noticing you are on main or the trunk and cutting a topic branch before the first edit, carrying other people's uncommitted work onto it, opening a pull request when the branch is pushed, and one branch per piece of work
vocabulary: branch checkout worktree main trunk feature fix refactor force-push temp tweaks context-switch topic-branch default-branch switch first-edit named-branch push-u fork upstream follow-up uncommitted carry
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

## Before the First Edit

When the work calls for a branch, cut it before the first edit:

1. Run `git status` and `git branch --show-current`. Find the default branch with `git symbolic-ref refs/remotes/origin/HEAD` or `gh repo view --json defaultBranchRef`.
2. On the default branch, pull it, then run `git switch -c <type>/<short-topic>`. Use the name the user gave, if any.
3. State the branch name in one line and keep going. A local branch is cheap to undo, so there is no need to stop and ask.
4. Carry uncommitted changes you did not make onto the new branch. Do not stash or discard them.

When the user names a branch, create it with that name, confirm you are on it, and wait for their next instruction.

## A Pushed Branch Gets a PR

When the user asks to push a topic branch, run `git push -u origin <branch>` and open the pull request in the same step, unless they say otherwise. Report the PR URL. Committing and merging still wait for the user's word.

## One Branch per Piece of Work

- Push follow-up commits to the open PR's branch.
- Start unrelated work on a new branch cut from a freshly pulled default branch.
- After a merge, switch back, pull, and delete the local branch.

## Exceptions

- The user names main as the target, as in "commit to main". Do exactly that.
- The repository has no remote. Branch and commit, and skip the PR.
- The repository is a fork. Push the branch to the fork and open the PR against upstream.

## See Also

- delivery/commits(softwaredev) — when to commit and how to word it
- delivery/github(softwaredev) — opening, reviewing, and merging the PR
- delivery/merge(softwaredev) — landing the branch and cleaning up
