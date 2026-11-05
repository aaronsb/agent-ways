---
contract: adr/v1
kind: decision
verb: change
capability: loop
basis:
  - operator: aaronsb
    level: guided
    said: "I think the WIP thing is a pretty significant gap. it likely needs to integrate with github issues, sequencing of work, worktrees, agents etc. let's discuss what that might look like"
    via: Claude Code session 2026-11-05, after a peer session asked whether the ways cover limiting work in progress
  - operator: aaronsb
    level: guided
    said: "when I read this I understand it as 'I made the effort to find the problem I traced the root cause, and I have a fix, but let's...just leave it for later' -> this is the sort of work that if we performed the remediation makes better code, and we've already done most of the hard part. while it extends WIP it is avoiding technical debt. there's an interesting line between 'this works pretty good, except for this one glaring memory eating problem' which needs a judgement, in my opinion, that's a fix now while we have context sort of thing"
    via: Claude Code session 2026-11-05, about a review finding a subagent marked non-blocking
  - operator: aaronsb
    level: guided
    said: "let's do this. I think these are excellent refinements to … targets in our ways"
    via: Claude Code session 2026-11-05, approving the direction and the five leans this record decides
  - evidence: "No way covers work in progress. On 2026-11-05 `ways_search` for 'limit work in progress, finish one item before starting the next, balance parallel and serial work' and for 'integration cost of parallel branches, reintegration, small batches, vertical slice' ranked nothing above cosine 0.43; the nearest were delivery/implement (its parallel-or-serial collision table), delivery/groundwork/sequence, workflows and workflows/fan-out"
  - evidence: "agents/code-reviewer.md sorts findings on one axis, 'Block merge when' (security, broken tests, unmet acceptance criteria) and 'Warn but allow when' (style, improvement opportunities, debt recorded in an ADR). It answers whether a change may merge and never whether a finding is fixed in the change, so a finding short of blocking reads as later. delivery/merge already says 'Do not merge with known findings open'"
  - evidence: "A review of a vtium change marked as a non-blocking follow-up a finding the change introduced (threads left at a config reload stay joinable, so glibc keeps each 8 MiB stack mapped until exit, where the dropped handle had detached them before), with the cause traced, a README sentence the change made false, and a two-line fix named inside the change's own files"
  - evidence: "vtium's alpha.6 plan, relayed by its session on 2026-11-05, staged cards by file overlap: a parallel first stage of cards on separate files, each done only when on main with its issues closed; a single card that moves the goldens, reviewed alone; a wide, short sweep of prose and strings run only after the code stopped moving, because a sweep during code changes kept bringing conflicts back; and a serial release stage"
  - precedent: ADR-180
  - precedent: ADR-612
  - precedent: ADR-611
agent:
  name: Claude
  model: claude-opus-5-5
status: proposed
date: 2026-11-05
deciders:
  - aaronsb
related:
  - ADR-138
  - ADR-175
  - ADR-181
observable:
  - "run: ways match 'we have six cards open and two PRs waiting on review, should I start the next one or finish these' ranks the work-in-progress way as firing"
  - "see: a code-reviewer report gives each finding a disposition, fix in this card or file with a named reason, and a finding with a traced cause and a fix inside the change's files is marked fix in this card"
  - "see: /merge applies every fix-in-this-card finding before the merge, and files only findings whose recorded reason is one of the filing reasons"
  - "see: starting a branch or worktree, or assigning yourself an issue, shows the count of open cards (in-progress issues, open pull requests, unmerged branches and worktrees), and says so when a card of yours is waiting in review or CI"
  - "run: a planning step over a parent issue lists the ready sub-issues and the file overlap between each pair, and proposes a wave"
---

# ADR-613: Work in progress is held per stage by what the stage consumes, and a card is done when it is on main with its in-card findings fixed

## Summary

