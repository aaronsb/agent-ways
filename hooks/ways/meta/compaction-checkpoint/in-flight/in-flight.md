---
description: At a compaction or handoff, the background jobs and dev servers still running and whether each survives, and the uncommitted work committed on the working branch, split by purpose
vocabulary: background jobs still running watcher dev server survive compaction lost at compaction stray shells uncommitted edits commit split
trigger: context-threshold
threshold: 85
scope: agent
refire: 0.15
---
<!-- epistemic: convention -->
# What Is Still Running or Uncommitted

The checkpoint's summary covers the work. Add what is still in flight around it.

## Running processes

List the background jobs, monitors, and dev servers still running, and whether each survives compaction or a reboot. Stop stray shells and stalled loops now.

## Uncommitted work

Commit the uncommitted work on the working branch, split by purpose, and name any file you hold back on purpose.

## On an unattended run

Ask nothing. Write the state, label each assumption "override if wrong", and keep working. The assumptions are the decisions choices(meta) would have put to the user.

## See Also

- compaction-checkpoint(meta) — parent: the summary, the user's steering, and the continuance prompt
- wrap(meta) — the same state gathered when you pick the seam
- choices(meta) — the decisions an unattended run labels instead of asking
