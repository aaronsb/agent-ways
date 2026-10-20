---
description: debugging, troubleshooting failures, investigating broken behavior, finding what changed and fixing the shared root cause, resetting to the last known-good commit when stuck, isolating an expensive failure cheaply, and working a list of UAT or review findings one at a time
vocabulary: debug debugging troubleshoot troubleshooting stack trace stacktrace backtrace add logging tracing root cause reproduce reproduction bisect regression crash crashes segfault exception panic core dump breakpoint bug guessing speculative typos simultaneous what changed still broken still not working rabbit hole last known good commit reset shared cause choke point isolate failing phase live run findings uat feedback remediation plan regression check guard
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Debugging Way

## Answer in Three Parts

When you report on a bug, say what is happening and why, how that differs from what was asked, and what the fix is. Accuracy comes before speed.

## Before Changing Any Code

1. Read the full error message and stack trace
2. Read the evidence that already exists before rerunning anything: the logs, the database state, the process that is still running
3. Search the codebase for the error string
4. Check recent changes: `git log --oneline -10` and `git diff HEAD~3`
5. Reproduce the issue — if you can't trigger it, you can't verify a fix

## Fix the Shared Cause

Trace the exact mechanism, then patch the shared pipeline, prompt, or choke point instead of the symptom. If a fix scoped to one mode or one tenant does not stop a recurring symptom, find the path every case shares. A note to remember the problem is not a fix.

## Do Not

- Change code based on guessing — verify the root cause first
- Fix multiple things at once — one change, one test
- Assume the bug is where the error appears — trace back to the source

## When Stuck

- `git bisect start` / `git bisect bad` / `git bisect good <ref>` to find the introducing commit
- Add targeted logging at function boundaries, not scattered everywhere
- Check the obvious: typos, wrong file, stale cache, wrong branch, missing env var
- When attempts stop producing new evidence, list what is actually known. Research workarounds past the blocker instead of stopping. If workarounds pile up, go back to the original simple approach and unblock it.
- Classify a repeated failure before retrying it (environment/recovery).

### Reset to the last known-good commit

For a rabbit hole, offer to find the last known-good commit in git history. Confirm it still has the features that must survive. Save the files the user names, such as local notes or instruction files, then reset and restore them.

## Isolate Cheaply

When a full run costs real money or a long wait, build a test for only the failing phase. Propose one concrete experiment per variable, such as the same prompt on a different input.

## Live Runs and Intermittent Issues

- During a live run, list the bugs instead of fixing each one. After the run, write one holistic plan, implement it, deploy, tail the logs, and retest.
- "Edit but do not deploy yet" means local edits only.
- Log an intermittent issue seen once and act only if it recurs. Do not guess a fix without reproducible evidence.
- When several files fail, get one passing the user's check before moving to the next.

## Findings Loop

Turn raw client, UAT, or review feedback into a numbered findings file. For each finding, read it, reason about it, research it, and propose a remediation plan. Implement after approval, wait for the user's test, then update the file. "What's next?" means recommend the most important remaining finding. Record reordering and out-of-scope calls in the file.

## Close the Loop

Add a regression check that reproduces the bug. Where one fits, add a guard or lint that would have blocked it. In incident notes, name the mechanism precisely, for example an ordering issue rather than a frequency issue.

## See Also

- environment/recovery(softwaredev) — classify a failure before the next attempt, escalate after three
- code/testing/evaluation(softwaredev) — reproduce as reported and count the runs a fix rests on
- code/quality/integration(softwaredev) — sweep sibling copies of the same defect
- incident(itops) — production incidents and what has to exist before one closes