- **Decided:** A unit of work (a card) is one issue. It is done when it is merged to main, its issue is closed, and every finding fixed-in-the-card has been fixed; until then it counts as open work. How many cards are open at each stage is judged by what that stage consumes: overlapping files at build, the operator's attention at a direction-setting review, the integration window where parts must meet, CI turnaround, and the lead's attention. A plan is a parent issue with sub-issues; `blockedBy` gives the serial order, and a wave is the ready cards that share few files. Review findings get a disposition by the cost of fixing now against later, not a blocking tier: a finding the change introduced, that makes its own text false, or whose traced fix stays inside the card's files is fixed in the card. The open count is measured from GitHub and git and reported when work starts, across sessions; any guard ships off.
- **Trades away:** A card's cycle time grows when its review findings are fixed inside it, and some findings a reviewer would have deferred now hold a merge. The limits stay judgment with a measured count, so nothing stops a session that ignores the count.
- **One-way?** No. It changes way text, the reviewer agent, the merge skill, and adds a macro and a `gh-tasks` subcommand; each comes out with a revert.
- **Probes:** *Confident:* a finding with its cause traced and its fix named inside the change's own files is fixed before the merge, whatever severity the reviewer gave it. *Not confident:* that limits stay judgment plus a measured count for now, with no numbers in `ways.yaml` until the counts show what the real limits are.
- **Inversion:** All serial (one card at a time from start to merge) against all parallel (every step across every card at once). The answer is per stage: parallel where files barely overlap and nothing must integrate, one at a time where a person reviews or everything is touched, and integration kept continuous where parts must meet.

## Context

The ways cover planning one change (delivery/implement), ordering a queue by readiness (delivery/groundwork/sequence), finding where one change waits (groundwork/stall), and running parallel agents (workflows/fan-out, subagents/agent-teams). The issue mirror (ADR-180) brings tasklist issues and their `blockedBy` edges into the session's task list. Nothing says how much work should be open at once, which cards run side by side, or when a card counts as finished.

Two failures follow from that gap. Doing each step across the whole stack builds up open work that finishes late, while finishing one card at a time is slow where parts must integrate; nothing guides the balance by stage. And a review that finds a defect, traces its cause and names its fix can still mark it non-blocking, because the reviewer's only axis is whether the change may merge. Filing such a finding does not reduce open work. It turns work whose context is loaded into a new card whose cause must be found again.

Two cases surfaced the need. A release plan staged its cards by file overlap: separate files ran in parallel, a card that moved the test baselines ran alone for review, and a prose sweep waited until the code stopped moving, because running it during code changes kept bringing conflicts back. A review elsewhere deferred a memory regression the change itself introduced, with the cause traced and a two-line fix named.

## Decision

### 1. A card, and when it is done

A card is one issue carrying the `tasklist` label (ADR-180). A card is done when its change is merged to main, its issue is closed, and every finding with the fix-in-this-card disposition (item 5) is fixed. Until all three hold, the card is open: its issue, branch, worktree and pull request all count toward work in progress. A card is sliced through every stage it needs, from code to tests to docs, by default; a card that does one stage across many areas is a sweep (item 4).

### 2. What limits open work, by stage

The number of cards open at a stage is set by what that stage consumes:

| What it consumes | Cost of too many open | Guidance |
|---|---|---|
| Files shared between cards | Rework at reintegration | Run cards in parallel when their file sets barely overlap. A card whose files overlap another open card's waits, or the two merge into one card. |
| The operator's attention at review | Reviews that are skimmed | A card that sets direction, changes a baseline, or needs the human gate (delivery/merge) is reviewed one at a time. |
| The window in which parts must integrate | Breakage found late | Where cards must meet, merge to main early and keep branches short. Integration runs continuously, not at the end. |
| CI turnaround | Hours lost per iteration | Evaluate locally first and push in batches. A release is not a way to try a fix. |
| The lead session's attention | Losing track of what is in flight | The agents in flight stay within the operator's standing limit (subagents/authorization). |
| Whether the area can be verified | Changes nobody can check | delivery/groundwork/sequence holds them. |

Starting a new card while one of yours waits in review or CI is the moment to check this table. Finishing the waiting card comes first, unless the wait is outside your control and the new card's files do not overlap.

### 3. Plans are waves over issues

A plan with more than one card is a parent issue with sub-issues. GitHub's `blockedBy` edges give the serial order, and ADR-180 already brings them into the task list. A wave is the set of ready cards (no open blocker) whose file sets barely overlap. Each card in a wave gets its own worktree and, where delegated, its own agent; the lead merges them one at a time and runs the gates after each merge.

A sweep (prose, strings, formatting, renames across many areas) runs only after the areas it covers have stopped changing, and fans out one agent per area. Review-bound and release stages run serially.

A card declares its file set, the paths or areas it expects to touch, in its issue when it is planned. At pull-request time the declared set is compared with the branch's diff; a card that touched much more than it declared has grown, and the growth is either split into its own card or recorded in the pull request.

