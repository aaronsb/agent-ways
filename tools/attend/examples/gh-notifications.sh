#!/usr/bin/env bash
#
# gh-notifications.sh — example attend external sensor
#
# Watches the user's GitHub notification inbox and emits one line per
# new notification since the previous poll. Different shape from the
# other shipped sensors, which is the whole reason it exists as a
# second reference:
#
#   - `xdg-downloads.sh` demonstrates a **local filesystem scan with
#     count-based tiers**: scan a directory, diff against a marker,
#     emit once with magnitude tied to how many new files.
#   - `gh-pr-checks.sh` demonstrates a **per-branch aggregate state
#     machine**: three buckets, emit only on terminal transitions.
#   - `gh-notifications.sh` (this file) demonstrates a **network API
#     stream keyed on a rolling timestamp**: query a remote endpoint
#     with `?since=<marker>`, emit one line per returned item, tier
#     magnitude by the *type* of event rather than an aggregate.
#
# Taken together the three sensors cover the failure modes an author
# will hit in the wild: local state, remote state that needs rollup,
# and remote state that arrives as a stream.
#
# Design choices worth the ink:
#
#   1. **Marker is a wall-clock timestamp, not a set of seen IDs.**
#      The notifications endpoint supports `?since=<ISO8601>`. Using
#      it directly means we never have to remember which notification
#      IDs we already saw; the server does the filtering. If a poll
#      fails, we leave the marker alone so the next successful poll
#      picks up the dropped window.
#
#   2. **First-run is silent.** The notifications inbox accumulates
#      forever, so the first poll against it would otherwise flood
#      the session with every unread notification the user has. We
#      record the current UTC timestamp and exit quietly; only events
#      that land *after* attend started get surfaced.
#
#   3. **Magnitude is keyed on `reason`.** GitHub tells us *why* it
#      thinks we care (`review_requested`, `mention`, `author`,
#      `comment`, etc.), which is exactly the information an author
#      should use to design magnitude tiers. Reasons we don't have a
#      magnitude for are silently dropped — this keeps `subscribed`
#      repo-watching noise from swamping the session. Add rows to
#      the jq table below if you want more reasons surfaced.
#
#   4. **Silent on auth or network failure.** If `gh auth status`
#      can't reach GitHub (offline, token expired, rate-limited) the
#      sensor exits 0 with no output. The sensor contract treats a
#      quiet poll as "nothing changed," which is the right framing
#      here — attend shouldn't be spamming error lines into the
#      conversation on every poll when the API is unreachable.
#
# Prerequisites: `gh` (authenticated), `jq`. Script exits silently if
# either is missing.
#
# Author's lever: magnitude + the reason table. Everything else is
# attend's job.
#
# --- Config recipe ------------------------------------------------------
#
# Add to ~/.config/attend/config.yaml (or a project-scope overlay):
#
#   sensors:
#     +gh-notifications:
#       script: ~/.claude/tools/attend/examples/gh-notifications.sh
#       enabled: true
#       interval: 180      # 3 min at rest — inbox is minute-scale
#       min_interval: 60   # 1 min during ramp-up
#       threshold: 2.0
#       requires:
#         - Bash(gh:*)
#         - Bash(jq:*)
#
# A commented-out stub for this sensor lives in the default user-scope
# config — flip the comments and set `enabled: true` to turn it on.
#
# ----------------------------------------------------------------------

set -euo pipefail

# Silent no-op if dependencies are missing. Exit 0 = "nothing observed
# this poll", which matches the sensor-layer contract.
command -v gh >/dev/null 2>&1 || exit 0
command -v jq >/dev/null 2>&1 || exit 0

# Silent on unauthenticated / offline / rate-limited.
gh auth status >/dev/null 2>&1 || exit 0

STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/attend"
mkdir -p "$STATE_DIR"
MARKER="$STATE_DIR/gh-notifications.marker"

