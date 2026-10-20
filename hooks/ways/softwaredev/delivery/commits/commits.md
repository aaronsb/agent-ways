---
description: git commit messages, conventional commits, atomic changes that split unrelated edits into separate commits, when to commit and when to stop, the pre-commit audit of the diff, and the status line on what stays uncommitted
vocabulary: commit message branch conventional feat fix refactor scope atomic squash amend stash rebase cherry trailers emoji co-authored-by prep stage staged unstaged untracked uncommitted gitignore scratch readme audit diff revertible push not pushed sha
pattern: push.{0,30}(remote|origin|upstream)|\bcommit (this|it|everything)\b|\bprep (a |the )?commit\b
commands: git\ commit
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Git Commits Way

## Commit Only When Asked

- Commit when the user asks in this session, and do only the step they named. A commit is not a push.
- "Prep" means stage and audit, then report ready and stop.
- When the user tests by hand, finish the change, say what to test, and wait for their word before committing.
- An approved plan covers only the commits and pushes it lists.
- In a directory with no repository, do not create one to commit.

## Before Committing

1. Check the branch and `git status`. On the default branch, branch first (delivery/branching) unless the user named main.
2. Leave uncommitted changes you did not make in place. Build on them and do not discard them.
3. Bring the README and index docs to the final state in the same commit. Document only what testing confirmed.
4. Remove temp and debug scripts, test outputs, and scratch files.
5. Gitignore large or generated files and every file that holds a key or token.
6. Hold back personal-data and dev-only files, and say that you did.
7. Grep the diff for token prefixes, internal URLs, and machine or account paths (code/security/secrets).
8. Stage files by name.

## Conventional Commit Format

Scopes match the area of change: `ways`, `hooks`, `adr`, `docs`, `config`, `governance`, or the specific way/feature name.

- `feat(scope): description` - New features
- `fix(scope): description` - Bug fixes
- `docs(scope): description` - Documentation
- `refactor(scope): description` - Code improvements
- `test(scope): description` - Tests
- `chore(scope): description` - Maintenance

## The Message Body

The subject is imperative and specific. The body records the work since the last commit: what was asked, what was tried, what was found and decided, any pivot, the final solution, and the exact configuration changes. Leave out the list of files, because the diff already has it. Write a one-line message only when asked. A PR title matches its commit subject. Re-read the message before you run `git commit`, because a message that lands on a protected main cannot be rewritten.

## Branch Names

- `adr-NNN-topic` - Implementing an ADR
- `feature/name` - New feature work
- `fix/issue` - Bug fixes
- `refactor/area` - Code improvements

## Rules

- Skip "Co-Authored-By" and emoji trailers
- Focus commit message on the "why" not the "what"
- Keep commits atomic - one logical change per commit
- Commit once per accepted unit of work and before any phase change, so each step can be reverted alone

## After Committing

Report what happened in one line: "Committed as `<sha>`, not pushed," or which remotes received the push. Name any file left untracked on purpose. When a turn ends with nothing committed, say so, list the uncommitted files, and propose how to split them into commits.

## See Also

- delivery/github(softwaredev) — commits feed into PRs
- delivery/release(softwaredev) — commit types drive version bumps
- delivery/branching(softwaredev) — branch before the first commit
- code/security/secrets(softwaredev) — what the diff audit looks for
