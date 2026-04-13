#!/usr/bin/env bash
#
# gh-pr-checks.sh — example attend external sensor
#
# Polls `gh pr checks` for the PR attached to the current branch and
# emits a notification only when the aggregate CI state crosses into a
# terminal bucket (all passing / any failing). Quiet on first run, quiet
# on pending-state churn, quiet when there's no PR or the branch is
# main/master. In practice this means: you push, your CI goes green or
# red, you get one notification, you move on.
#
# Why this exists. The built-in sensors don't observe GitHub — nothing
# inside attend knows your PR just went red. Without this sensor the
# only way to learn is to stop and run `gh pr checks` yourself every
# few minutes. That's the exact thing attend is supposed to replace.
#
# Design choices worth the ink:
#
#   1. **Aggregate, not per-check.** `gh pr checks` emits one line per
#      check run. Per-check notifications would be chatty beyond use —
#      a big matrix can generate a dozen events for one push. The
#      sensor rolls everything into three states (pass, fail, pending)
#      and emits only on terminal transitions. Users who want the
#      check-by-check view run `gh pr checks` on demand.
#
#   2. **Silent on pending entry.** A new push restarts checks, which
#      means every push triggers `pass → pending` or `fail → pending`.
#      Emitting on those would produce "CI is running…" spam; the user
#      knows they pushed. Only transitions *out* of pending matter.
#
#   3. **Loudest event is a regression.** `pass → fail` is magnitude
#      4.5 because it's unusual (you thought it was green and it
#      broke), breaks refractory, and deserves attention now. A fresh
#      `pending → fail` is 4.0: still loud, still breaks refractory,
#      but it's the normal "you pushed broken code" feedback loop.
#
#   4. **State keyed by repo + branch + PR number.** The marker
#      filename sanitizes the repo root, the branch name, and the
#      current PR's number into a filename-safe key. Including the
#      PR number matters: if you close a PR on a branch and open a
#      new one on the same branch, the old marker's terminal state
#      shouldn't tag the new PR's first transition as "recovered"
#      (or worse, "regressed"). Sanitization reduces collisions
#      across unrelated checkouts — it doesn't guarantee uniqueness
#      (two paths that differ only in a non-alnum separator can
#      still alias), but collisions are rare enough in practice
#      that a hash would be over-engineering.
#
# Prerequisites: `git`, `gh` (authenticated), `jq`. The script exits
# silently if any of these are missing — attend treats a silent exit
# as a quiet poll, not an error.
#
# Author's lever: magnitude. Everything else is attend's job.
#
# --- Config recipe ------------------------------------------------------
#
# Add to ~/.config/attend/config.yaml (or <project>/.claude/attend.yaml):
#
#   sensors:
#     +gh-pr-checks:
#       script: ~/.claude/tools/attend/examples/gh-pr-checks.sh
#       enabled: true
#       interval: 120       # 2 min at rest — CI is minute-scale anyway
#       min_interval: 30    # 30 s during change
#       threshold: 2.0
#       requires:
#         - Bash(git:*)
#         - Bash(gh:*)
#         - Bash(jq:*)
#
# ----------------------------------------------------------------------

set -euo pipefail

# Silent no-op if any dependency is missing. Opt-in sensor — we don't
# emit errors to stdout, which attend would interpret as a signal.
command -v git >/dev/null 2>&1 || exit 0
command -v gh  >/dev/null 2>&1 || exit 0
command -v jq  >/dev/null 2>&1 || exit 0

# Must be inside a git repo.
git rev-parse --is-inside-work-tree >/dev/null 2>&1 || exit 0

branch=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || true)
[[ -z "$branch" || "$branch" == "HEAD" ]] && exit 0  # detached head

# Skip base branches — unlikely to have an open PR and not worth the
# API cost of finding out every 2 minutes.
case "$branch" in
  main|master|develop|trunk) exit 0 ;;
esac

# --- Query the PR ------------------------------------------------------

# Single API call — grab the PR number and the full check rollup in one
# round trip. If the branch has no PR, `gh pr view` exits non-zero and
# rollup stays empty; that's a silent quiet poll.
rollup=$(gh pr view --json number,statusCheckRollup 2>/dev/null || true)
[[ -z "$rollup" ]] && exit 0

pr_number=$(printf '%s' "$rollup" | jq -r '.number // empty')
[[ -z "$pr_number" ]] && exit 0

