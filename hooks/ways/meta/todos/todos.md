---
trigger: context-threshold
threshold: 75
repeat: true
macro: prepend
scope: agent, subagent
requires: ["Bash(jq:*)", "Bash(ways:*)"]
refire: 0.15
---
<!-- epistemic: heuristic -->
# Task List Checkpoint

You have no active task list and context is filling up. But first — is there actually unfinished work? If the current task is nearly done or the session is wrapping up naturally, you don't need a task list just because context is high. The point of a task list is to survive compaction with enough detail to resume, not to document completed work.

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

Mark the in-flight task as `in_progress`. This creates the tasks-active marker and stops this checkpoint from repeating.

This repeats every prompt until you create a task list or the session ends.
