#!/usr/bin/env bash
# PostToolUse: scan queued mid-turn operator messages (ADR-161).
#
# A message the operator types while the agent is working is queued and
# flushed into the running turn at the next LLM pause — it never fires
# UserPromptSubmit, so check-prompt.sh never scans it. Those messages are
# recorded in the transcript as `queue-operation`/`enqueue` entries. Here,
# on PostToolUse, `ways scan messages` reads the transcript, aggregates
# every enqueue newer than the per-session scan mark into one surface
# (so a burst of short fragments is a >=2-chunk late-interaction surface,
# not a single-vector-fallback fragment), matches it through the same
# engine as a prompt, and advances the mark.
#
# Emits its own PostToolUse hookSpecificOutput envelope, independent of
# check-post.sh's reactive postcheck dispatch (both run on PostToolUse).

source "$(dirname "$0")/require-ways.sh"

INPUT=$(cat)
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty')
[[ -z "$SESSION_ID" ]] && exit 0

PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$(echo "$INPUT" | jq -r '.cwd // empty')}"
TRANSCRIPT=$(echo "$INPUT" | jq -r '.transcript_path // empty')
AGENT_ID=$(echo "$INPUT" | jq -r '.agent_id // empty')
[[ -n "$AGENT_ID" ]] && export CLAUDE_AGENT_ID="$AGENT_ID"

# Without a transcript path there is nothing to read; exit quietly.
[[ -z "$TRANSCRIPT" ]] && exit 0

export CLAUDE_PROJECT_DIR="${PROJECT_DIR}"

ARGS=(--session="$SESSION_ID" --transcript="$TRANSCRIPT")
[[ -n "$PROJECT_DIR" ]] && ARGS+=(--project="$PROJECT_DIR")

OUTPUT=$("${HOME}/.claude/bin/ways" scan messages "${ARGS[@]}" 2>/dev/null) || exit 0
[[ -n "$OUTPUT" ]] && printf '%s\n' "$OUTPUT"
exit 0
