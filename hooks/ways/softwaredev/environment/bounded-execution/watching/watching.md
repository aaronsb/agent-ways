---
description: watching background work while it runs; attach a watcher to each background job, keep a list of jobs in flight and close each one out, grep a running log for errors and phase changes, report progress with what is done, what remains and an estimate when asked to keep the user updated, and remove spent watchers before reporting
vocabulary: keep an eye on it takes hours keep me posted progress eta backfill reindex import jobs in flight phase changes time left leftover stray background shell
scope: agent, subagent
refire: 0.2
---
<!-- epistemic: heuristic -->
# Watching What You Started

- Attach the Monitor tool, or a bounded poll, to each background job when you start it, so a failure arrives when it happens and not when someone asks.
- Keep a list of the jobs in flight. Close each one out with its result, and say which are still pending and why.
- Check a running job's log by grepping it for errors and phase changes. Do not read the full stream into context. Hand heavy log reading to a subagent. Where your instructions restrict the Agent tool, do that when the user, a skill in use or a CLAUDE.md line asks for it, and otherwise propose it.
- When the user asks to be kept updated, report at intervals without being asked again: what is done, what remains, and an estimate of the time left.
- Before you report, remove each watcher whose event has happened and stop any tail or loop you no longer need. A leftover watcher fires into later turns.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "I'll check on it when the user asks" | Attach the watcher now. The failure lands while nobody is looking. |

## See Also

- environment/bounded-execution(softwaredev) — parent: background forms, liveness signals, killing by pid
- environment/attend(softwaredev) — peer messages and sensors under Monitor
