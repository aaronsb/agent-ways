---
description: debugging from evidence already on hand; read the logs, the database rows and the process still running before rerunning, patch the shared path every tenant or mode runs through when a fix to one did not stop it recurring, build a cheap test for only the failing phase of an expensive run, offer a reset to the last known-good commit, and add a regression check
vocabulary: leftover logs database state half-written still-running process before rerunning shared path choke point recurring keeps coming back every tenant mode patched again known-good commit reset rabbit hole expensive run cheap test one phase variable guard lint
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Debugging from the Evidence

When you report on a bug, say what is happening and why, how that differs from what was asked, and what the fix is.

## Read What Already Exists

Before rerunning anything, read the evidence that already exists: the logs, the database state, the process that is still running.

## Fix the Shared Cause

Trace the exact mechanism, then patch the shared pipeline, prompt, or choke point instead of the symptom. If a fix scoped to one mode or one tenant does not stop a recurring symptom, find the path every case shares. A note to remember the problem is not a fix.

## When Attempts Stop Producing Evidence

- List what is actually known. If workarounds pile up, go back to the original simple approach.
- Classify a repeated failure before retrying it (environment/recovery).
- Offer to find the last known-good commit in git history. Confirm it still has the features that must survive. Save the files the user names, such as local notes or instruction files, then reset and restore them.

## Isolate Cheaply

When a full run costs real money or a long wait, build a test for only the failing phase. Propose one concrete experiment per variable, such as the same prompt on a different input.

## Close the Loop

Add a regression check that reproduces the bug. Where one fits, add a guard or lint that would have blocked it. In incident notes, name the mechanism precisely, for example an ordering issue rather than a frequency issue.

## See Also

- environment/debugging(softwaredev) — parent: before changing code, what not to do, when stuck
- environment/recovery(softwaredev) — classify a failure before the next attempt, escalate after three
