#!/usr/bin/env bash
# Freshness check (v1: documentation). Surface README.md / docs/ when their git
# history lags far behind HEAD and no local branch already carries an update.
# Silent on the happy path — emits nothing unless there's something worth a look.
#
# Tunable: WAYS_FRESHNESS_COMMITS (default 25) — how far HEAD may advance past
# the last doc-touching commit before this fires.
#
# v1 scope is docs. The same shape applies to other derived/descriptive
# artifacts (lockfile vs manifest, generated client vs schema); add those as
# additional path-pair checks here rather than as new ways.

git rev-parse --is-inside-work-tree &>/dev/null || exit 0

THRESHOLD=${WAYS_FRESHNESS_COMMITS:-25}
PATHS=(README.md docs)

# Most recent commit that touched any of PATHS.
last=$(git log -1 --format=%H -- "${PATHS[@]}" 2>/dev/null)
[[ -z "$last" ]] && exit 0   # none of these paths are tracked here — nothing to say

# How far has HEAD advanced since then?
behind=$(git rev-list --count "$last"..HEAD 2>/dev/null)
[[ -z "$behind" || "$behind" -lt "$THRESHOLD" ]] && exit 0   # keeping pace

# Suppress if a local branch ahead of the current one already touches PATHS —
# the update is in flight, no nudge needed.
cur=$(git symbolic-ref --short HEAD 2>/dev/null)
[[ -z "$cur" ]] && cur=HEAD
while IFS= read -r b; do
  [[ -z "$b" || "$b" == "$cur" ]] && continue
  if [[ -n "$(git rev-list "$cur".."$b" -- "${PATHS[@]}" 2>/dev/null | head -1)" ]]; then
    exit 0
  fi
done < <(git for-each-ref --format='%(refname:short)' refs/heads 2>/dev/null)

ts=$(git log -1 --format=%ct "$last" 2>/dev/null)
if [[ -n "$ts" ]]; then
  months=$(( ( $(date +%s) - ts ) / 2592000 ))
  age=" (~${months} mo)"
fi
echo "📄 **Freshness:** \`README.md\`/\`docs/\` last had a substantive commit ${behind} commits back${age}, and no local branch carries an update. Worth a pass — keeping in mind this flags the *abandoned* doc, not the *subtly wrong* one (stale counts, dead links — recency can't see those)."
echo ""
