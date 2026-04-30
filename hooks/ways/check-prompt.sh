#!/bin/bash
# Check user prompts against ways — thin dispatcher to ways binary
#
# The ways binary handles: file walking, frontmatter extraction, pattern
# + semantic matching, scope/precondition gating, parent threshold
# lowering, session markers, macro dispatch, and content output.

source "$(dirname "$0")/require-ways.sh"

INPUT=$(cat)
PROMPT=$(echo "$INPUT" | jq -r '.prompt // empty' | tr '[:upper:]' '[:lower:]')
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty')
PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$(echo "$INPUT" | jq -r '.cwd // empty')}"
AGENT_ID=$(echo "$INPUT" | jq -r '.agent_id // empty')
[[ -n "$AGENT_ID" ]] && export CLAUDE_AGENT_ID="$AGENT_ID"

# Read response topics from Stop hook (if available).
# Path resolves through the binary so the writer (check-response.sh),
# the consumer (here), and `ways reset` cannot drift.
RESPONSE_STATE=$("${HOME}/.claude/bin/ways" response-topics-path "$SESSION_ID")
RESPONSE_TOPICS=""
if [[ -f "$RESPONSE_STATE" ]]; then
  RESPONSE_TOPICS=$(jq -r '.topics // empty' "$RESPONSE_STATE" 2>/dev/null)
fi

# Combined context: user prompt + Claude's recent topics
COMBINED="${PROMPT} ${RESPONSE_TOPICS}"

export CLAUDE_PROJECT_DIR="${PROJECT_DIR}"
"${HOME}/.claude/bin/ways" scan prompt \
  --query "$COMBINED" \
  --session "$SESSION_ID" \
  --project "$PROJECT_DIR"
