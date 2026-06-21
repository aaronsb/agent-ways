#!/usr/bin/env bash
#
# gh-monitor inbox-watch — persistent watch over your GitHub notifications.
#
# Launched by the gh-monitor skill via the Monitor tool (persistent: true).
# Each new, relevant notification since the previous poll becomes one line —
# one notification in the chat. Runs until you stop it with TaskStop.
#
# This is the session-scoped, opt-in home for notification awareness — the
# thing that this project established is a Monitor concern, not an ambient
# attend sensor (ADR-137). The reason->tier filter is lifted from the example
# sensor tools/attend/examples/gh-notifications.sh; the magnitude floats are
# dropped here because Monitor lines are plain text, not attend's mag|text.
#
# Env: GH_MONITOR_INTERVAL  poll seconds (default 60 — matches GitHub's
#                           X-Poll-Interval header on the notifications API)
#
# Marker: a rolling wall-clock timestamp passed as `?since=`. We start at
# "now" so launch doesn't dump the whole backlog, and we advance it only after
# a *successful* poll — a failed request leaves the window intact so the next
# poll catches what we missed.
#
# Upgrade path (verified supported, deferred for v1): the notifications API
# honors `If-None-Match` against a stored ETag and returns 304 Not Modified
# when nothing changed, making idle polls free against the rate-limit budget.
# `gh api --include` exposes the ETag header. At a 60s interval the `since=`
# approach here stays comfortably within rate limits, so this is pure
# efficiency, not correctness.

set -uo pipefail

INTERVAL="${GH_MONITOR_INTERVAL:-60}"

command -v gh >/dev/null 2>&1 || { echo "gh-monitor inbox: gh not installed"; exit 1; }
command -v jq >/dev/null 2>&1 || { echo "gh-monitor inbox: jq not installed"; exit 1; }
gh auth status >/dev/null 2>&1 || { echo "gh-monitor inbox: gh not authenticated (run: gh auth login)"; exit 1; }

since=$(date -u +%Y-%m-%dT%H:%M:%SZ)
echo "gh-monitor inbox: watching GitHub notifications since $since (polling every ${INTERVAL}s)…"

while true; do
  now=$(date -u +%Y-%m-%dT%H:%M:%SZ)

  # `|| true` so a transient failure can't kill the watch. On success gh
  # returns a JSON array ([] when nothing is new); on failure it returns "".
  resp=$(gh api "notifications?since=${since}&per_page=100" 2>/dev/null || true)
  if [[ -z "$resp" ]]; then
    # Transient failure / offline / rate-limited: keep the window, retry.
    sleep "$INTERVAL"
    continue
  fi

  # Success — advance the marker before emitting so a downstream jq error
  # can't make us replay the same notifications next poll.
  since="$now"

  if [[ "$resp" != "[]" ]]; then
    # Tier filter keyed on GitHub's `reason`: only surface reasons we have a
    # tier for; `subscribed`, `ci_activity`, etc. fall through to nothing —
    # CI watching is the `ci` mode's job, repo-watch chatter is noise.
    (jq -r '
      ["security_alert","review_requested","mention","team_mention",
       "assign","invitation","author","push","state_change","comment","manual"] as $tiers
      | .[]
      | select(.unread == true)
      | select(.reason as $r | $tiers | index($r))
      | { reason,
          kind:  (.subject.type // "Thread"),
          repo:  (.repository.full_name // "?"),
          title: ((.subject.title // "")
                  | if length > 80 then .[0:77] + "..." else . end) }
      | "[\(.reason)] \(.kind) in \(.repo): \(.title)"
    ' <<<"$resp") || true
  fi

  sleep "$INTERVAL"
done
