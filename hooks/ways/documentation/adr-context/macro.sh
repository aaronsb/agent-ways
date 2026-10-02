#!/usr/bin/env bash
# ADR context macro — detect project ADR state and provide actionable commands
#
# Outputs nothing if the project has no ADRs (way content still shows as a reminder,
# but without concrete commands it stays lightweight).

PROJECT_DIR="$CLAUDE_PROJECT_DIR"

# `ways` exports the project's ADR tool path when it has one.
ADR_SCRIPT="${WAYS_ADR_TOOL:-}"

if [[ -n "$ADR_SCRIPT" ]]; then
  # Tooling installed — show quick-reference
  echo "**ADR tool available**: \`$ADR_SCRIPT\`"
  echo ""
  echo "| Discover | Read |"
  echo "|----------|------|"
  echo "| \`$ADR_SCRIPT list --group\` | \`$ADR_SCRIPT view <N>\` |"
  echo "| \`$ADR_SCRIPT domains\` | \`$ADR_SCRIPT view <N>\` with Read tool |"
else
  # No tool — check for ADR files directly
  ADR_COUNT=$(find "$PROJECT_DIR" -name "ADR-*.md" 2>/dev/null | wc -l)
  if [[ "$ADR_COUNT" -gt 0 ]]; then
    echo "**$ADR_COUNT ADR files found** (no ADR tool — read files directly from \`docs/architecture/\`)"
  fi
  # If no ADRs at all, output nothing — the way file still provides the general reminder
fi
