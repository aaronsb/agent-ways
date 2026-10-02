#!/usr/bin/env bash
# Check if agent-ways is up to date with upstream.
#
# The app source lives in $XDG_DATA_HOME/agent-ways and ~/.claude is a thin
# symlink projection of it (ADR-142). Check the app source for behind-upstream.
# Only the direct-install case is nudged (origin is upstream); a native fork or a
# non-GitHub origin stamps a behind=0 cache and gets no nudge.
#
# The in-place clone, ADR-140 subdirectory, fork, renamed-clone and plugin
# layouts are no longer checked (ADR-505, ADR-506). Nothing here migrates them:
# `ways reconcile` does not (it stops at the real directories of an in-place
# clone and changes nothing). The pre-1.0 migrator ships only at the
# `ways-v1.8.3` tag; see docs/migration-1.0.md.
#
# Network calls (git fetch) are rate-limited to once per hour.
# Writes state to cache file; display is handled by `ways show core`.

UPSTREAM_REPO="aaronsb/agent-ways"
# Cache file path MUST match metrics::update_status_text() in the ways binary
# (the reader). Unix keys by uid under /tmp; Windows uses the per-user
# LOCALAPPDATA base (no uid — LOCALAPPDATA is already per-user, and the parent
# always exists so the atomic mv below succeeds).
case "$(uname -s 2>/dev/null)" in
  MINGW*|MSYS*|CYGWIN*)
    CACHE_FILE="${LOCALAPPDATA}/.claude-config-update-state"
    ;;
  *)
    CACHE_FILE="/tmp/.claude-config-update-state-$(id -u)"
    ;;
esac
ONE_HOUR=3600
CURRENT_TIME=$(date +%s)

# --- Helpers ---

needs_refresh() {
  [[ ! -f "$CACHE_FILE" ]] && return 0
  local last_fetch
  last_fetch=$(sed -n 's/^fetched=//p' "$CACHE_FILE" 2>/dev/null)
  [[ -z "$last_fetch" ]] && return 0
  (( CURRENT_TIME - last_fetch >= ONE_HOUR ))
}

# Atomic cache write — write to temp then mv to avoid races between sessions
write_cache() {
  # 4th arg lets a caller preserve a prior fetch timestamp (so a cache it rewrites
  # every run doesn't reset the once-per-hour fetch rate limit). Defaults to now.
  local type="$1" behind="$2" extra="$3" fetched="${4:-$CURRENT_TIME}"
  local tmp="${CACHE_FILE}.$$"
  {
    echo "fetched=${fetched}"
    echo "type=${type}"
    echo "behind=${behind}"
    [[ -n "$extra" ]] && echo "$extra"
  } > "$tmp"
  mv -f "$tmp" "$CACHE_FILE"
}

# --- Native XDG projection (1.0, ADR-142) ---

APP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/agent-ways"
if git -C "$APP_DIR" rev-parse --git-dir >/dev/null 2>&1; then
  APP_REMOTE=$(git -C "$APP_DIR" remote get-url origin 2>/dev/null)
  APP_OWNER_REPO=$(echo "$APP_REMOTE" | sed -E 's#.*github\.com[:/]##; s/\.git$//')
  if [[ "$APP_REMOTE" == *github.com* && "$APP_OWNER_REPO" == "$UPSTREAM_REPO" ]]; then
    PREV_FETCH=$(sed -n 's/^fetched=//p' "$CACHE_FILE" 2>/dev/null | head -1)
    FETCH_TS="${PREV_FETCH:-0}"
    if needs_refresh; then
      timeout 10 git -C "$APP_DIR" fetch origin --quiet 2>/dev/null
      FETCH_TS="$CURRENT_TIME"
    fi
    BEHIND=$(git -C "$APP_DIR" rev-list HEAD..origin/main --count 2>/dev/null || echo 0)
    write_cache "native" "$BEHIND" "repo=${APP_DIR}" "$FETCH_TS"
  else
    # Native fork / non-GitHub origin: behind-upstream isn't computed, but still
    # stamp a native-type cache (behind=0, no nudge) so a stale entry from an
    # earlier layout can't render a wrong command.
    write_cache "native" "0" "repo=${APP_DIR}"
  fi
fi
exit 0
