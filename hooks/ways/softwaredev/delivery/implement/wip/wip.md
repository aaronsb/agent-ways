---
description: how much work to have open at once and when a piece of work counts as finished, such as whether to start another branch or worktree while pull requests wait on review or CI; one issue per card, done when merged with its issue closed and its in-card review findings fixed; limits per stage from what each stage consumes; waves of ready cards with little file overlap, sweeps after the code stops moving, review and release one at a time
vocabulary: wip work in progress open cards in flight finish before starting next card parallel serial wave stage sweep reintegration overlap file set sub-issue blockedby backlog cycle time definition of done batch vertical slice waiting review ci spin up another worktree next issue pull requests piling up
scope: agent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Work in Progress

A card is one piece of work, tracked as one issue. Each open card holds context, files that other cards may need, and a place in the review queue. Starting every step across every card builds up work that all finishes late. Finishing one card at a time is slow where parts have to integrate. The balance is set per stage, by what that stage consumes (ADR-613).

## A card and when it is done

- A card is one issue in the project's tracker. Where issues are mirrored into the task list, it carries the mirror's label (delivery/issues).
- A card is done when three things hold: its change is merged to main, its issue is closed, and every review finding it owns is fixed or has left by the table in delivery/merge. Until then its issue, branch, worktree and pull request all count as open work.
- Slice a card through every stage it needs, from code to tests to docs. A card that does one stage across many areas is a sweep, covered below.

## What limits open work at each stage

| What the stage consumes | What goes wrong when too much is open | What to do |
|---|---|---|
| Files shared between cards | Rework when the branches are reintegrated | Run cards in parallel when their file sets barely overlap. A card whose files overlap another open card's waits, or the two become one card. |
| The operator's attention at review | Reviews get skimmed | Review one at a time any card that sets direction, changes a test baseline, or needs the human gate. |
| The window in which parts must integrate | Breakage turns up late | Where cards must fit together, merge to main early and keep branches short. Integrate continuously, not at the end. |
| CI turnaround | Hours lost per iteration | Evaluate on this host first and push in batches. |
| The lead session's attention | Losing track of what is in flight | Keep the agents in flight within the operator's standing limit (subagents/authorization). |
| Whether the area can be verified | Changes nobody can check | Hold them (delivery/groundwork/sequence). |

## Before starting another card

Check whether a card of yours is waiting in review or CI. If one is, finish it first: answer the review, fix the findings, watch the checks. Start the new card only when the wait is outside your control and the new card's files do not overlap the waiting one's.

## Plans are waves

A plan with more than one card is a parent issue with sub-issues. Dependency links between the sub-issues (GitHub's blocked-by) give the order that must be serial, and the issue mirror brings them into the task list.

1. **Find the ready cards:** sub-issues with no open blocker.
2. **Pick a wave:** the ready cards whose file sets barely overlap.
3. **Run the wave:** one worktree per card, and one agent per card where the work is delegated (subagents/agent-teams).
4. **Merge one at a time:** the lead merges each card and runs the gates after each merge, then picks the next wave.

A sweep (prose, strings, formatting, renames across many areas) starts only after the areas it covers have stopped changing, and runs one agent per area. A sweep that runs while code is still changing keeps bringing the same conflicts back. Stages bound by review, and the release, run one card at a time.

## The file set

When you plan a card, write the paths or areas you expect it to touch in its issue. At pull-request time, compare that list with the branch's diff. A card that touched much more than it declared has grown. Split the growth into its own card, or say in the pull request why it belongs.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "I'll start the next card while this one is in review" | Finish the review first, unless the wait is out of your hands and the files do not overlap. |
| "The finding is non-blocking, so I'll file it" | Non-blocking answers whether the change may merge. Whether an owned finding leaves the change comes from delivery/merge, and non-blocking is not one of its reasons. |
| "Running everything in parallel is faster" | Faster to start. Count the reintegration on overlapping files and the reviews it queues. |
| "The sweep can run alongside the code changes" | Each code change reopens the sweep's conflicts. Run it after the code stops moving. |
| "It's merged, so it's done" | Done also means the issue is closed and the fix-in-this-change findings are fixed. |

## See Also

- delivery/implement(softwaredev) — parent: planning one change, safe parallelization, the task shape
- delivery/merge(softwaredev) — review findings and their disposition, the merge gate
- delivery/issues(softwaredev) — issues as the task list, blockedBy, residuals
- delivery/groundwork/sequence(softwaredev) — holding a card while its area cannot be verified
- subagents/agent-teams(meta) — one worktree and one owner per file
- workflows/fan-out(meta) — parallel workers with a shared contract and one writer
