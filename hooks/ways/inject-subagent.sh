#!/usr/bin/env bash
# SubagentStart - Inject subagent-scoped ways from stash
#
# TRIGGER FLOW:
# ┌────────────────┐     ┌──────────────────┐     ┌──────────────────┐
# │ SubagentStart  │────▶│ read stash file  │────▶│ emit way content │
# │ (hook event)   │     │ (oldest first)   │     │ (bypass markers) │
# └────────────────┘     └──────────────────┘     └──────────────────┘
#
# Phase 2 of two-phase subagent injection:
# 1. PreToolUse:Task (check-task-pre.sh) stashed matched way paths
# 2. This script reads the stash, emits way content as additionalContext
#
# Way content is emitted WITHOUT marker checks - subagents get fresh
# context regardless of what the parent already triggered. The binary
# resolves, disable-checks and renders each way (`ways show way --subagent`),
# so the ways roots and macro trust live in one place.

source "$(dirname "$0")/require-ways.sh"
source "$(dirname "$0")/sessions-root.sh"
source "$(dirname "$0")/events-log.sh"

INPUT=$(cat)
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty')
AGENT_ID=$(echo "$INPUT" | jq -r '.agent_id // empty')
[[ -n "$AGENT_ID" ]] && export CLAUDE_AGENT_ID="$AGENT_ID"
PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$(echo "$INPUT" | jq -r '.cwd // empty')}"

[[ -z "$SESSION_ID" ]] && exit 0

STASH_DIR="${SESSIONS_ROOT}/${SESSION_ID}/subagent-stash"
[[ ! -d "$STASH_DIR" ]] && exit 0

# Claim the oldest stash file (FIFO for parallel Task invocations)
OLDEST=$(ls "$STASH_DIR"/*.json 2>/dev/null | sort | head -1)
[[ -z "$OLDEST" ]] && exit 0

# Atomic claim: rename so no other SubagentStart grabs it
CLAIMED="${OLDEST}.claimed"
mv "$OLDEST" "$CLAIMED" 2>/dev/null || exit 0

# Read matched way paths, channels, teammate flag, and team name
WAYS=$(jq -r '.ways[]' "$CLAIMED" 2>/dev/null)
CHANNELS=$(jq -r '.channels // [] | .[]' "$CLAIMED" 2>/dev/null)
IS_TEAMMATE=$(jq -r '.is_teammate // false' "$CLAIMED" 2>/dev/null)
TEAM_NAME=$(jq -r '.team_name // empty' "$CLAIMED" 2>/dev/null)
rm -f "$CLAIMED"

# Build channel lookup array
declare -a CHANNEL_ARR
while IFS= read -r ch; do
  CHANNEL_ARR+=("$ch")
done <<< "$CHANNELS"

# If this is a teammate spawn, write a marker the teammate's own hooks can detect
# The marker persists for the teammate's session lifetime
if [[ "$IS_TEAMMATE" == "true" ]]; then
  mkdir -p "${SESSIONS_ROOT}/${SESSION_ID}"
  echo "${TEAM_NAME}" > "${SESSIONS_ROOT}/${SESSION_ID}/teammate"
fi

[[ -z "$WAYS" ]] && exit 0

# Emit way content for each matched way (bypassing markers)
CONTEXT=""
WAY_IDX=0

while IFS= read -r waypath; do
  [[ -z "$waypath" ]] && continue
  MATCH_CH="${CHANNEL_ARR[$WAY_IDX]:-prompt}"
  ((WAY_IDX++))
  DOMAIN="${waypath%%/*}"

  WAY_CONTENT=$(CLAUDE_PROJECT_DIR="$PROJECT_DIR" "$WAYS_BIN" show way "$waypath" \
    --session "$SESSION_ID" --subagent 2>/dev/null)

  if [[ -n "$WAY_CONTENT" ]]; then
    CONTEXT+="$WAY_CONTENT"$'\n\n'
    scope="subagent"
    [[ "$IS_TEAMMATE" == "true" ]] && scope="teammate"
    log_args=(event=way_fired way="$waypath" domain="$DOMAIN"
      trigger="${MATCH_CH}" scope="$scope" project="$PROJECT_DIR" session="$SESSION_ID")
    [[ -n "$TEAM_NAME" ]] && log_args+=(team="$TEAM_NAME")
    # Inline event logging (canonical events-log path resolved via events-log.sh)
    mkdir -p "$(dirname "$EVENTS_LOG")" 2>/dev/null
    _args=(--arg ts "$(date -u +%Y-%m-%dT%H:%M:%SZ)") _obj="ts:\$ts"
    for _kv in "${log_args[@]}"; do _args+=(--arg "${_kv%%=*}" "${_kv#*=}"); _obj+=",${_kv%%=*}:\$${_kv%%=*}"; done
    jq -nc "${_args[@]}" "{${_obj}}" >> "$EVENTS_LOG" 2>/dev/null
  fi
done <<< "$WAYS"

# Output JSON for SubagentStart (additionalContext format)
if [[ -n "$CONTEXT" ]]; then
  TRIMMED="${CONTEXT%$'\n\n'}"
  # Guard against whitespace-only content from malformed ways. A regex test
  # stops at the first visible character; `${TRIMMED// /}` rewrote the whole
  # string, superlinear in a UTF-8 locale: 150 ms on 13 KB of ways (#705).
  if [[ $TRIMMED =~ [^[:space:]] ]]; then
    jq -n --arg ctx "$TRIMMED" '{
      hookSpecificOutput: {
        hookEventName: "SubagentStart",
        additionalContext: $ctx
      }
    }'
  fi
fi
