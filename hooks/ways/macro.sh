#!/usr/bin/env bash
# Core macro — generates the available ways table for session start.
# This is the archetype macro: other macros follow this pattern.
#
# A macro is a shell script referenced by a way's `macro: prepend` field.
# Its stdout is prepended to the way content before injection.
# Keep macros fast (no network, no heavy computation).

WAYS_DIR="${HOME}/.claude/hooks/ways"

# ── Skills context cost ────────────────────────────────────────
# Skills front-load instructions into early context (ROPE position 0-N).
# Too many degrades retrieval and instruction-following.

skill_count=0
if command -v claude >/dev/null 2>&1; then
  skill_count=$(claude plugin list 2>/dev/null | grep -c '✔ enabled' || echo 0)
fi

if [[ "$skill_count" -gt 12 ]]; then
  echo "Skills loaded: ${skill_count} — **HIGH context cost.** Tell the user: \"You have ${skill_count} skills loaded. Each adds instructions to early context, degrading response quality. Run \`claude plugin list\` and disable unused ones. Aim for ≤5.\""
  echo ""
elif [[ "$skill_count" -gt 5 ]]; then
  echo "Skills loaded: ${skill_count} — moderate context cost. Suggest reviewing with \`claude plugin list\`."
  echo ""
fi

# ── Available ways table ───────────────────────────────────────

echo "## Available Ways"
echo ""

current_domain=""

while IFS= read -r wayfile; do
  relpath="${wayfile#$WAYS_DIR/}"
  relpath="${relpath%/*}"

  # Skip files not in a domain/way subdirectory
  [[ "$relpath" != */* ]] && continue

  domain="${relpath%%/*}"
  wayname="${relpath#*/}"
  wayname="${wayname//\// > }"

  # Domain header
  if [[ "$domain" != "$current_domain" ]]; then
    domain_display="$(echo "${domain:0:1}" | tr '[:lower:]' '[:upper:]')${domain:1}"
    echo "### ${domain_display}"
    echo ""
    echo "| Way | Tool Trigger | Keyword Trigger |"
    echo "|-----|--------------|-----------------|"
    current_domain="$domain"
  fi

  # Parse frontmatter (first YAML block only)
  frontmatter=$(awk 'NR==1 && /^---$/{p=1; next} p && /^---$/{exit} p{print}' "$wayfile")

  get_field() { echo "$frontmatter" | awk -v f="$1" '$0 ~ "^"f":"{gsub("^"f": *", ""); print}'; }

  match_type=$(get_field match)
  pattern=$(get_field pattern)
  commands=$(get_field commands)
  files=$(get_field files)

  # Tool trigger column
  tool_trigger="—"
  if [[ -n "$commands" ]]; then
    cmd_clean="${commands//\\}"
    case "$cmd_clean" in
      *"git commit"*)                     tool_trigger="Run \`git commit\`" ;;
      *"^gh"*|*"gh "*)                    tool_trigger="Run \`gh\`" ;;
      *"ssh"*|*"scp"*|*"rsync"*)          tool_trigger="Run \`ssh/scp/rsync\`" ;;
      *"pytest"*|*"jest"*)                tool_trigger="Run test runner" ;;
      *"npm install"*|*"pip install"*)    tool_trigger="Run package install" ;;
      *"git apply"*)                      tool_trigger="Run \`git apply\`" ;;
      *)                                  tool_trigger="Run command" ;;
    esac
  elif [[ -n "$files" ]]; then
    case "$files" in
      *"docs/adr"*)       tool_trigger="Edit \`docs/adr/*.md\`" ;;
      *"\.env"*)          tool_trigger="Edit \`.env\`" ;;
      *"\.patch"*)        tool_trigger="Edit \`*.patch\`" ;;
      *"todo-"*)          tool_trigger="Edit \`.claude/todo-*.md\`" ;;
      *"ways/"*)          tool_trigger="Edit \`.claude/ways/*.md\`" ;;
      *"README"*)         tool_trigger="Edit \`README.md\`" ;;
      *)                  tool_trigger="Edit files" ;;
    esac
  fi

  # Keyword trigger column
  keyword_display="—"
  if [[ "$match_type" == "semantic" || "$match_type" == "model" ]]; then
    keyword_display="_(${match_type})_"
  elif [[ -n "$pattern" ]]; then
    # Strip regex syntax to show human-readable keywords
    keyword_display=$(echo "$pattern" \
      | sed 's/[.][?+*]/ /g' \
      | sed 's/\\[bnrst]//g; s/\\//g; s/[?^$]//g; s/[()]/ /g; s/|/, /g; s/[][]//g' \
      | sed 's/  */ /g; s/ *, */,/g; s/,,*/,/g; s/^,//; s/,$//; s/,/, /g' \
      | awk -F', ' '{for(i=1;i<=NF;i++) if(!seen[$i]++) printf "%s%s",(i>1?", ":""),$i; print ""}')
  fi

  echo "| **${wayname}** | ${tool_trigger} | ${keyword_display} |"

done < <(find -L "$WAYS_DIR" -path "*/*/*.md" ! -name "*.check.md" ! -name "*.yaml" -type f | sort)

echo ""
echo "Project-local ways: \`\$PROJECT/.claude/ways/{domain}/{way}/{way}.md\` override global."

# ── AGENTS.md migration notice ─────────────────────────────────
# AGENTS.md front-loads all instructions at once. Ways decompose guidance
# into targeted fragments that fire once per session when relevant.

PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$PWD}"

if [[ -n "$PROJECT_DIR" && "$PROJECT_DIR" != "$HOME" && -d "$PROJECT_DIR" \
      && ! -f "$PROJECT_DIR/.claude/no-agents-migration" ]]; then
  agents_files=()
  while IFS= read -r f; do
    agents_files+=("$f")
  done < <(find "$PROJECT_DIR" -maxdepth 3 -name "AGENTS.md" -type f 2>/dev/null | sort)

  if [[ ${#agents_files[@]} -gt 0 ]]; then
    echo ""
    echo "## AGENTS.md Detected"
    echo ""
    echo "Found ${#agents_files[@]} AGENTS.md file(s):"
    echo ""
    for f in "${agents_files[@]}"; do
      relpath="${f#$PROJECT_DIR/}"
      linecount=$(wc -l < "$f" 2>/dev/null | tr -d ' ')
      echo "- \`${relpath}\` (${linecount} lines)"
    done
    echo ""
    echo "**Ways are already active** — this table was generated by the framework."
    echo "AGENTS.md front-loads all instructions into context at once, which degrades"
    echo "performance as context grows. Ways fire once per session, only when relevant."
    echo ""
    echo "**Read the AGENTS.md file(s) above**, then ask the user:"
    echo "1. **Migrate** — decompose into project-scoped ways (\`.claude/ways/\`)"
    echo "2. **Keep** — leave untouched (may duplicate/conflict with ways)"
    echo "3. **Decline** — create \`.claude/no-agents-migration\` to suppress this notice"
  fi
fi
