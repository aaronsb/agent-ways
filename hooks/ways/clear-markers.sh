#!/usr/bin/env bash
# Clear way markers for fresh session
# Called on SessionStart and after compaction
#
# Reads session_id from stdin JSON input (Claude Code hook format)
# Clears this session's state directory only — other sessions stay intact

source "$(dirname "$0")/sessions-root.sh"
source "$(dirname "$0")/events-log.sh"

INPUT=$(cat)
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty' 2>/dev/null)

# Clear session state
if [[ -n "$SESSION_ID" ]]; then
  rm -rf "${SESSIONS_ROOT}/${SESSION_ID}" 2>/dev/null
else
  # No session ID — legacy fallback, clear everything
  rm -rf "${SESSIONS_ROOT}" 2>/dev/null
fi

# Log session event (canonical events-log path resolved via events-log.sh)
mkdir -p "$(dirname "$EVENTS_LOG")" 2>/dev/null
jq -nc --arg ts "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg event "session_start" \
  --arg project "${CLAUDE_PROJECT_DIR:-$PWD}" \
  --arg session "${SESSION_ID:-unknown}" \
  '{ts:$ts,event:$event,project:$project,session:$session}' \
  >> "$EVENTS_LOG" 2>/dev/null
