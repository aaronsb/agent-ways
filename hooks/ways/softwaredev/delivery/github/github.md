---
description: GitHub pull requests, issues, review comments, CI checks, the issue loop from opening to closing with a commit link, triaging open issues by critical path, the commit-push-PR-merge chain, and reviewing a teammate's PR for substance rather than nits
vocabulary: github pull request pull requests pr prs issue issues fork upstream label labels milestone branch protection codeowners gh cli review comments ci checks squash rebase badges shields.io post-merge boards park resume triage critical path dependency graph orphans clusters runner-up close issue commit link teammate nits contribute upstream one fix per pr trade-off not in scope
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

Review after opening the PR without waiting to be asked. Review depth and whether a human reads first are the four-square decision in `delivery/merge`. At minimum dispatch a `code-reviewer` subagent; scale to a swarm for high-blast-radius changes, and gate on operator approval when the work sets direction. "Merge it" is a request for that review, so dispatch it (ADR-175). Merge strategy is a separate question and still gets asked. The repository's own enforcement comes first, and a version-only release bump can skip review where nothing enforces it; see `delivery/merge`.

## Merge Strategy: Regular Merge by Default

Default to `gh pr merge --merge`. A branch with an ADR, an implementation, and review fixes carries a narrative that `git log` on main should keep. Squash only when the branch is single-purpose with commit noise worth dropping. Never rebase-merge unless asked; it rewrites authorship and timestamps.

When the user says "merge it" without a strategy, ask: regular merge or squash?

## Post-Merge Cleanup

After every merge: `git checkout main && git pull && git fetch --prune`, then `git branch -d <branch>`. When a workflow runs on main (CI, Pages, a deploy), confirm it finished green before calling the work landed.

## One Request, One Chain

"Commit, push, PR, and merge" is one request. Pull the latest main, branch, commit, `git push -u`, `gh pr create`, review per `delivery/merge`, fix or decline each finding, merge, then clean up. Skip the PR only when the user says to merge directly. Self-approve only where the repository codifies it. Use an admin override only when branch protection requires it and the user asked.

## Repo Health

The macro checks repository configuration (README, license, templates, branch protection, badges) and reports gaps. Offer to fix the items the user has rights to. Note the ones needing admin access without pushing. When badges are missing, suggest shields.io badges under the README title.

## The Issue Loop

- Feature work starts from an issue. Open one when none exists.
- When parking an idea, file the issue before any build. Give enough context, files, and decisions to resume from the issue alone. Describe the need, and propose no solution unless asked.
- Record later decisions, such as which variant shipped, as comments. Leave the body as filed, and leave the issue open while discussion is pending.
- Close with a comment that says what was done and links the commit. Act with the account that owns the repository.

## Triage

When asked to triage all open issues, pull every one. Write "Issue landscape (as of <date>)", then a dependency graph: the critical path, clusters, items blocked from outside, and orphans. Recommend one issue with numbered reasons it beats the rest, and name a runner-up. Plan only the pick.

## Reviewing Someone Else's PR

Flag contradictions with the repository's purpose, style, or other content. Flag false statements and decisions that belong to the group. Skip wording nits and prescribed rewrites. Open with what you verified, phrase doubts as questions, and leave fixes to the author.

## Contributing Upstream

One fix per PR. The body carries Summary, Why, Trade-off, Test plan, and Not in scope. The title matches the commit subject. When the user names a reviewer, request their review.

## Keep It Light

Issues for requirements and bugs, a basic label set, no project boards or milestone hierarchies. When "issue" could mean a GitHub issue or a problem to investigate, ask which.

## See Also

- delivery/commits(softwaredev) — PR quality depends on commit quality
- delivery/merge(softwaredev) — the review-gate decision and landing an increment
- delivery/issues(softwaredev) — issues mirrored into the session task list
- adr(documentation) — reference ADRs in PR descriptions
- delivery/groundwork/sequence(softwaredev) — ordering a queue of issues and PRs so verification lands first
