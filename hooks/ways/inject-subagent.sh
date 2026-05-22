#!/bin/bash
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
# context regardless of what the parent already triggered.

source "$(dirname "$0")/sessions-root.sh"

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

# Collect project-scope disabled ways once per invocation (ADR-131).
# Delegate to the `ways` CLI so the bash gate sees exactly what the Rust
# config parser sees — any divergence here is a subtle cross-path bug
# where the same overlay disables a way in one path but not the other.
DISABLED_WAYS=""
if command -v ways >/dev/null 2>&1; then
  DISABLED_WAYS=$(CLAUDE_PROJECT_DIR="$PROJECT_DIR" ways disable --list --names-only 2>/dev/null)
fi

# Emit way content for each matched way (bypassing markers)
CONTEXT=""
WAY_IDX=0

while IFS= read -r waypath; do
  [[ -z "$waypath" ]] && continue
  MATCH_CH="${CHANNEL_ARR[$WAY_IDX]:-prompt}"
  ((WAY_IDX++))

  # Resolve way file (project-local > global)
  WAY_FILE=""
  WAY_DIR=""
  # Find way file — any .md with frontmatter in the way directory
  for _base in "$PROJECT_DIR/.claude/ways" "${HOME}/.claude/hooks/ways"; do
    [[ -d "${_base}/${waypath}" ]] || continue
    for _f in "${_base}/${waypath}"/*.md; do
      [[ -f "$_f" ]] && head -1 "$_f" 2>/dev/null | grep -q '^---$' && {
        WAY_FILE="$_f"
        WAY_DIR="${_base}/${waypath}"
        break 2
      }
    done
  done
  [[ -z "$WAY_FILE" ]] && continue

  # Check domain disabled (user scope, legacy)
  DOMAIN="${waypath%%/*}"
  WAYS_CONFIG="${HOME}/.claude/ways.json"
  if [[ -f "$WAYS_CONFIG" ]]; then
    if jq -e --arg d "$DOMAIN" '.disabled | index($d) != null' "$WAYS_CONFIG" >/dev/null 2>&1; then
      continue
    fi
  fi

  # Check per-way disabled in project overlay (ADR-131)
  if [[ -n "$DISABLED_WAYS" ]] && grep -qxF "$waypath" <<< "$DISABLED_WAYS"; then
    continue
  fi

  # Extract macro position
  MACRO_POS=$(awk '/^---$/{p=!p; next} p && /^macro:/{gsub(/^macro: */, ""); print; exit}' "$WAY_FILE")
  MACRO_FILE="${WAY_DIR}/macro.sh"
  MACRO_OUT=""

  if [[ -n "$MACRO_POS" && -x "$MACRO_FILE" ]]; then
    # Project-local macros need trust check
    if [[ "$WAY_FILE" == "${HOME}/.claude/hooks/ways/"* ]]; then
      MACRO_OUT=$("$MACRO_FILE" 2>/dev/null)
    else
      # Check project trust for project-local macros
      trust_file="${HOME}/.claude/trusted-project-macros"
      if [[ -f "$trust_file" ]] && grep -qxF "$PROJECT_DIR" "$trust_file"; then
        MACRO_OUT=$("$MACRO_FILE" 2>/dev/null)
      fi
    fi
  fi

  # Build way output
  WAY_CONTENT=""
  if [[ "$MACRO_POS" == "prepend" && -n "$MACRO_OUT" ]]; then
    WAY_CONTENT+="$MACRO_OUT"$'\n'
  fi

  WAY_CONTENT+=$(awk 'BEGIN{fm=0} /^---$/{fm++; next} fm!=1' "$WAY_FILE")

  if [[ "$MACRO_POS" == "append" && -n "$MACRO_OUT" ]]; then
    WAY_CONTENT+=$'\n'"$MACRO_OUT"
  fi

  if [[ -n "$WAY_CONTENT" ]]; then
    CONTEXT+="$WAY_CONTENT"$'\n\n'
    scope="subagent"
    [[ "$IS_TEAMMATE" == "true" ]] && scope="teammate"
    log_args=(event=way_fired way="$waypath" domain="$DOMAIN"
      trigger="${MATCH_CH}" scope="$scope" project="$PROJECT_DIR" session="$SESSION_ID")
    [[ -n "$TEAM_NAME" ]] && log_args+=(team="$TEAM_NAME")
    # Inline event logging
    mkdir -p "${HOME}/.claude/stats" 2>/dev/null
    _args=(--arg ts "$(date -u +%Y-%m-%dT%H:%M:%SZ)") _obj="ts:\$ts"
    for _kv in "${log_args[@]}"; do _args+=(--arg "${_kv%%=*}" "${_kv#*=}"); _obj+=",${_kv%%=*}:\$${_kv%%=*}"; done
    jq -nc "${_args[@]}" "{${_obj}}" >> "${HOME}/.claude/stats/events.jsonl" 2>/dev/null
  fi
done <<< "$WAYS"

# Output JSON for SubagentStart (additionalContext format)
if [[ -n "$CONTEXT" ]]; then
  TRIMMED="${CONTEXT%$'\n\n'}"
  # Guard against whitespace-only content from malformed ways
  if [[ -n "${TRIMMED// /}" ]]; then
    jq -n --arg ctx "$TRIMMED" '{
      hookSpecificOutput: {
        hookEventName: "SubagentStart",
        additionalContext: $ctx
      }
    }'
  fi
fi
