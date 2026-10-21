---
description: Several agents or Claude sessions editing one git checkout at the same time, as a team or in parallel. Give each its own files, worktree and scratch folder; in a shared checkout, check branch and status before any git action, never switch branches or stash, stage files by name, and leave other sessions' changes alone.
vocabulary: agent team teammates parallel agents several sessions other sessions at once same repo same checkout git working tree worktree each agent own files ownership overwrote clobbered each other collide stash add all switch branch scratch folder idle notice relayed
pattern: \bagent teams?\b|\bshared (checkout|working (tree|directory))\b|\b(checkout|working tree) is shared\b|\b(other|several|multiple|parallel) (claude( code)?|cc) sessions\b
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Several Agents on One Repository

## Design the team

- Give each agent its own files or packages. No two agents edit the same file; the agent that does not own a file sends its change to the owner. File ownership in a plan is set in delivery/implement.
- Use a team whose members message each other only when they must coordinate. Otherwise use isolated subagents (subagents).

## One worktree per agent

- Give each agent its own git worktree or working copy.
- Give each agent a scratch folder named for it, such as `<scratch>/<agent-name>/`. Two agents that write a helper at the same shared path overwrite each other.
- Copy back only the files the agent owns.
- Each agent stays inside its assigned scope.

## Shared checkouts

Assume another session may be live in the same checkout. A branch switch there moves every session in it, and a sweeping command picks up their uncommitted work.

| Do | Don't |
|----|-------|
| Run `git branch --show-current` and `git status` before any git action | Switch branches in the shared checkout |
| Do parallel task work in a worktree | Run task work in the shared checkout |
| Stage files by name | Run `git add -A`, `git add .` or a bare `git stash -u`, which sweep up other sessions' work |
| Leave files you did not change as you found them | Restore, reset or clean another session's changes |
| Check for other sessions' new commits before a merge | Merge without looking |

## Teammate traffic

- Answer an idle or duplicate notice in one line: say it was already relayed and name what is still running.
- Nudge an agent that went idle without delivering its report. Release agents once their reports are in.
- Draw no conclusion until every track has reported, and say which reports are outstanding.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "Only my session uses this checkout" | Check `git status` first. Another session may have left work there. |
| "Both agents only touch a little of that file" | Give the file one owner. |
| "Three of four tracks agree, so I can conclude" | Wait for the fourth. It may be the one that disagrees. |

## See Also

- subagents(meta) — parent: briefs, the return shape and worktree base branches
- subagents/briefs(meta) — what each agent's prompt carries
- teams(collaboration) — the teammate side of this traffic
- delivery/branching(softwaredev) — branch awareness before edits
