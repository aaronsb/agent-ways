---
name: gh-monitor
description: Watch GitHub from a running session by launching a Monitor instead of polling by hand. `ci` watches the current PR's checks until they resolve; `inbox` watches your GitHub notifications while you work. Use when the user says "watch CI", "tell me when checks finish", "watch my PR", "watch my notifications/inbox", or invokes /gh-monitor.
allowed-tools: Bash, Monitor, Read
argument-hint: [ci | inbox]
---

# gh-monitor — watch GitHub without stopping to check

The wrong way to learn that CI went red, or that someone requested your review,
is to stop work every few minutes and run `gh pr checks` / `gh api notifications`
by hand. This skill launches a **Monitor** that does the polling in the
background and drops a notification into the chat only when something actually
happens.

**Why a Monitor and not an attend sensor.** External, session-specific watching
(one PR's CI, *your* inbox) is not ambient local-world observation — it is
bounded to the task in front of you, and it belongs in a Monitor the agent
launches when the work is relevant, not in attend's uniform per-tick sensor loop
(ADR-137; the GitHub sensors were retired for exactly this reason). Monitor
natively does "poll endpoint → emit line → notify"; that is the whole job.

## Pre-flight

Both modes need an authenticated `gh`. Check once before launching:

```bash
gh auth status
```

If that fails, tell the user to run `gh auth login` and stop — Claude inherits
the user's GitHub account, so there is nothing else to configure.

## Mode `ci` — watch this PR's checks until they resolve

**Lifecycle: until-terminal, NOT persistent.** It watches *this push's* CI and
exits when every check has resolved. That bound is the point — there is nothing
to leave running afterward.

Launch with the **Monitor tool** (not Bash):

- **command**: `bash ~/.claude/scripts/gh-monitor-ci-watch.sh` — append a PR
  number/branch/URL only if watching something other than the current branch,
  e.g. `bash ~/.claude/scripts/gh-monitor-ci-watch.sh 142`
- **description**: `gh-monitor ci: <repo>#<pr>` (fill in the PR you're watching)
- **persistent**: `false`
- **timeout_ms**: `1800000` (30 min — a generous cap so a wedged/never-running
  required check can't watch forever; raise it for known-slow pipelines)

The script announces each check the first time it lands (`✓ name — passed`,
`✗ name — FAILED`), then one aggregate line (`CI PASSED (N checks)` /
`CI FAILED (names)`), then exits. If the branch has no open PR or the PR has no
checks, it says so once and exits cleanly.

## Mode `inbox` — watch your notifications while you work

**Lifecycle: persistent** (session-length). Stop it with **TaskStop** when the
work it was supporting is done.

Launch with the **Monitor tool**:

- **command**: `bash ~/.claude/scripts/gh-monitor-inbox-watch.sh`
- **description**: `gh-monitor: GitHub notifications`
- **persistent**: `true`
- **timeout_ms**: `3600000` (ignored when persistent, but pass it anyway)

It starts from "now" (no backlog dump) and emits one line per new, relevant
notification — `[review_requested] PullRequest in org/repo: title`. It tiers on
GitHub's `reason` and silently drops `subscribed` / `ci_activity` chatter (CI is
the `ci` mode's job).

## Correct-invocation rules (already baked into the scripts — preserve them)

If you ever inline a variant instead of calling these scripts, keep these — they
are the difference between a useful watch and a silent or runaway one:

- **Use the Monitor tool, never Bash.** Bash blocks and discards the stream;
  only Monitor delivers stdout lines as async notifications.
- **Cover every terminal state, not just success.** A watch that greps only for
  the happy path is silent through a failure, and silence reads as "still
  running." `ci` emits on pass *and* fail/cancel.
- **Flush every pipe stage** (`grep --line-buffered`, `awk 'fflush()'`); never
  `| head` — it buffers until N matches accumulate, then ends the stream.
- **Remote poll intervals ≥ 30s** (rate limits); honor `X-Poll-Interval` when a
  feed advertises it (the inbox does: 60s).
- **Guard transient failures** (`gh ... || true`) so one bad poll doesn't kill a
  long watch; advance any `since=` marker only after a successful poll.

## Stopping

`ci` exits on its own. For `inbox`, stop with **TaskStop** on its Monitor task
id.

## Arguments

- `/gh-monitor ci [pr]` — watch a PR's checks until terminal (default: current branch)
- `/gh-monitor inbox` — watch your GitHub notifications until stopped
