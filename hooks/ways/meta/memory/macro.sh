#!/usr/bin/env bash
# Check MEMORY.md state and inject context budget for the current project

PROJECT_DIR="${CLAUDE_PROJECT_DIR:-${PROJECT_DIR:-.}}"

# Context budget
if command -v ways &>/dev/null; then
  JSON=$(ways context --json 2>/dev/null)
  if [[ -n "$JSON" ]]; then
    REMAINING=$(echo "$JSON" | jq -r '.tokens_remaining')
    PCT=$(echo "$JSON" | jq -r '.pct_remaining')
    echo "**Context budget: ~${REMAINING} tokens remaining (${PCT}% of window).** After compaction, session details are summarized and specifics are lost. Per ADR-128: project knowledge belongs in ways/ADRs/notes/issues/PRs — MEMORY.md is narrow (short cross-project user facts only). Before saving a memory entry, check: could this be a way?"
    echo ""
  fi
fi

# MEMORY.md state
NORMALIZED=$(echo "$PROJECT_DIR" | sed 's|[/.]|-|g')
MEMORY_DIR="$HOME/.claude/projects/${NORMALIZED}/memory"
MEMORY_FILE="$MEMORY_DIR/MEMORY.md"

if [ ! -f "$MEMORY_FILE" ]; then
    echo "**MEMORY.md does not exist yet for this project.** Run \`ways init\` to seed it (ADR-128)."
elif [ ! -s "$MEMORY_FILE" ]; then
    echo "**MEMORY.md exists but is empty.** Run \`ways init\` to seed it (ADR-128)."
else
    LINES=$(wc -l < "$MEMORY_FILE")
    echo "**MEMORY.md has ${LINES} lines.** Review for drift against the ADR-128 seed; add only cross-project user facts under \`## User Context\`."
fi
