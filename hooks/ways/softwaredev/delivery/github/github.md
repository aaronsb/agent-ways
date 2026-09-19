---
description: GitHub pull requests, issues, review comments, CI checks
vocabulary: github pull request pull requests pr prs issue issues fork upstream label labels milestone branch protection codeowners gh cli review comments ci checks
pattern: github|\bgh (pr|issue)\b|issue #?\d+|github issues?|pull.?requests?|\bprs?\b|review.?(pr|comment)|merge.?request
commands: ^gh\ |^gh$
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

Review after opening the PR without waiting to be asked. Review depth and whether a human reads first are the four-square decision in `delivery/merge`. At minimum dispatch a `code-reviewer` subagent; scale to a swarm for high-blast-radius changes, and gate on operator approval when the work sets direction. "Merge it" is a request for that review, so dispatch it (ADR-175). Merge strategy is a separate question and still gets asked.

## Merge Strategy: Regular Merge by Default

Default to `gh pr merge --merge`. A branch with an ADR, an implementation, and review fixes carries a narrative that `git log` on main should keep. Squash only when the branch is single-purpose with commit noise worth dropping. Never rebase-merge unless asked; it rewrites authorship and timestamps.

When the user says "merge it" without a strategy, ask: regular merge or squash?

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