# GitHub wants ISO8601 UTC in the `since` query parameter. The exact
# format is `YYYY-MM-DDTHH:MM:SSZ`; anything else is rejected with 422.
now_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)

# First run: record current time, emit nothing. This is what keeps us
# from flooding the session with every unread notification on startup.
if [[ ! -f "$MARKER" ]]; then
  printf '%s\n' "$now_utc" > "$MARKER"
  exit 0
fi

since=$(cat "$MARKER" 2>/dev/null || printf '')
if [[ -z "$since" ]]; then
  # Empty or unreadable marker — reset to now and wait for next tick.
  printf '%s\n' "$now_utc" > "$MARKER"
  exit 0
fi

# --- Query notifications ----------------------------------------------

# Single API call. `per_page=100` is the max; if you're getting more
# than 100 notifications in one poll interval, something external
# (repo-watch spam, auto-generated mentions) is filling your inbox
# faster than attend should be surfacing it — the table below drops
# `subscribed` by default for exactly this reason.
#
# If the API call fails we leave the marker alone so the next
# successful poll catches the dropped window.
response=$(gh api "notifications?since=${since}&per_page=100" 2>/dev/null || true)
[[ -z "$response" ]] && exit 0

# API call succeeded — advance the marker before emitting, so a
# downstream jq failure doesn't make us re-emit the same notifications
# on the next poll.
printf '%s\n' "$now_utc" > "$MARKER"

# --- Emit ---------------------------------------------------------------
#
# Magnitude hierarchy, keyed on GitHub's `reason` field:
#
#   security_alert      → 5.0  (dependabot / supply chain — top priority)
#   review_requested    → 4.5  (someone explicitly wants your review)
#   mention             → 3.5  (you were @-mentioned)
#   team_mention        → 3.0  (a team you're on was mentioned)
#   assign              → 3.0  (you were assigned)
#   invitation          → 3.0  (you were invited)
#   author              → 2.5  (activity on a thread you started)
#   push                → 2.0  (push to a PR branch you're involved in)
#   state_change        → 2.0  (status changed on a thread you watch)
#   comment             → 1.5  (subscribed thread got a comment — needs accumulation)
#   manual              → 1.5  (you explicitly subscribed)
#   anything else       → silent (subscribed, ci_activity, etc.)
#
# Magnitudes above 2.0 surface individually once the emission threshold
# is crossed; 1.5 items only fire after multiple land in the same
# window. `ci_activity` is deliberately dropped — gh-pr-checks already
# covers that surface and we don't want double notification. `subscribed`
# is dropped because repo-watch notifications are too chatty.
#
# The subshell + `|| true` keeps a jq parse failure from killing the
# script with a non-zero exit — we'd rather lose one poll's output
# than leave the marker un-advanced.
(printf '%s' "$response" | jq -r '
  .[]
  | select(.unread == true)
  | . as $n
  | {
      mag: (
        if   .reason == "security_alert"   then 5.0
        elif .reason == "review_requested" then 4.5
        elif .reason == "mention"          then 3.5
        elif .reason == "team_mention"     then 3.0
        elif .reason == "assign"           then 3.0
        elif .reason == "invitation"       then 3.0
        elif .reason == "author"           then 2.5
        elif .reason == "push"             then 2.0
        elif .reason == "state_change"     then 2.0
        elif .reason == "comment"          then 1.5
        elif .reason == "manual"           then 1.5
        else empty
        end
      ),
      reason: $n.reason,
      # Truncate titles so a single notification stays well under
      # Monitor'"'"'s per-line buffer. Full title is always available
      # via `gh api notifications/threads/<id>` if needed.
      title: ($n.subject.title
              | if length > 80 then .[0:77] + "..." else . end),
      kind: ($n.subject.type // "Thread"),
      repo: ($n.repository.full_name // "?")
    }
  | "\(.mag)|\(.kind) \(.reason) in \(.repo): \(.title)"
') || true

exit 0
