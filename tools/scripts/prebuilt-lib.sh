#!/usr/bin/env bash
# Shared helpers for the per-component pre-built download scripts
# (download-attend.sh, download-attend-chat.sh, download-ways.sh,
# download-ways-audit.sh). Sourced, not executed.
#
# Why this exists: a transient GitHub API / network blip on a `gh release`
# call used to be swallowed (the call was `... 2>/dev/null`), so an empty
# result read as "no release" and `ways update` silently degraded to a
# from-source build. These helpers retry transient failures and — critically —
# distinguish "the API call failed" from "the API succeeded but found nothing",
# so a real blip is retried and, if it persists, reported honestly instead of
# masquerading as a missing binary.

# Run a command, retrying on failure with exponential backoff. Returns the
# command's own exit status once it succeeds, or non-zero after the last try.
# Progress notes go to stderr so callers can capture stdout cleanly.
retry() {
  local n=1 max="${RETRY_MAX:-3}" delay="${RETRY_DELAY:-2}"
  while true; do
    if "$@"; then
      return 0
    fi
    if [[ "$n" -ge "$max" ]]; then
      return 1
    fi
    echo "  (attempt ${n}/${max} failed; retrying in ${delay}s...)" >&2
    sleep "$delay"
    n=$((n + 1))
    delay=$((delay * 2))
  done
}
