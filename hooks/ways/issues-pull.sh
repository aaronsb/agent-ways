#!/usr/bin/env bash
# SessionStart / UserPromptSubmit / PostToolUse(Bash gh issue) — pull GitHub
# issues carrying the `tasklist` label into the session task store and whisper
# what changed (ADR-180).
#
# Usage in settings.json: issues-pull.sh <session-start|prompt|post-gh>
#   session-start  forced pull, then the full open list
#   prompt         pull if the snapshot is older than GH_TASKS_TTL, then deltas
#   post-gh        forced pull after an in-session `gh issue` command, then deltas
#
# SessionStart and UserPromptSubmit accept plain stdout as context.
# PostToolUse needs hookSpecificOutput.additionalContext.

MODE="${1:-prompt}"
GH_TASKS="$(dirname "$0")/softwaredev/delivery/issues/gh-tasks"
[[ -x "$GH_TASKS" ]] || exit 0
command -v gh >/dev/null 2>&1 && command -v jq >/dev/null 2>&1 || exit 0

INPUT=$(cat)
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty')
[[ -n "$SESSION_ID" ]] || exit 0
CWD=$(echo "$INPUT" | jq -r '.cwd // empty')
[[ -n "$CWD" && -d "$CWD" ]] && cd "$CWD"

# No repo, no gh call.
git rev-parse --is-inside-work-tree >/dev/null 2>&1 || exit 0

case "$MODE" in
  session-start)
    "$GH_TASKS" --session "$SESSION_ID" --force pull 2>/dev/null
    OUT=$("$GH_TASKS" --session "$SESSION_ID" whisper --full 2>/dev/null)
    ;;
  post-gh)
    "$GH_TASKS" --session "$SESSION_ID" --force pull 2>/dev/null
    OUT=$("$GH_TASKS" --session "$SESSION_ID" whisper 2>/dev/null)
    ;;
  *)
    "$GH_TASKS" --session "$SESSION_ID" pull 2>/dev/null
    OUT=$("$GH_TASKS" --session "$SESSION_ID" whisper 2>/dev/null)
    ;;
esac

[[ -n "$OUT" ]] || exit 0

if [[ "$MODE" == "post-gh" ]]; then
  jq -n --arg ctx "$OUT" '{hookSpecificOutput: {hookEventName: "PostToolUse", additionalContext: $ctx}}'
else
  printf '%s\n' "$OUT"
fi
exit 0
