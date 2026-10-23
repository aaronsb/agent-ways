---
description: GitHub pull requests, issues, review comments, CI checks
vocabulary: github pull request pull requests pr prs issue issues fork upstream label labels milestone branch protection codeowners gh cli review comments ci checks squash rebase badges shields.io post-merge boards
pattern: github|\bgh (pr|issue)\b|issue #?\d+|github issues?|pull.?requests?|\bprs?\b|review.?(pr|comment)|merge.?request
commands: (^|[;&|(]\s*)gh(\s|$)|\bgit\s+push\b
refire: 0.15
macro: prepend
scope: agent, subagent
requires: ["Read", "Bash(cat:*)", "Bash(gh:*)", "Bash(git:*)", "Bash(grep:*)", "Bash(head:*)", "Bash(jq:*)", "Bash(rm:*)", "Bash(sed:*)", "Bash(sort:*)", "Bash(tr:*)", "Bash(wc:*)"]
---
<!-- epistemic: convention -->
# GitHub Way

## Pull Requests, Always

Every change lands through a PR, solo projects included. A PR with no reviewer is still a decision record and a CI gate.

- **Solo or pair:** a title and a few bullets.
- **Team:** context, reviewers, linked issues.
- **Three or more contributors:** consider automated PR review such as Claude Code Review.

## Review Before Merge

Review after opening the PR. Dispatch a `code-reviewer` subagent for it when the user asked to open or land the PR, a skill in use directs it, or a CLAUDE.md line covers it. Otherwise, where your instructions restrict the Agent tool, propose the review in one line or review inline (ADR-608). Review depth and whether a human reads first are the four-square decision in `delivery/merge`. Scale the reviewer to a swarm for high-blast-radius changes, and gate on operator approval when the work sets direction. "Merge it" is a request for that review. Merge strategy is a separate question, settled by the default below. The repository's own enforcement comes first, and a version-only release bump can skip review where nothing enforces it; see `delivery/merge`.

## Merge Strategy: Regular Merge by Default

Default to `gh pr merge --merge`. A branch with an ADR, an implementation, and review fixes carries a narrative that `git log` on main should keep. Squash only when the branch is single-purpose with commit noise worth dropping. Never rebase-merge unless asked; it rewrites authorship and timestamps.

When the user says "merge it" without a strategy, use the regular merge and name it in the reply.

## Post-Merge Cleanup

After every merge: `git checkout main && git pull && git fetch --prune`, then `git branch -d <branch>`.

## Repo Health

The macro checks repository configuration (README, license, templates, branch protection, badges) and reports gaps. Offer to fix the items the user has rights to. Note the ones needing admin access without pushing. When badges are missing, suggest shields.io badges under the README title.

## Keep It Light

Issues for requirements and bugs, a basic label set, no project boards or milestone hierarchies. When "issue" could mean a GitHub issue or a problem to investigate, ask which.

## See Also

- delivery/commits(softwaredev) — PR quality depends on commit quality
- delivery/merge(softwaredev) — the review-gate decision and landing an increment
- delivery/issues(softwaredev) — issues mirrored into the session task list
- adr(documentation) — reference ADRs in PR descriptions
- delivery/github/follow-through(softwaredev) — CI on main after the merge, parked and closed issues, contributing to another project
