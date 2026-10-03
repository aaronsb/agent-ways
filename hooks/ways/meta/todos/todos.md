---
trigger: context-threshold
threshold: 75
macro: prepend
scope: agent, subagent
requires: ["Bash(jq:*)", "Bash(ways:*)"]
refire: 0.15
---
<!-- epistemic: heuristic -->
# Task List Checkpoint

Context is filling up. If the task list already holds the current work, keep it current and move on. If not, first ask whether there is actually unfinished work. If the current task is nearly done or the session is wrapping up naturally, you don't need a task list just because context is high. The point of a task list is to survive compaction with enough detail to resume, not to document completed work.

If there *is* work in progress that would be lost to compaction, capture it now.

## What to Capture

Compile the current state into tasks using `TaskCreate`. You hold the session history, so only you can do this accurately.

For each task, capture:
- **subject**: What needs to be done (imperative form)
- **description**: Enough detail that a post-compaction agent (or subagent) can pick it up cold — file paths, decisions made, what's been tried, what's left
- **activeForm**: Present continuous for the spinner

**Include at minimum:**
- The current goal and what prompted it
- Progress so far (what's done, what's in flight)
- Next steps with enough specifics to resume without the conversation history
- Key decisions already made (so they don't get re-debated)

Mark the in-flight task as `in_progress`.

This checkpoint returns as the session keeps growing, on its refire cadence, whether or not a task list exists.

## See Also

- wrap(meta) — wrapping a session deliberately runs this same TaskList-honesty pass, then writes a continuation prompt and hands off to a directed compaction. The `/wrap` skill is where the on-demand version lives.
- compaction-checkpoint(meta) — the broader near-limit checkpoint this task discipline feeds.
