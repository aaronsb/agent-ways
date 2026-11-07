---
description: Landing an increment to main — the review gate is a four-square decision, not one policy. Choose machine-review depth by complexity and blast-radius, choose the human gate by whether the work sets direction, fix every finding the change owns before the merge rather than filing it for later, then merge and clean up. Route to the /merge skill.
vocabulary: merge land increment ship it review gate code review findings fix remediate approve pull request pr blast radius complexity swarm single agent operator read direction architecture merge cleanup delete branch four square gate before merge reviewer codeowners swarm-gate dispatch defer deferred follow-up non-blocking nit optional finding owned finding fix round review round filed later
pattern: /merge\b|merge (this|it|the pr)|land (this|it)|review.{0,20}(fix|and merge)|fix the findings|ship (it|this)|squash.?merge|rebase.?merge
commands: gh pr (merge|checks)
refire: 0.15
scope: agent
---
<!-- epistemic: heuristic -->
# Merging

Landing an increment is the **stable tail** of the develop loop, the part that runs the same way every time: review → fix → merge → clean up. The **`merge` skill** runs it. This way sets the review gate for each change: how deep the review goes, and whether a person reads the change before the merge, depend on the work.

## The repository's enforcement decides first

Every project sets its own review policy, and the strict ones encode it mechanically: a ruleset or protected branch that requires approvals, a CODEOWNERS file, required status checks, commit hooks. Before classifying, read what this repository enforces. That enforcement is the project's policy and it decides. The judgement below applies only where nothing enforces review, and it can add review to what a mechanism requires but never remove any. When the policy cannot be read (no forge CLI, no access, an unfamiliar host), treat review as required.

| Where | What to read |
|---|---|
| Default branch | `git symbolic-ref --short refs/remotes/origin/HEAD` |
| GitHub | `gh api repos/{owner}/{repo}/rules/branches/<branch>`; `gh api repos/{owner}/{repo}/branches/<branch>/protection` (a 404 means none) |
| GitLab | `glab api projects/:id/protected_branches`; `glab api projects/:id/approval_rules` |
| Any repository | `CODEOWNERS` at the root, `.github/`, `.gitlab/` or `docs/`; `git config core.hooksPath`; `.pre-commit-config.yaml`; `.husky/` |

## The four-square

Classify the increment on two axes and pick the path:

|                          | machine review: **light** (small, contained diff) | machine review: **deep / swarm** (complex, high blast-radius) |
|--------------------------|----------------------------------------------------|---------------------------------------------------------------|
| **human gate: none** (routine, or already read) | **run it** — a quick `code-review` (or skip) → merge | **swarm-gate** — fan review across dimensions, adversarially verify → agent-gated merge |
| **human gate: required** (sets direction, or unread one-way door) | **review + offer** — single-agent review → "want to read before I merge?" | **full gate** — swarm review **and** operator approval before merge |

- **The X axis** is driven by **complexity and blast-radius**: diff size, number of files, core-vs-leaf, test coverage, reversibility. A large change to a core path gets a swarm of reviewers, and a small change to a leaf gets a single review.
- **The Y axis** is driven by **whether the work sets direction** — an ADR that steers architecture lands in the bottom row no matter how small the diff — and by **whether the operator has already read it**. If they wrote it or reviewed it live, the human gate is already satisfied.

When the classification is unclear, **raise it with the operator** instead of picking a corner silently. Inside a `/goal` loop, lean toward the corners without a human gate; outside one, ask which corner applies.

In every corner, a request to merge is a request for the review that corner names. Start the reviewer without asking permission again, and say in a clause what the review buys. The question to raise is *which corner* applies; *whether to review* is already settled. See ADR-175.

## Version bumps merge on green CI

A release bump PR whose diff is only the package's version line and its lockfile entry merges once CI passes, without a review. The code it releases was reviewed when it merged, and the bump adds nothing to read. A bump that carries anything else, such as a changelog entry or a fix, takes the gate like any other change. It holds only where the repository enforces no review and its policy could be read.

## Green CI is a command

Green CI means `gh pr checks <n> --json bucket -q 'all(.[]; .bucket=="pass" or .bucket=="skipping")'` prints `true`. Run it as its own command and read the result before `gh pr merge`. Do not parse the text table. A pipeline's exit status is its last command's, so `gh pr checks 5 | cut -f1 && gh pr merge 5` merges whatever the checks say. A repository with no checks has no green CI to merge under; `gh pr checks` exits 1 there. Trust a guard script once you have seen it refuse a red case. After a merge or an auto-merge request, a hook names any check that was not green; tell the operator when it does.

## Remediate before you merge

A review is worth running only if its findings get fixed. This step is the easiest one in the loop to skip (ADR-613).

- **The change owns a finding** when the change caused it, when it sits inside a function, section or config block the diff touches, or when it sits in this repository's code that the behaviour the change claims runs through. A defect in a dependency maintained elsewhere is not owned, and a workaround for it in the change's own code is. A finding about a whole file, such as its length, is owned only when the change made it worse.
- **Fix every owned finding in the branch before the merge**, wording included, under the project's ADRs, instruction files and project ways. Where no convention covers the fix and the call sits below the waterline (choices), settle it, record it as an ADR, a project way or an instruction-file line, and say so in the PR. Above the waterline, ask the operator (choices/interview); the finding stays owned and open until they answer.
- **Filing an issue does not fix an owned finding.** Decline one only with evidence in the PR that it is wrong. Severity, cost and review fatigue are not grounds.
- **Deferring an owned finding is the operator's call.** Ask per finding, with what the fix costs now. Without that answer, fix it.
- **A finding the change does not own** is a residual (`delivery/issues`) and does not hold the merge.
- **A cap on review rounds** limits how many rounds run, not what a round fixes. A review brief asks for findings to fix, not for a follow-up list.

Do not merge with an owned finding open.

| Rationalization | Counter |
|---|---|
| "None of these blocks the merge" | Blocking is not the test. Ownership is. Fix what the change owns. |
| "It's only wording, it can go in a follow-up" | Wording is the cheapest fix there is. Make it in this round. |
| "I filed an issue, so it's recorded" | A filed issue is a record that the fix was skipped. |
| "Another round would cost a cycle" | Fix it in the round already running. The cap limits rounds, not fixes. |

## Then land it

Merge with a regular merge commit, which keeps the branch's commits in the history (see `delivery/github`), then run the full cleanup: back to `main`, pull, prune, and delete the merged branch. Merging an increment does **not** cut a release. Versions, changelogs and artifacts belong to `delivery/release` and the `/release` skill, a separate and larger step.

## Reconcile the decision ledger

An ADR that authorized the work stays at `Draft` until the work merges. Once the merge lands, the ADR describes the system, so set it to `Accepted` during the cleanup. Run `adr archive` on an ADR that the merge supersedes, or that the work showed was unnecessary.

No check fails while records drift. An audit in August 2026 found seventeen ADRs still at `Draft` or `Proposed` with their work long shipped; a one-line edit at each merge would have kept them current.

## See Also

- delivery/github(softwaredev) — PR creation, merge strategy, and post-merge cleanup discipline.
- delivery/commits(softwaredev) — the commit quality the PR is built from.
- delivery/release(softwaredev) — the heavier sibling: publishing a versioned release.
- delivery/groundwork/sequence(softwaredev) — a hold for an area that cannot yet be verified, decided before the four-square classifies the review.
- develop(meta) — the loop that this way finishes.
- code-reviewer (subagent, `agents/code-reviewer.md`) — the reviewer `merge` spawns at the gate; the built-in `/code-review` is the operator's manual equivalent.
