---
description: Landing an increment to main — the review gate is a four-square decision, not one policy. Choose machine-review depth by complexity and blast-radius, choose the human gate by whether the work sets direction, remediate the findings, then merge and clean up. Route to the /merge skill.
vocabulary: merge land increment ship it review gate code review findings fix remediate approve pull request pr blast radius complexity swarm single agent operator read direction architecture merge cleanup delete branch four square gate before merge
pattern: /merge\b|merge (this|it|the pr)|land (this|it)|review.{0,20}(fix|and merge)|fix the findings
refire: 0.15
scope: agent
---
<!-- epistemic: heuristic -->
# Merging

Landing an increment is the **stable tail** of the develop loop: review → fix → merge → clean up. The **`merge` skill** runs it. The judgement this way carries is that the *review gate is not one policy* — how hard to review, and whether a human reads before merge, depend on the work.

## The four-square

Classify the increment on two axes and pick the path:

|                          | machine review: **light** (small, contained diff) | machine review: **deep / swarm** (complex, high blast-radius) |
|--------------------------|----------------------------------------------------|---------------------------------------------------------------|
| **human gate: none** (routine, or already read) | **run it** — a quick `code-review` (or skip) → merge | **swarm-gate** — fan review across dimensions, adversarially verify → agent-gated merge |
| **human gate: required** (sets direction, or unread one-way door) | **review + offer** — single-agent review → "want to read before I merge?" | **full gate** — swarm review **and** operator approval before merge |

- **The X axis** is driven by **complexity and blast-radius**: diff size, number of files, core-vs-leaf, test coverage, reversibility. A sprawling change to a core path earns a swarm; a contained leaf edit earns a single pass.
- **The Y axis** is driven by **whether the work sets direction** — an ADR that steers architecture lands in the bottom row no matter how small the diff — and by
  **whether the operator has already read it**. If they wrote it or reviewed it live,
  the human gate is already satisfied.

When the classification is ambiguous, **surface the call** rather than silently picking a corner. Inside a `/goal` loop, bias toward the autonomous corners; outside one, ask.

Whichever corner you land in, asking to merge is asking for the review this gate specifies — dispatch the reviewer rather than re-requesting permission to, and name the payoff in a clause as you do. The ambiguity worth surfacing is *which corner*, never *whether to review*. See ADR-175.

## Remediate before you merge

A review that finds nothing changes nothing; a review that finds something is only worth running if the findings get fixed. **Remediate per finding** before the merge gate — apply the fix, or record why a finding is declined. Don't carry known findings past the gate. This is the step the loop is easy to skip: "reviewed" is not "landed clean."

## Then land it

Merge (regular merge commit, preserving the branch's narrative — see `delivery/github`), then run the full cleanup: back to `main`, pull, prune, delete the merged branch. Landing an increment is **not** cutting a release — versioning, changelogs, and artifacts are `delivery/release` and the `/release` skill, a separate, heavier act.

## See Also

- delivery/github(softwaredev) — PR creation, merge strategy, and post-merge cleanup discipline.
- delivery/commits(softwaredev) — the commit quality the PR is built from.
- delivery/release(softwaredev) — the heavier sibling: publishing a versioned release.
- develop(meta) — the loop whose stable tail this is.
- code-reviewer (subagent, `agents/code-reviewer.md`) — the reviewer `merge` spawns at the gate; the built-in `/code-review` is the operator's manual equivalent.
