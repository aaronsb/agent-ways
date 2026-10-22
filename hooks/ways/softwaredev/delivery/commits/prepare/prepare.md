---
description: removing leftover debug scripts and scratch files before git commit, leaving another person's uncommitted edits alone, adding files by name, committing at each accepted step, and a commit body that records findings and decisions
vocabulary: debug scripts scratch files output dumps throwaway leftover someone else's edits git add path add by name git add -A commit body findings re-read revertible
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Preparing a Commit

## When to Commit

When committing is in scope (the user asked, a procedure they asked for commits, or their CLAUDE.md grants it), commit once per accepted unit of work and before any phase change, so each step can be reverted alone.

## Before Committing

1. Leave uncommitted changes you did not make in place. Build on them. Do not reset, stash or overwrite them unless their owner calls them disposable.
2. Remove temp and debug scripts, test outputs, and scratch files.
3. Stage files by name. `git add -A`, `git add .` or a bare `git stash -u` sweep up other sessions' work.

## The Message Body

The body says why: what was asked, what was found and decided, any pivot, and the exact configuration changes. Leave out the list of files, because the diff already has it. Re-read the message before you run `git commit`, because a message that lands on a protected main cannot be rewritten.

## See Also

- delivery/commits(softwaredev) — parent: conventional format and atomic commits
- code/security/secrets/handling(softwaredev) — the scan of the diff before a commit
