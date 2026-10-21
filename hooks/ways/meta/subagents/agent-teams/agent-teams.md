---
description: Leading an agent team or running several Claude sessions at once. Split roles by file ownership, give each agent its own worktree and named scratch folder, check branch and status before any git action in a checkout other sessions share, and answer teammate notices in one line.
vocabulary: agent-team lead roles owns-files file-ownership tester fixer orchestrator scratch-folder shared-checkout parallel-sessions concurrent-sessions other-sessions isolation idle-notice duplicate-notice relayed copy-back branch-switch stage-by-name
pattern: \bagent teams?\b|tester.{0,60}fixer|shared (checkout|working directory)|(checkout|working directory) is shared|\b(other|several|multiple|parallel|two|three) (claude( code)?|cc) sessions\b
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Leading an Agent Team

## Design the team

- Split roles by file or package ownership. No two agents edit the same file.
- Use an agent team, whose members can message each other, only when they must coordinate. Otherwise use isolated subagents.
- For iterative quality work, use three roles: a tester, a fixer, and an orchestrator.
- Use agents to protect the main context from large inputs, such as a long error log or a token-heavy read. Synthesize their reports yourself.
- Keep the main thread free to answer the user while agents run. Report progress when asked.

## Isolate each agent

- Give each agent its own git worktree or working copy.
- Give each agent a scratch folder named for it: `<scratch>/<agent-name>/`. Never write helpers at shared names such as `<scratch>/run.sh`. Two agents writing the same helper overwrite each other.
- Copy back only the files the agent owns.
- One invocation does one task. Do not chain two goals in one brief.
- Each agent stays inside its assigned scope. An agent that deploys a service does not edit the docs.

## Shared checkouts

Assume other Claude sessions may be live in the same checkout. A branch switch there moves every one of them.

| Do | Don't |
|----|-------|
| Run `git branch --show-current` and `git status` before any git action | Switch branches in the shared checkout |
| Do parallel task work in a worktree | Run task work in the shared checkout |
| Stage files by name when the user asks for a commit | Use `git add -A` or a bare `git stash -u`, which sweep up other sessions' work |
| Check for other sessions' new commits before merging | Merge without looking |
| Run `git log -1` in the directory you deploy from | Deploy from a checkout that lags a worktree deploy |

Commits and merges the user asks for still go ahead after these checks. Never commit in a shared checkout on your own initiative.

## Teammate traffic

- Answer an idle or duplicate notice in one line. Say it was already relayed and name what is still running: "Same report, already relayed. The docs build is the only thing running now."
- A late notice for work already integrated gets one line and no other action.
- Nudge an agent that went idle without delivering its report.
- Release agents once their reports are in.
- Draw no conclusions until every track has reported. Say what you are waiting on: "Waiting on the last two reports."

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "Only my session uses this checkout." | Check `git status` first. Another session may have left work there. |
| "Both agents only touch a little of that file." | Give the file one owner. The other agent sends its change to the owner. |
| "Three of four tracks agree, so I can conclude." | Wait for the fourth. It may be the one that disagrees. |

## See Also

- subagents(meta) — briefs, the handback shape, and worktree base branches
- teams(collaboration) — the teammate side of this traffic
- workflows(meta) — when a scripted workflow beats a hand-run team
- delivery/branching(softwaredev) — branch awareness before edits
