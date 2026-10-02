#!/usr/bin/env bash
# Macro: prepend way vocabulary health summary when optimization way fires
# Runs ways author suggest on all semantic ways and outputs a compact summary

if ! command -v ways &>/dev/null; then
  echo "**ways binary not found** — build with: \`make setup\` in the agent-ways app dir (see docs/install-guide.md)"
  exit 0
fi

echo "## Current Way Health"
echo ""
printf "%-35s %6s %6s %6s %s\n" "Way" "Gaps" "Cover" "Unused" "Match"
printf "%-35s %6s %6s %6s %s\n" "---" "----" "-----" "------" "-----"

for wayfile in $(find -L "${HOME}/.claude/hooks/ways" -name "*.md" ! -name "*.check.md" -print 2>/dev/null | while IFS= read -r f; do head -1 "$f" 2>/dev/null | grep -q '^---$' && echo "$f"; done | sort); do
  relpath="${wayfile#${HOME}/.claude/hooks/ways/}"
  relpath="${relpath%/*}"

  # Check if this way has vocabulary (semantic matching)
  has_vocab=$(awk 'NR==1 && /^---$/{p=1;next} p&&/^---$/{exit} p && /^vocabulary:/{print "yes";exit}' "$wayfile")
  has_desc=$(awk 'NR==1 && /^---$/{p=1;next} p&&/^---$/{exit} p && /^description:/{print "yes";exit}' "$wayfile")

  if [[ "$has_vocab" == "yes" && "$has_desc" == "yes" ]]; then
    match_type="embed"
  elif [[ "$has_desc" == "yes" ]]; then
    match_type="desc"
  else
    # Detect match type from frontmatter
    match_type=$(awk 'NR==1 && /^---$/{p=1;next} p&&/^---$/{exit} p && /^pattern:/{print "regex";exit} p && /^files:/{print "file";exit} p && /^commands:/{print "cmd";exit} p && /^trigger:/{print "state";exit}' "$wayfile")
    match_type="${match_type:-regex}"
  fi

  if [[ "$match_type" == "embed" ]]; then
    stderr=$(ways author suggest "$wayfile" 2>&1 >/dev/null)
    gaps=$(echo "$stderr" | sed -n 's/suggest: \([0-9]*\) gaps.*/\1/p')
    covered=$(echo "$stderr" | sed -n 's/.*, \([0-9]*\) covered.*/\1/p')
    unused=$(echo "$stderr" | sed -n 's/.*, \([0-9]*\) unused/\1/p')
    printf "%-35s %6s %6s %6s %s\n" "$relpath" "${gaps:-0}" "${covered:-0}" "${unused:-0}" "$match_type"
  else
    printf "%-35s %6s %6s %6s %s\n" "$relpath" "-" "-" "-" "$match_type"
  fi
done

# Check project-local ways too
PROJECT_DIR="$CLAUDE_PROJECT_DIR"
if [[ -d "$PROJECT_DIR/.claude/ways" ]]; then
  echo ""
  echo "### Project-local ways"
  for wayfile in $(find -L "$PROJECT_DIR/.claude/ways" -name "*.md" ! -name "*.check.md" -print 2>/dev/null | while IFS= read -r f; do head -1 "$f" 2>/dev/null | grep -q '^---$' && echo "$f"; done | sort); do
    relpath="${wayfile#$PROJECT_DIR/.claude/ways/}"
    relpath="${relpath%/*}"
    printf "%-35s %s\n" "$relpath" "(project-local)"
  done
fi
