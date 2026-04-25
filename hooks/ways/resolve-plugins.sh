#!/bin/bash
# Resolve enabled Claude Code plugin way-paths at session start.
#
# Runs `claude plugin list --json`, filters to enabled plugins that ship
# hooks/ways/ directories, and writes a session-scoped manifest so the
# ways binary can scan them without per-invocation subprocess overhead.
#
# ADR-129: Plugin Way Discovery

source "$(dirname "$0")/sessions-root.sh"

INPUT=$(cat)
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty')

[[ -z "$SESSION_ID" ]] && exit 0

# Resolve plugin list via official CLI (stable interface)
PLUGIN_JSON=$(claude plugin list --json 2>/dev/null) || exit 0

# Filter to enabled plugins whose installPath contains hooks/ways/
MANIFEST=$(echo "$PLUGIN_JSON" | jq -c '
  [ .[]
    | select(.enabled == true)
    | . as $p
    | ($p.installPath + "/hooks/ways") as $wpath
    | { id: $p.id, path: $wpath, scope: ($p.scope // "user"), projectPath: ($p.projectPath // null) }
  ]
')

# Filter to plugins where hooks/ways/ actually exists on disk
RESULT="[]"
for row in $(echo "$MANIFEST" | jq -c '.[]'); do
  WPATH=$(echo "$row" | jq -r '.path')
  if [[ -d "$WPATH" ]]; then
    RESULT=$(echo "$RESULT" | jq -c --argjson entry "$row" '. + [$entry]')
  fi
done

# Write manifest to session directory
SESSION_DIR="${SESSIONS_ROOT}/${SESSION_ID}"
mkdir -p "$SESSION_DIR"
echo "$RESULT" > "${SESSION_DIR}/plugin-ways.json"
