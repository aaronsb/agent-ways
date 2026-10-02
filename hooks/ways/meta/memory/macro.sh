#!/usr/bin/env bash
# Check MEMORY.md state and inject context budget for the current project

PROJECT_DIR="$CLAUDE_PROJECT_DIR"

# Context budget: `ways` exports WAYS_CONTEXT_* to a macro that names them.
if [[ -n "${WAYS_CONTEXT_REMAINING:-}" ]]; then
    echo "**Context budget: ~${WAYS_CONTEXT_REMAINING} tokens remaining (${WAYS_CONTEXT_PCT_REMAINING}% of window).** After compaction, session details are summarized and specifics are lost. Per ADR-128: project knowledge belongs in ways/ADRs/notes/issues/PRs — MEMORY.md is narrow (short cross-project user facts only). Before saving a memory entry, check: could this be a way?"
    echo ""
fi

# MEMORY.md state, under the dir Claude Code names the project by
# (`ways project-slug` is the one copy of that rule).
WAYS_BIN=$(command -v ways || echo "$HOME/.claude/bin/ways")
NORMALIZED=$("$WAYS_BIN" project-slug "$PROJECT_DIR" 2>/dev/null)
if [[ -z "$NORMALIZED" ]]; then
    echo "**MEMORY.md state unknown: \`ways project-slug\` is unavailable.**"
    exit 0
fi
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
