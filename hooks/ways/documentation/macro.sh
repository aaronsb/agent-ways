#!/usr/bin/env bash
# Documentation catalog tooling macro — tri-state detection of doc/doclint tooling.
#
# States:
#   declined  → .claude/no-doc-tooling exists → one-liner, stop nagging
#   installed → docs/scripts/doc found → command reference
#   available → neither → suggest installation (copy, not symlink)
#
# Mirrors documentation/adr/macro.sh. The sentinel is separate from
# .claude/no-adr-tooling so a repo can adopt ADRs but not the doc catalog
# (or vice versa).

PROJECT_DIR="$CLAUDE_PROJECT_DIR"

# State 1: Declined
if [[ -f "$PROJECT_DIR/.claude/no-doc-tooling" ]]; then
  echo "Documentation catalog tooling declined for this project. Remove \`.claude/no-doc-tooling\` to enable."
  exit 0
fi

# State 2: Installed — check common locations
# `ways` exports the project's doc tool path when it has one.
DOC_SCRIPT="${WAYS_DOC_TOOL:-}"

if [[ -n "$DOC_SCRIPT" ]]; then
  echo "## Documentation Catalog Tooling"
  echo ""
  echo "Use \`$DOC_SCRIPT\` for the documentation catalog — docs and ADRs as one typed graph:"
  echo ""
  echo "| Command | Purpose |"
  echo "|---------|---------|"
  echo "| \`$DOC_SCRIPT coverage\` | Domain × mode coverage matrix |"
  echo "| \`$DOC_SCRIPT list [--domain D] [--mode M]\` | List catalog pages |"
  echo "| \`$DOC_SCRIPT gaps\` | Empty cells + doc/ADR imbalance |"
  echo "| \`$DOC_SCRIPT lint [--strict]\` | Lint the catalog graph (doclint) |"
  echo "| \`$DOC_SCRIPT domains\` | Show domain bands |"
  echo ""
  echo "Catalog pages carry frontmatter \`id: DD.NNN.P\`, \`domain\`, \`mode\` (Diátaxis: tutorial/how-to/reference/explanation), and \`related\`/\`supersedes\` edges. See this project's documentation-catalog ADR for the model."
  exit 0
fi

# State 3: Available
echo "## Documentation Catalog Tooling Available"
echo ""
echo "This project doesn't have the documentation catalog linter installed."
echo "A graph-aware system is available — it treats docs + ADRs as one typed graph:"
echo "- Diátaxis classification (tutorial / how-to / reference / explanation)"
echo "- \`DD.NNN.P\` catalog ids sharing ADR domain bands"
echo "- Coverage matrix + dangling-edge and supersede-cycle linting"
echo ""
echo "To vendor it, use the \`docs\` skill — it carries the install steps and the shared \`adr.yaml\` requirement. Or run \`/project-init\` to scaffold it alongside the rest of the repo."
echo ""
echo "To decline permanently: \`mkdir -p .claude && touch .claude/no-doc-tooling\`"
