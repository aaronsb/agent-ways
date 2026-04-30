#!/bin/bash
# Stop hook: Analyze Claude's response for topic awareness
#
# Reads the transcript after Claude responds, extracts topics,
# and writes state for the next UserPromptSubmit to use.
#
# This enables ways to trigger based on what Claude discussed,
# not just what the user asked.

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
# Use tail instead of tac to avoid reading entire file
LAST_RESPONSE=$(tail -100 "$TRANSCRIPT" | grep '"type":"assistant"' | tail -1 | jq -r '.message.content[]?.text // empty' 2>/dev/null | head -c 2000)

[[ -z "$LAST_RESPONSE" ]] && exit 0

# Extract potential topics (simple keyword extraction)
# Look for: capitalized terms, technical words, repeated nouns
TOPICS=$(echo "$LAST_RESPONSE" | tr '[:upper:]' '[:lower:]' | \
  grep -oE '\b(api|test|debug|config|security|auth|database|migration|deploy|git|commit|pr|issue|error|hook|trigger|way|todo|context|token|model|prompt)\b' | \
  sort | uniq -c | sort -rn | head -10 | awk '{print $2}' | tr '\n' ' ')

# Write state for next turn
if [[ -n "$TOPICS" ]]; then
  cat > "$STATE_FILE" << EOF
{
  "timestamp": "$(date -Iseconds)",
  "topics": "$TOPICS",
  "response_length": ${#LAST_RESPONSE}
}
EOF
fi

# Stop hooks can output but it doesn't inject into next turn
# This is just for logging/debugging
# echo "Topics detected: $TOPICS"