# Roll up the per-check array into a single aggregate state. GitHub's
# API returns two shapes in the rollup — CheckRun entries carry
# `conclusion` + `status`, StatusContext entries carry `state` — so we
# coalesce with `conclusion // state // "PENDING"`. That trailing
# `"PENDING"` literal is load-bearing: an in-progress CheckRun has
# `conclusion: null, state: null`, so without the literal it would
# coalesce to null and break the aggregate comparison. With it, pending
# CheckRuns route cleanly through the `any(. == "PENDING" ...)` arm.
#
# Failure set: FAILURE, TIMED_OUT, CANCELLED, ACTION_REQUIRED, plus
# STARTUP_FAILURE (an Actions-side infra failure where the runner
# never started the job — masking this as pass would hide a real
# broken-CI signal). NEUTRAL and SKIPPED fall through to pass, which
# matches gh's own non-blocking semantics. STALE (a CheckRun
# conclusion meaning the check ran against a commit that's no longer
# HEAD) also falls through to pass — GitHub itself treats it as
# non-blocking, and re-running CI on the new HEAD will supersede it.
state=$(printf '%s' "$rollup" | jq -r '
  (.statusCheckRollup // [])
  | map(.conclusion // .state // "PENDING")
  | if length == 0 then "none"
    elif any(. == "FAILURE" or . == "TIMED_OUT" or . == "CANCELLED" or . == "ACTION_REQUIRED" or . == "STARTUP_FAILURE") then "fail"
    elif any(. == "IN_PROGRESS" or . == "PENDING" or . == "QUEUED" or . == "WAITING" or . == "EXPECTED") then "pending"
    else "pass" end
')

# Nothing to compare against (no checks configured on this repo).
[[ "$state" == "none" ]] && exit 0

# Pick the first failing check's name for the notification, if any.
# This gives the agent one concrete pointer instead of "something broke."
first_fail=$(printf '%s' "$rollup" | jq -r '
  (.statusCheckRollup // [])
  | map(select((.conclusion // .state) == "FAILURE" or (.conclusion // .state) == "TIMED_OUT" or (.conclusion // .state) == "CANCELLED" or (.conclusion // .state) == "STARTUP_FAILURE"))
  | .[0].name // .[0].context // empty
')

# --- State marker ------------------------------------------------------

STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/attend"
mkdir -p "$STATE_DIR"

# Key the marker on repo-root + branch + PR number. Including the PR
# number avoids a subtle footgun: if you close PR N on `feat/x` and
# open PR N+1 on the same branch, a repo+branch-only marker would
# carry PR N's terminal state into PR N+1's first transition, so the
# sensor might emit "recovered" or "regressed" on what is actually a
# brand-new PR. The sanitize pass strips anything filename-hostile
# and reduces (but does not guarantee) collisions across unrelated
# checkouts — see header note #4.
repo_key=$(git rev-parse --show-toplevel 2>/dev/null | tr -c 'a-zA-Z0-9' '_')
branch_key=$(printf '%s' "$branch" | tr -c 'a-zA-Z0-9' '_')
MARKER="$STATE_DIR/gh-pr-checks.${repo_key}.${branch_key}.pr${pr_number}.state"

# --- Compare against previous and emit --------------------------------

prev=""
[[ -f "$MARKER" ]] && prev=$(cat "$MARKER" 2>/dev/null || printf '')

# Update the marker unconditionally. Parse failures above already
# exited; if we get here the state is a known bucket.
printf '%s\n' "$state" > "$MARKER"

# First run — record state but don't emit. Prevents a flood of
# "checks passing" notifications whenever attend restarts.
[[ -z "$prev" ]] && exit 0

# No transition, no news.
[[ "$state" == "$prev" ]] && exit 0

# Magnitude table:
#
#   pass  after fail     → 3.5  (recovered — notable, not an alarm)
#   pass  after pending  → 3.0  (green on a fresh push — good news)
#   fail  after pass     → 4.5  (regression — break refractory hard)
#   fail  after pending  → 4.0  (broke on push — break refractory)
#   pending              → silent (user just pushed; they know)
#
# Failures sit above the refractory ceiling so they break through even
# when the agent is deep in another task. Passes are quieter — they
# land above the emission threshold (2.0) but won't jolt refractory.

case "$state" in
  pass)
    if [[ "$prev" == "fail" ]]; then
      printf '3.5|PR #%s recovered — all checks passing\n' "$pr_number"
    else
      printf '3.0|PR #%s checks all passing\n' "$pr_number"
    fi
    ;;
  fail)
    suffix=""
    [[ -n "$first_fail" ]] && suffix=" ($first_fail)"
    if [[ "$prev" == "pass" ]]; then
      printf '4.5|PR #%s regressed to failing%s\n' "$pr_number" "$suffix"
    else
      printf '4.0|PR #%s checks failing%s\n' "$pr_number" "$suffix"
    fi
    ;;
  pending)
    : # silent on pending entry — see design note at top
    ;;
esac

exit 0
