#!/usr/bin/env bash
#
# gh-monitor ci-watch — until-terminal CI watch for one PR's checks.
#
# Launched by the gh-monitor skill via the Monitor tool (persistent: false).
# Each stdout line becomes a notification; the script exits when CI reaches a
# terminal state, which ends the watch. That boundedness is the point — this
# watches *this push's* CI and stops, it is not an ambient sensor (ADR-137).
#
# Shape: poll `gh pr checks` every INTERVAL seconds, announce each check the
# first time it lands in a terminal bucket, and emit one final PASS/FAIL
# aggregate before exiting once nothing is pending. On fast CI where the first
# poll already finds everything terminal, that single poll emits and exits.
#
# Adapted from the retired attend sensor tools/attend/examples/gh-pr-checks.sh
# (recover with: git show 78028f3~1:tools/attend/examples/gh-pr-checks.sh).
# That version rolled the raw statusCheckRollup into one aggregate because an
# ambient sensor wants aggregate-only; here we want per-check visibility, so we
# read gh's own per-check buckets (pass|fail|pending|skipping|cancel) directly.
#
# Usage: ci-watch.sh [<pr-number> | <url> | <branch>]   (default: current branch)
# Env:   GH_MONITOR_INTERVAL  poll seconds (default 30)
#
# Coverage note: emits on PASS *and* fail/cancel — a watch that only announced
# success would be silent through a failure, and silence reads as "still
# running." See the Monitor tool's "silence is not success" guidance.

set -uo pipefail

PR="${1:-}"
INTERVAL="${GH_MONITOR_INTERVAL:-30}"

command -v gh >/dev/null 2>&1 || { echo "gh-monitor ci: gh not installed"; exit 1; }
command -v jq >/dev/null 2>&1 || { echo "gh-monitor ci: jq not installed"; exit 1; }
gh auth status >/dev/null 2>&1 || { echo "gh-monitor ci: gh not authenticated (run: gh auth login)"; exit 1; }

# Resolve the PR once: gives a stable identity for the notifications and lets
# us distinguish "no PR to watch" (a clean exit) from a transient query failure.
pr_num=$(gh pr view $PR --json number --jq .number 2>/dev/null || true)
if [[ -z "$pr_num" ]]; then
  echo "gh-monitor ci: no open PR found${PR:+ for '$PR'} — nothing to watch"
  exit 0
fi

echo "gh-monitor ci: watching checks on PR #$pr_num (polling every ${INTERVAL}s)…"

declare -A announced   # check name -> bucket already announced

while true; do
  # `|| true`: one failed poll (rate limit, blip) must not kill the watch.
  json=$(gh pr checks "$pr_num" --json name,bucket 2>/dev/null || true)

  if [[ -z "$json" || "$json" == "[]" ]]; then
    # gh exits non-zero with empty stdout when a PR has no checks at all.
    echo "gh-monitor ci: PR #$pr_num has no checks configured — done"
    exit 0
  fi

  # Announce each check the first time it reaches a terminal bucket.
  while IFS=$'\t' read -r name bucket; do
    [[ -z "$name" || "$bucket" == "pending" ]] && continue
    if [[ "${announced[$name]:-}" != "$bucket" ]]; then
      case "$bucket" in
        pass)     echo "✓ $name — passed" ;;
        fail)     echo "✗ $name — FAILED" ;;
        cancel)   echo "⊘ $name — cancelled" ;;
        skipping) echo "↪ $name — skipped" ;;
        *)        echo "• $name — $bucket" ;;
      esac
      announced[$name]="$bucket"
    fi
  done < <(jq -r '.[] | "\(.name)\t\(.bucket)"' <<<"$json")

  # Terminal once nothing is pending. Emit one aggregate line, then exit.
  if ! jq -e 'any(.[]; .bucket=="pending")' <<<"$json" >/dev/null 2>&1; then
    fails=$(jq -r '[.[] | select(.bucket=="fail" or .bucket=="cancel") | .name] | join(", ")' <<<"$json")
    if [[ -n "$fails" ]]; then
      echo "gh-monitor ci: PR #$pr_num — CI FAILED ($fails)"
    else
      n=$(jq 'length' <<<"$json")
      echo "gh-monitor ci: PR #$pr_num — CI PASSED ($n checks)"
    fi
    exit 0
  fi

  sleep "$INTERVAL"
done
