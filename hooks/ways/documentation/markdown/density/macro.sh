#!/usr/bin/env bash
# Macro for documentation/markdown/density — names the file and the counts.
#
# postcheck.sh detects and records; this reads the record. That handoff exists
# because check-post.sh runs postchecks with stdout to /dev/null and reads only
# the exit code, and macro.sh itself gets no stdin and no arguments — so a
# session-scoped file is the only channel by which a reactive way can name the
# artifact it is reacting to (ADR-123, amended 2026-07-29).
#
# The numbers are the entire point of this way. A restated rule doesn't help,
# because the failure being addressed is that the writer cannot detect the
# violation while producing it. A count can.
#
# Silence is the correct output when there's nothing pending: the way body still
# fires, just without the measurements.
set -uo pipefail

source "$(dirname "$0")/../../../sessions-root.sh"

# `ways show way` exports this for every macro. No guessing fallback: inferring
# the session from directory mtimes silently picks the wrong one whenever two
# sessions are active, and naming the wrong file is worse than naming none.
SESSION="${CLAUDE_SESSION_ID:-}"
[[ -n "$SESSION" ]] || exit 0

PENDING="${SESSIONS_ROOT}/${SESSION}/markdown-density/pending"
[[ -f "$PENDING" ]] || exit 0

NOW=$(date +%s)
TTL=900   # 15 minutes

# Claim by renaming before reading. Reading and then deleting would silently
# drop any entry a postcheck appended in between; rename is atomic, so a
# concurrent append lands on a fresh file and is reported next fire instead.
CLAIM="${PENDING}.consume.$$"
mv -f "$PENDING" "$CLAIM" 2>/dev/null || exit 0

# Consume unconditionally: a denied fire leaves entries behind, and reporting
# counts from twenty minutes ago is worse than reporting none.
FRESH=()
while IFS=$'\t' read -r stamp path words sig sig_rate dash_rate; do
  [[ -n "${path:-}" ]] || continue
  [[ "$stamp" =~ ^[0-9]+$ ]] || continue
  (( NOW - stamp <= TTL )) || continue
  FRESH+=("$(printf '`%s` — %s words, %s significance clauses (%s per 1k), %s em-dashes per 1k' \
    "$path" "${words:-?}" "${sig:-?}" "${sig_rate:-?}" "${dash_rate:-?}")")
done <"$CLAIM"

rm -f "$CLAIM"

(( ${#FRESH[@]} )) || exit 0

if (( ${#FRESH[@]} == 1 )); then
  printf '**Decoration density in markdown just written:** %s\n' "${FRESH[0]}"
else
  printf '**Decoration density in markdown just written:**\n'
  printf -- '- %s\n' "${FRESH[@]}"
fi
