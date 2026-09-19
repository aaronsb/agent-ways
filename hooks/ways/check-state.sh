#!/usr/bin/env bash
# State-based trigger evaluator — thin dispatcher to ways binary
#
# Evaluates: context-threshold, file-exists, session-start triggers.
# Also handles core guidance re-injection safety net.

source "$(dirname "$0")/require-ways.sh"

INPUT=$(cat)
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty')
AGENT_ID=$(echo "$INPUT" | jq -r '.agent_id // empty')
[[ -n "$AGENT_ID" ]] && export CLAUDE_AGENT_ID="$AGENT_ID"
TRANSCRIPT=$(echo "$INPUT" | jq -r '.transcript_path // empty')
PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$(echo "$INPUT" | jq -r '.cwd // empty')}"
HOOK_EVENT=$(echo "$INPUT" | jq -r '.hook_event_name // "SessionStart"')
# On UserPromptSubmit the payload carries the prompt. The binary uses it only
# to recognise harness envelopes (Monitor notifications, task hand-backs, skill
# bodies) and skip the scan, so state-triggered ways ride operator turns only.
# Only the leading prefix travels: the predicate reads the first tag, and a
# pasted log or persisted-output blob would otherwise overflow argv (E2BIG),
# which the skew guard below would misread as an old binary.
PROMPT=$(echo "$INPUT" | jq -r '.prompt // empty' | tr '[:upper:]' '[:lower:]')
PROMPT="${PROMPT#"${PROMPT%%[![:space:]]*}"}"
PROMPT="${PROMPT:0:96}"

export CLAUDE_PROJECT_DIR="${PROJECT_DIR}"

# --opt=value form for consistency with the other scan hooks.
ARGS=(--session="$SESSION_ID" --project="$PROJECT_DIR" --hook-event="$HOOK_EVENT")
[[ -n "$TRANSCRIPT" ]] && ARGS+=(--transcript="$TRANSCRIPT")

# Deploy-order skew guard, as in check-prompt.sh: projected hooks newer than the
# installed binary would make clap reject --query with a non-zero exit, which
# UserPromptSubmit reads as "block the prompt". Retry without the flag instead.
if [[ -n "$PROMPT" ]]; then
  if OUTPUT=$("${HOME}/.claude/bin/ways" scan state "${ARGS[@]}" --query="$PROMPT" 2>/dev/null); then
    [[ -n "$OUTPUT" ]] && printf '%s\n' "$OUTPUT"
    exit 0
  fi
fi
"${HOME}/.claude/bin/ways" scan state "${ARGS[@]}"
