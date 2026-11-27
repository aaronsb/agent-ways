---
description: The lead session's role when it runs workers in parallel — watching PRs, CI, running agents, the task list and sensor reports at low resolution, deciding where a worker goes next, briefing it and merging, without reading diffs, re-running gates or redoing a worker's work to check its claims
vocabulary: lead orchestrator coordinator main session ambient attention watch streams low resolution workers parallel branches decide next brief merge check claim falsify redo duplicate work re-run gates inverted roles overseer dispatch steer integrate wave candidate
scope: agent
refire: 0.15
---
<!-- epistemic: heuristic -->
# The Lead

The lead is the session that starts workers, watches them and merges their work. A worker is a subagent, teammate or other session that holds focal attention: one worktree, one change, the gates that change reaches, and the findings it owns. The lead holds ambient attention: it watches every stream at low resolution and decides where focal attention goes next.

## What the lead watches

- Open PRs and the state of their CI checks.
- Running agents, and the reports they return.
- The task list.
- Sensor reports and peer messages, which attend delivers to the session (environment/attend).

From these, the lead decides which change needs a worker next, briefs that worker in one complete brief (subagents/briefs), and merges the work when it is ready (delivery/merge). It also decides review depth. A deep review goes to a change that sets direction or has a high blast radius, and the breadth of review comes once, at integration. Which unfinished checks hold a merge is in delivery/ci.

## What the lead does not do

- It does not read a worker's diff. Reading a diff is focal work, and a reviewer or the worker does it.
- It does not re-run a worker's gates. A tree that passed is not tested again (delivery/ci).
- It does not check a worker's claim by redoing the work.

## Inverted roles

Roles are inverted when the lead does focal work, such as reading a diff or re-running a suite, or when a worker does ambient work, such as running the full gate battery after each fix round. An inversion tests the same tree twice, and while the lead reads one change its view of the other streams goes stale. When the lead finds itself doing a worker's job, it briefs a worker instead. Which gates a change reaches, and when the full pass runs, is set in code/testing/gates/attention.

## Watch for

| You notice | It means | Do this |
|---|---|---|
| You have opened a worker's diff, or started its tests, to check its report | You are redoing the worker's focal work | Name what would show the claim false and look at that one thing. For a green-CI claim, read the checks' result. For a claim that the worker stayed in its files, read `git diff --stat <base>`. |
| The brief you are writing lists every row of the gates table | The brief asks the worker for the ambient pass | Name the gates the change reaches, and say that the full suite runs at integration (subagents/briefs). |
| You are about to send a running worker its second or third correction | Each message reaches the worker at the end of a turn, after it has built on the brief it corrects | Stop the worker and restart it with one complete brief (subagents/briefs). |
| Two workers' reports each describe a scratch helper that does the same job | The briefs left a shared need unstated, so each worker wrote it | Have one worker write the helper, or write it yourself, and name its path in the other briefs. |

The signals about CI runs and shared build directories are in delivery/ci. The signals about choosing focal or ambient checks and review are in code/testing/gates/attention.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "I'll re-run the suite before I merge" | The tree already has its run. Re-run only when the tree changed. |
| "The worker should run everything after each fix" | A fix round gets the gates the fix reaches. The full pass runs once at integration. |
| "Every branch gets a review swarm" | Deep review goes to direction-setting or high-blast-radius changes. Breadth comes at integration. |

## See Also

- subagents(meta) — parent: the return shape a worker hands back
- subagents/briefs(meta) — what the lead puts in a worker's brief, and how to correct a running worker
- subagents/agent-teams(meta) — several agents on one repository
- code/testing/gates/attention(softwaredev) — focal and ambient verification, and which gates a change reaches
- delivery/ci(softwaredev) — one run per tree, and checks that are only waiting
- delivery/merge(softwaredev) — the review gate and the merge
- environment/attend(softwaredev) — the sensors and peer messages that make up the lead's ambient view
