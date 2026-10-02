#!/usr/bin/env bash
# Dynamic context for Makefile way
# Detects Makefile in project, shows available targets, warns about bare-make safety.

PROJECT_DIR="$CLAUDE_PROJECT_DIR"

# Find Makefile (GNU make search order)
MAKEFILE=""
for candidate in GNUmakefile makefile Makefile; do
  if [[ -f "${PROJECT_DIR}/${candidate}" ]]; then
    MAKEFILE="${PROJECT_DIR}/${candidate}"
    break
  fi
done

[[ -z "$MAKEFILE" ]] && exit 0

echo "**Makefile detected**: \`${MAKEFILE##*/}\`"

# Check bare-make safety: is .DEFAULT_GOAL set to help?
HAS_DEFAULT_HELP=""
if grep -qE '^\s*\.DEFAULT_GOAL\s*:?=\s*help' "$MAKEFILE" 2>/dev/null; then
  HAS_DEFAULT_HELP="yes"
fi

if [[ -z "$HAS_DEFAULT_HELP" ]]; then
  # Check if first target is 'help'
  FIRST_TARGET=$(awk '/^[a-zA-Z_-]+:/ { sub(/:.*/, ""); print; exit }' "$MAKEFILE" 2>/dev/null)
  if [[ "$FIRST_TARGET" == "help" ]]; then
    HAS_DEFAULT_HELP="yes"
  fi
fi

if [[ -z "$HAS_DEFAULT_HELP" ]]; then
  echo ""
  echo "**Fix required**: This Makefile has no safe default goal. Bare \`make\` may trigger a build."
  echo "Add this to the top of the Makefile (before any targets):"
  echo '```makefile'
  echo '.DEFAULT_GOAL := help'
  echo ''
  echo 'help: ## Show this help'
  echo '	@grep -E '"'"'^[a-zA-Z_-]+:.*?## .*$$'"'"' $(MAKEFILE_LIST) | \'
  echo '		awk '"'"'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2}'"'"
  echo '```'
  echo "Then annotate each target with \`## description\` comments."
  echo "**Do this now before running any make commands.**"
fi

# Try to get targets from make help (timeout quickly, with macOS fallback)
if command -v timeout >/dev/null 2>&1; then
  HELP_OUTPUT=$(cd "$PROJECT_DIR" && timeout 3 make --no-print-directory help 2>/dev/null)
else
  HELP_OUTPUT=$(cd "$PROJECT_DIR" && make --no-print-directory help 2>/dev/null)
fi

if [[ -n "$HELP_OUTPUT" && ${#HELP_OUTPUT} -lt 2000 ]]; then
  echo ""
  echo "**Available targets** (\`make help\`):"
  echo '```'
  echo "$HELP_OUTPUT"
  echo '```'
else
  # Fallback: parse .PHONY and target names from the Makefile
  TARGETS=$(awk '
    /^\.PHONY:/ { gsub(/^\.PHONY:[ \t]*/, ""); print; next }
    /^[a-zA-Z_][a-zA-Z0-9_-]*:/ && !/^\t/ {
      sub(/:.*/, "")
      if ($0 != ".DEFAULT_GOAL") print
    }
  ' "$MAKEFILE" 2>/dev/null | tr ' ' '\n' | sort -u | tr '\n' ' ')

  if [[ -n "$TARGETS" ]]; then
    echo ""
    echo "**Available targets**: \`${TARGETS% }\`"
  fi
fi

# Show target-to-raw-command mapping for the check to reference
# This helps Claude know which make target replaces which raw command
echo ""
echo "**Use \`make <target>\` instead of raw commands.** The Makefile is the canonical interface for this project."
