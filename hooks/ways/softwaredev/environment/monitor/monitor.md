---
description: Watching long-running jobs, imports, builds, log tails, reply watches, live events, and autonomous loops while they run, and answering status checks such as where are we at or what is still pending. Attach a watcher to every job, keep a ledger, report progress with an ETA without being asked, pause with resumable state when the user steps away, remove watchers when done, and ship loops with a dry-run switch on.
vocabulary: long-running job import build going-for-a-while status still-pending step-away leaving watcher ledger pending progress eta check-in where-we-at how-we-doing periodically-update log-tail grep-errors phase reply-watch waiting-on alert-rules false-alarm live-event clock tick loop dryrun dry-run append-only active-hours stray-shells leftover resumable
pattern: while we wait|still running|shells? (still )?running|(^|\s)/loop\b|how'?s the .{0,30} going|where (are )?we at|how (are )?we doin|set (up )?a monitor|monitor (it|this|that|the)\b|periodically update|tail the logs|dry.?run (mode|flag|switch)
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Watching Running Work

## Watch every job

- Attach the Monitor tool, or a bounded watch loop, to every background shell and async wait. Results and errors then arrive as they happen.
- Keep a ledger labeled by batch. Close each job out with its result. Say which jobs are still pending and why.
- When the user asks "are you sure?", check the job directly. Do not rely on the completion notice.
- Report a job as running or done only on a liveness or completion signal (environment/bounded-execution).

## Leave nothing behind

- Before you report, check that no stray shells or loops are left.
- Remove each watcher once its event has happened.
- Stop a stalled job by its pid, as environment/bounded-execution describes.

## Logs

- Keep the log tail running in the background for the whole run.
- Check in briefly every few minutes. Grep for errors and phase changes.
- Hand heavy log reading to a subagent. Never read the full stream into context.
- Stop the tail when the run ends.

## Progress

- Report at intervals without being asked. "Periodically update me" holds for the whole run.
- Answer "where are we at?" with four items: the count done, what remains, an ETA, and a recommendation with its trade-off.
- If asked to parallelize, also offer other levers, such as cutting repetitions.
- If the user has to leave mid-run, finish the current step and pause. Save resumable state in the repo so the next session can continue.

## Reply watches

- After you post a question or hand work to another session, set a watcher and record what you are waiting on.
- Do not mark messages read while watching.
- Report only material changes. Write explicit alert rules so routine traffic does not raise false alarms.
- Say whether the watch survives compaction or a reboot.

## Live events

- Run a `/loop` and keep the clock in sync by counting forward from the time the prompt was sent.
- Between ticks, post observations and judgment calls.
- Flag anomalies. Do not act on participants unless asked.

## Autonomous loops

- Ship with a dry-run switch on, an append-only log, active-hours gating, and a strict scope.
- Defer consequential decisions to the user.
- Go live only after the user has reviewed the dry-run ticks and says to turn dry-run off.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "I'll check on it when the user asks." | Attach the watcher now. The error lands while nobody is looking. |
| "The loop is simple, it can go live." | Run it dry first. The user reviews the ticks before anything acts. |
| "The watcher is harmless, I'll leave it." | Remove it. Leftover watchers fire into later turns. |

## See Also

- environment/bounded-execution(softwaredev) — background forms, liveness signals, and killing by pid
- environment/attend(softwaredev) — peer messages and sensors under Monitor
- goals(meta) — a completion condition the session works toward
