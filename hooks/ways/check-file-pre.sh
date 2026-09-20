#!/usr/bin/env bash
# PreToolUse: Check file operations against ways — thin dispatcher
#
# The ways binary handles: file pattern matching, check scoring,
# session state, and content output.

source "$(dirname "$0")/require-ways.sh"

INPUT=$(cat)
FP=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty')
AGENT_ID=$(echo "$INPUT" | jq -r '.agent_id // empty')
[[ -n "$AGENT_ID" ]] && export CLAUDE_AGENT_ID="$AGENT_ID"
PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$(echo "$INPUT" | jq -r '.cwd // empty')}"
# The invoking agent's transcript: the binary stamps each fired way with the
# session's model id read from it (see check-prompt.sh).
TRANSCRIPT=$(echo "$INPUT" | jq -r '.transcript_path // empty')

[[ -z "$FP" ]] && exit 0

export CLAUDE_PROJECT_DIR="${PROJECT_DIR}"
# --opt=value form for consistency with the other scan hooks (binds values that
# could begin with '-' unambiguously).
ARGS=(--path="$FP" --session="$SESSION_ID" --project="$PROJECT_DIR")

# Deploy-order skew guard, as in check-prompt.sh: projected hooks newer than
# the installed binary make clap reject --transcript with a usage error
# (exit 2), which PreToolUse would read as "block the tool". Retry without
# the flag on exit 2 only: any other status means the scan ran (and stamped
# its ways), so it is passed through as-is, stderr included, rather than
# re-run.
if [[ -n "$TRANSCRIPT" ]]; then
  ERR=$(mktemp)
  OUTPUT=$("${HOME}/.claude/bin/ways" scan file "${ARGS[@]}" --transcript="$TRANSCRIPT" 2>"$ERR")
  STATUS=$?
  if [[ $STATUS -ne 2 ]]; then
    cat "$ERR" >&2
    rm -f "$ERR"
    [[ -n "$OUTPUT" ]] && printf '%s\n' "$OUTPUT"
    exit $STATUS
  fi
  rm -f "$ERR"
fi
"${HOME}/.claude/bin/ways" scan file "${ARGS[@]}"
