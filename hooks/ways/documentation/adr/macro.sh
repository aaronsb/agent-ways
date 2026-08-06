#!/usr/bin/env bash
# ADR way macro — tri-state detection of ADR tooling in project
#
# States:
#   declined  → .claude/no-adr-tooling exists → one-liner, stop nagging
#   installed → docs/scripts/adr (or similar) found → command reference
#   available → neither → suggest installation

PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$PWD}"

# State 1: Declined
if [[ -f "$PROJECT_DIR/.claude/no-adr-tooling" ]]; then
  echo "ADR tooling declined for this project. Remove \`.claude/no-adr-tooling\` to enable."
  exit 0
fi

# State 2: Installed — check common locations
ADR_SCRIPT=""
for path in "docs/scripts/adr" "scripts/adr" "tools/adr"; do
  if [[ -x "$PROJECT_DIR/$path" ]]; then
    ADR_SCRIPT="$path"
    break
  fi
done

if [[ -n "$ADR_SCRIPT" ]]; then
  echo "## ADR Tooling"
  echo ""
  echo "Use \`$ADR_SCRIPT\` for ADR management:"
  echo ""
  echo "| Command | Purpose |"
  echo "|---------|---------|"
  echo "| \`$ADR_SCRIPT new <domain> <title>\` | Create new ADR |"
  echo "| \`$ADR_SCRIPT list [--group]\` | List all ADRs |"
  echo "| \`$ADR_SCRIPT view <number>\` | View an ADR |"
  echo "| \`$ADR_SCRIPT lint [--check]\` | Validate ADRs |"
  echo "| \`$ADR_SCRIPT index -y\` | Regenerate index |"
  echo "| \`$ADR_SCRIPT domains\` | Show domain series |"
  echo ""
  echo "**Always use \`$ADR_SCRIPT new\` to create ADRs** — it handles numbering, domain routing, and templates."

  # Direction-aware drift check against the universal template (ADR-177):
  # compare TOOL_VERSION stamps to tell stale from customized from ahead.
  UNIVERSAL="${HOME}/.claude/hooks/ways/documentation/adr/adr-tool"
  if [[ -f "$UNIVERSAL" ]]; then
    # Capture is shape-restricted: a stamp that isn't a plain version string is
    # treated as unversioned rather than echoed into disclosed context.
    ver_re='^TOOL_VERSION = "\K[0-9]+(\.[0-9]+)*(-[0-9A-Za-z.]+)?(?=")'
    local_ver=$(grep -m1 -oP "$ver_re" "$PROJECT_DIR/$ADR_SCRIPT" 2>/dev/null || true)
    univ_ver=$(grep -m1 -oP "$ver_re" "$UNIVERSAL" 2>/dev/null || true)
    if [[ -z "$local_ver" && -n "$univ_ver" ]]; then
      echo ""
      echo "_The project's copy predates tool versioning (ways ships v${univ_ver}) — it is out of date. Re-vendor via the \`adr\` skill; if it was customized, diff first and carry the changes forward._"
    elif [[ -n "$local_ver" && -z "$univ_ver" ]]; then
      echo ""
      echo "_The project's copy is v${local_ver} but the installed template is unversioned — the agent-ways install is stale. Update it (\`/ways-update\`)._"
    elif [[ -n "$local_ver" && -n "$univ_ver" && "$local_ver" != "$univ_ver" ]]; then
      newest=$(printf '%s\n%s\n' "$local_ver" "$univ_ver" | sort -V | tail -1)
      if [[ "$newest" == "$univ_ver" ]]; then
        echo ""
        echo "_The project's copy is v${local_ver}; ways ships v${univ_ver} — out of date. Re-vendor via the \`adr\` skill; if it was customized, diff first and carry the changes forward._"
      else
        echo ""
        echo "_The project's copy is v${local_ver}, ahead of the installed template (v${univ_ver}) — the agent-ways install is stale. Update it (\`/ways-update\`)._"
      fi
    elif ! diff -q <(grep -v '^TOOL_VERSION = ' "$PROJECT_DIR/$ADR_SCRIPT") <(grep -v '^TOOL_VERSION = ' "$UNIVERSAL") &>/dev/null; then
      echo ""
      echo "_Note: Project script differs from the universal template. This is expected for customized setups._"
    fi
  fi
  exit 0
fi

# State 3: Not installed
echo "## ADR Tooling Available"
echo ""
echo "This project doesn't have ADR management tooling installed."
echo "A script-based system is available that provides:"
echo "- Automatic numbering by domain"
echo "- Template generation with frontmatter"
echo "- Linting and validation"
echo "- Index generation"
echo ""
echo "To vendor it, use the \`adr\` skill — it carries the install steps and \`adr.yaml\` setup. Or run \`/project-init\` to scaffold it alongside the rest of the repo."
echo ""
echo "To decline permanently: \`mkdir -p .claude && touch .claude/no-adr-tooling\`"
