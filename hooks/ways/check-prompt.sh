#!/usr/bin/env bash
# Check user prompts against ways — thin dispatcher to ways binary
#
# The ways binary handles: file walking, frontmatter extraction, pattern
# + semantic matching, scope/precondition gating, parent threshold
# lowering, session markers, macro dispatch, and content output.
#
# UserPromptSubmit doesn't just carry the user's typed message — the
# harness also injects structured content here: <task-notification>
# blobs from completed background agents, <persisted-output> pointers
# for tool results that exceed inline budget, and other system-reminder
# envelopes. Size bounding for the embed query is the ways binary's
# responsibility (ADR-130 sentence-salience reducer in scan/reduce.rs).
# This script passes the full prompt through so the reducer can score
# sentence salience across the whole input.

source "$(dirname "$0")/require-ways.sh"

INPUT=$(cat)
PROMPT=$(echo "$INPUT" | jq -r '.prompt // empty' | tr '[:upper:]' '[:lower:]')
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty')
PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$(echo "$INPUT" | jq -r '.cwd // empty')}"
AGENT_ID=$(echo "$INPUT" | jq -r '.agent_id // empty')
[[ -n "$AGENT_ID" ]] && export CLAUDE_AGENT_ID="$AGENT_ID"
# The invoking agent's transcript. The binary reads the session's model id
# from it and stamps every fired way with it (`model` on the event), and
# resolves the refire window from the same read.
TRANSCRIPT=$(echo "$INPUT" | jq -r '.transcript_path // empty')

# Read Claude's last response from the Stop hook state (if available).
# Path resolves through the binary so the writer (check-response.sh),
# the consumer (here), and `ways reset` cannot drift. `.topics` is the
# pre-ADR-155 field name — read as fallback so one stale state file
# across an upgrade degrades gracefully instead of vanishing.
RESPONSE_STATE=$("${HOME}/.claude/bin/ways" response-topics-path "$SESSION_ID")
RESPONSE_CONTEXT=""
if [[ -f "$RESPONSE_STATE" ]]; then
  RESPONSE_CONTEXT=$(jq -r '.context // .topics // empty' "$RESPONSE_STATE" 2>/dev/null)
fi

export CLAUDE_PROJECT_DIR="${PROJECT_DIR}"
# The prompt and the response context ride separate flags (ADR-155 §3):
# the binary keyword-matches only the prompt, and embeds both. Response
# tokens can no longer keyword-fire ways the user never mentioned.
# Use --opt=value (not --opt value): a prompt may begin with '-' (e.g. the user
# pastes "-spawn ..."), and the space form makes clap parse that as a flag.
# The = form binds the value unambiguously even when it starts with a dash.
#
# Deploy-order skew guard: if the projected hooks are newer than the
# installed binary (reconcile ran before a rebuild), clap rejects an
# unknown flag with a usage error (exit 2) — which the UserPromptSubmit
# contract reads as "block the prompt". Shed the newest flag first and
# retry, so a binary that knows --response-context but not --transcript
# keeps the response-context lane; only a binary that knows neither gets
# the flagless pre-ADR-155 invocation. Only exit 2 retries: any other
# failure has already run the scan (and stamped its ways), so re-running it
# would suppress them and hide the first attempt's stderr.
scan_prompt() {
  "${HOME}/.claude/bin/ways" scan prompt \
    --query="$PROMPT" \
    --session="$SESSION_ID" \
    --project="$PROJECT_DIR" \
    "$@"
}
FLAGS=(--response-context="$RESPONSE_CONTEXT")
[[ -n "$TRANSCRIPT" ]] && FLAGS+=(--transcript="$TRANSCRIPT")
OUTPUT=$(scan_prompt "${FLAGS[@]}" 2>/dev/null)
STATUS=$?
if [[ $STATUS -eq 2 && ${#FLAGS[@]} -gt 1 ]]; then
  OUTPUT=$(scan_prompt "${FLAGS[0]}" 2>/dev/null)
  STATUS=$?
fi
if [[ $STATUS -eq 2 ]]; then
  OUTPUT=$(scan_prompt)
fi
[[ -n "$OUTPUT" ]] && printf '%s\n' "$OUTPUT"
exit 0
