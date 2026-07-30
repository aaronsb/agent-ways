#!/usr/bin/env bash
# Macro for documentation/markdown/reflow (#415) — names the file.
#
# postcheck.sh detects and records; this reads the record. That handoff exists
# because check-post.sh runs postchecks with stdout to /dev/null and reads only
# the exit code, and macro.sh itself gets no stdin and no arguments — so a
# session-scoped file is the only channel by which a reactive way can name the
# artifact it is reacting to (ADR-123, amended 2026-07-29).
#
# Silence is the correct output when there's nothing pending: the way body still
# fires, just without a filename.
set -uo pipefail

source "$(dirname "$0")/../../../sessions-root.sh"

# `ways show way` exports this for every macro. No guessing fallback: inferring
# the session from directory mtimes silently picks the wrong one whenever two
# sessions are active, and naming the wrong file is worse than naming none.
SESSION="${CLAUDE_SESSION_ID:-}"
[[ -n "$SESSION" ]] || exit 0

PENDING="${SESSIONS_ROOT}/${SESSION}/markdown-reflow/pending"
[[ -f "$PENDING" ]] || exit 0

NOW=$(date +%s)
TTL=900   # 15 minutes

# Consume unconditionally: a denied fire leaves entries behind, and naming a
# file from twenty minutes ago is worse than naming none.
FRESH=()
while IFS=$'\t' read -r stamp path; do
  [[ -n "${path:-}" ]] || continue
  [[ "$stamp" =~ ^[0-9]+$ ]] || continue
  (( NOW - stamp <= TTL )) && FRESH+=("$path")
done <"$PENDING"

rm -f "$PENDING"

(( ${#FRESH[@]} )) || exit 0

if (( ${#FRESH[@]} == 1 )); then
  printf '**Hard-wrapped markdown just written:** `%s`\n' "${FRESH[0]}"
else
  printf '**Hard-wrapped markdown just written:**\n'
  printf -- '- `%s`\n' "${FRESH[@]}"
fi
