#!/usr/bin/env bash
# Stop hook: capture Claude's response for topic awareness (ADR-155 §3)
#
# Reads the transcript after Claude responds and stores the last assistant
# message RAW for the next UserPromptSubmit. No extraction happens here —
# the scan-time sentence-salience reducer (ADR-130) selects what matters,
# and the stored text feeds only the embed lane, never the keyword regex
# lane. This replaced a 24-word grep whitelist that both missed Claude's
# actual reasoning and leaked way-trigger tokens ("way", "pr", "commit")
# into the next prompt's keyword matching.

INPUT=$(cat)
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty')
AGENT_ID=$(echo "$INPUT" | jq -r '.agent_id // empty')
[[ -n "$AGENT_ID" ]] && export CLAUDE_AGENT_ID="$AGENT_ID"
TRANSCRIPT=$(echo "$INPUT" | jq -r '.transcript_path // empty')
STOP_ACTIVE=$(echo "$INPUT" | jq -r '.stop_hook_active // false')

# Prevent infinite loops
[[ "$STOP_ACTIVE" == "true" ]] && exit 0

# Need transcript
[[ ! -f "$TRANSCRIPT" ]] && exit 0

# Path resolves through the binary so this writer, the consumer
# (check-prompt.sh), and `ways reset` cannot drift.
STATE_FILE=$("${HOME}/.claude/bin/ways" response-topics-path "$SESSION_ID")

# Extract last assistant message from transcript (JSONL format)
# Use tail instead of tac to avoid reading entire file. 2000 chars is
# plenty: the reducer's budget is ~110 tokens, and the response competes
# with the user's prompt on salience anyway.
LAST_RESPONSE=$(tail -100 "$TRANSCRIPT" | grep '"type":"assistant"' | tail -1 | jq -r '.message.content[]?.text // empty' 2>/dev/null | head -c 2000)

[[ -z "$LAST_RESPONSE" ]] && exit 0

# Write state for next turn. jq -n builds the JSON so quotes/newlines in
# the response can't break the document (the old heredoc could).
jq -n \
  --arg ts "$(date -Iseconds)" \
  --arg ctx "$LAST_RESPONSE" \
  '{timestamp: $ts, context: $ctx, response_length: ($ctx | length)}' \
  > "$STATE_FILE"
