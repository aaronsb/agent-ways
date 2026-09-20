#!/usr/bin/env bash
# PreToolUse: Check bash commands against ways — thin dispatcher
#
# The ways binary handles: command pattern matching, semantic scoring,
# check curve scoring, session state, and content output.
#
# Size bounding for the embed query is the ways binary's responsibility
# (ADR-130 sentence-salience reducer in scan/reduce.rs). This script
# passes the full command through so the reducer can score the prose
# distribution itself — pre-truncating here would starve the reducer
# of the back half of any long input. The regex `commands:` matcher
# also gets the full command, which is what ways with patterns like
# `^(npm|cargo|gh) ` expect.

source "$(dirname "$0")/require-ways.sh"

INPUT=$(cat)
CMD=$(echo "$INPUT" | jq -r '.tool_input.command // empty')
DESC=$(echo "$INPUT" | jq -r '.tool_input.description // empty' | tr '[:upper:]' '[:lower:]')
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty')
AGENT_ID=$(echo "$INPUT" | jq -r '.agent_id // empty')
[[ -n "$AGENT_ID" ]] && export CLAUDE_AGENT_ID="$AGENT_ID"
PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$(echo "$INPUT" | jq -r '.cwd // empty')}"
# The invoking agent's transcript: the binary stamps each fired way with the
# session's model id read from it (see check-prompt.sh).
TRANSCRIPT=$(echo "$INPUT" | jq -r '.transcript_path // empty')

export CLAUDE_PROJECT_DIR="${PROJECT_DIR}"
# --opt=value form: $CMD/$DESC are free text that may begin with '-'; the space
# form would make clap parse the value as a flag. The = form binds it safely.
ARGS=(--command="$CMD" --description="$DESC" --session="$SESSION_ID" --project="$PROJECT_DIR")

# Deploy-order skew guard, as in check-prompt.sh: projected hooks newer than
# the installed binary make clap reject --transcript with a non-zero exit.
# Retry without the flag rather than surfacing a usage error on every call.
if [[ -n "$TRANSCRIPT" ]]; then
  if OUTPUT=$("${HOME}/.claude/bin/ways" scan command "${ARGS[@]}" --transcript="$TRANSCRIPT" 2>/dev/null); then
    [[ -n "$OUTPUT" ]] && printf '%s\n' "$OUTPUT"
    exit 0
  fi
fi
"${HOME}/.claude/bin/ways" scan command "${ARGS[@]}"