### 4. Findings get a disposition, not a tier

Every review finding is given one of two dispositions, by the cost of fixing it now against later:

| Fix in this card when any of these holds | File it, with its reopen trigger, when |
|---|---|
| The change introduced it: a regression against main | The fix needs a call above the waterline (ADR-612): a dependency, a format, a default users will feel |
| It makes a statement in the change false: a doc, a comment, a help string | The fix lands in another open card's files |
| Its cause is traced and the named fix stays inside the card's files | The fix needs verification the card cannot run |
| Users would feel it, at any fix cost | The fix is large and its impact small |

A finding users would feel holds the card open whatever the fix costs. A finding filed carries the reason from the right-hand column and the condition that reopens it (delivery/issues, residuals). The merge verdict (may this merge) stays, and is a separate question from the disposition: a change can be safe to merge and still not done.

### 5. Counting open work

A macro counts open cards from GitHub and git: issues in progress, open pull requests, unmerged branches, and worktrees. It reports the count when work starts (a new branch, a new worktree, assigning yourself an issue), names cards of yours waiting in review or CI, and counts other sessions' cards through issue assignees, since GitHub is shared. It warns; it does not refuse. A guard that refuses a new branch or worktree over a limit may be added later, shipped deactivated like the bounded-execution guard (ADR-181).

The limits stay judgment with this measured count. Numbers in `ways.yaml` (for example a review limit of one) wait until the counts show what the real limits are.

### 6. Where it lands

| Target | Change |
|---|---|
| A new way, `softwaredev/delivery/implement/wip` | Items 1 to 3: the card, the stage table, waves, sweeps, file sets |
| `agents/code-reviewer.md` | Findings carry a disposition from item 4; a finding with a traced cause and a fix inside the change's files defaults to fix in this card; the merge verdict stays separate |
| `delivery/merge` and the `/merge` skill | The remediation step applies every fix-in-this-card finding before the merge and files only with a right-hand reason recorded; the definition of done from item 1 |
| `delivery/implement` | Points at the new way for more than one card |
| A macro on the new way | Item 5's count and warnings |
| `gh-tasks ready` | Lists ready sub-issues of a parent with the file overlap between each pair, from the declared file sets |
| The `/develop` skill | A planning step that lays waves when the work is more than one card |

The first increment lands the disposition rule (reviewer, merge way and skill) and the way. The count macro and `gh-tasks ready` follow as their own increments.

## Consequences

### Positive

- A defect a change introduced, with its fix already found, is fixed while its context is loaded, instead of becoming a card that must find the cause again.
- Done means one thing across sessions, agents and the merge skill.
- Parallel work is chosen by file overlap and integration need, so reintegration cost is weighed before cards start instead of discovered at merge.
- Open work becomes visible as a number, across sessions in the same repository.

### Negative

- Cards take longer to close when their findings are fixed in them, and a reviewer's findings can hold a merge that would have gone through before.
- Declaring a file set adds a step to planning, and the declared set is a prediction that will often be wrong at the edges.
- The count depends on the `tasklist` label and on assignees being kept; a repository that uses neither gets a partial count.

### Neutral

- The reviewer's block and warn wording moves from a tier on findings to a verdict on the merge.
- GitHub sub-issues become part of planning; a repository without them can keep the plan in the parent issue's body.

## Alternatives Considered

- **Fixed numeric limits in `ways.yaml` from the start.** Rejected for now: no measurement says what the limits are, and a number set by guess trains sessions to work around it. The count in item 5 gathers that measurement first.
- **A guard that refuses a new branch or worktree over the limit.** Deferred: a refusal on a guessed limit blocks legitimate work, such as an urgent fix while cards wait in CI. It may ship later, deactivated by default.
- **The plan as a file in the repository, as SPE plans are.** Rejected: issues are already the shared truth (ADR-180), other sessions read them without the checkout, and `blockedBy` already reaches the task list.
- **Keeping block and warn tiers and adding "fix it if cheap".** Rejected: "cheap" has no threshold, and the tier still answers the wrong question. The disposition rows name checkable conditions: introduced by the change, makes its text false, traced fix inside the card's files.
- **A board tool, such as GitHub Projects columns, as the source of the count.** Rejected for the first increment: it needs project setup in every repository, where issues, pull requests, branches and worktrees already exist everywhere. A project board can read the same states later.
