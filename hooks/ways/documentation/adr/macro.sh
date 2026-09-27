#!/usr/bin/env bash
# ADR way macro — the record guidance that fits this project.
#
# Tooling states:
#   declined  → .claude/no-adr-tooling exists → one-liner, stop nagging
#   installed → docs/scripts/adr (or similar) found → guidance for its contract
#   available → neither → suggest installation
#
# When installed, two axes vary independently (ADR-304 §10):
#   tool version      the vendored copy's TOOL_VERSION; 2.x and later is v1-capable
#   contract version  `contract:` in docs/architecture/adr.yaml; absent means adr/v0
# The way body stays contract-neutral. Everything that depends on the
# contract (commands, record format, lifecycle) is printed here.
#
# ADR_UNIVERSAL_TOOL overrides the installed template's path (tests).

PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$PWD}"
UNIVERSAL="${ADR_UNIVERSAL_TOOL:-${HOME}/.claude/hooks/ways/documentation/adr/adr-tool}"

# Capture is shape-restricted: a stamp that isn't a plain version string is
# treated as unversioned rather than echoed into disclosed context (ADR-177).
# sed -E rather than grep -P, which BSD grep (macOS) lacks.
tool_version() {
  sed -nE 's/^TOOL_VERSION = "([0-9]+(\.[0-9]+)*(-[0-9A-Za-z.]+)?)"$/\1/p' "$1" 2>/dev/null | head -1
}

print_v0_commands() {
  local s="$1"
  echo "## ADR Tooling"
  echo ""
  echo "Use \`$s\` for ADR management:"
  echo ""
  echo "| Command | Purpose |"
  echo "|---------|---------|"
  echo "| \`$s new <domain> <title>\` | Create new ADR |"
  echo "| \`$s list [--group]\` | List all ADRs |"
  echo "| \`$s view <number>\` | View an ADR |"
  echo "| \`$s lint [--check]\` | Validate ADRs (incl. supersession links) |"
  echo "| \`$s index -y\` | Regenerate index (active set) |"
  echo "| \`$s archive <n> --reason \"...\"\` | Move an ADR out of the active set |"
  echo "| \`$s domains\` | Show domain series |"
  echo ""
  echo "**Always use \`$s new\` to create ADRs.** It handles numbering, domain routing, and templates."
}

print_v0_format() {
  echo "### Record format (adr/v0)"
  echo ""
  echo "YAML frontmatter carries \`status\`, \`date\`, \`deciders\` and \`related\`. The body has Context, Decision (with a reversibility grade), Consequences (Positive, Negative, Neutral) and Alternatives Considered. Statuses: \`Draft\` | \`Proposed\` | \`Accepted\` | \`Superseded\` | \`Deprecated\`."
  echo ""
  echo "A clarification to an accepted ADR lands in place: a fixed typo, a sharper sentence, a link. A change in what the project does gets a new ADR that supersedes the old one; flip the old status to Superseded and leave its body alone."
  echo ""
  echo "Workflow: debate, then create the record, then a PR for review. The ADR is accepted when the PR merges, and the index is regenerated."
}

print_v0_guide() {
  print_v0_commands "$1"
  echo ""
  print_v0_format
}

print_v1_guide() {
  local s="$1"
  echo "## ADR Tooling (adr/v1)"
  echo ""
  echo "This project's records follow the adr/v1 contract (ADR-304): ADR means Agent Decision Record. Kinds, capabilities and surfaces are declared in \`docs/architecture/adr.yaml\`."
  echo ""
  echo "| Command | Purpose |"
  echo "|---------|---------|"
  echo "| \`$s new <domain> <title>\` | Create a record (then set its v1 frontmatter) |"
  echo "| \`$s lint [--check]\` | Validate records against the contract |"
  echo "| \`$s accept <n>\` | Accept a proposed record; refuses if it would not lint clean |"
  echo "| \`$s reject <n> --reason \"...\"\` | Considered and declined |"
  echo "| \`$s abandon <n> --reason \"...\"\` | Dropped before a decision |"
  echo "| \`$s cite [--check]\` | Check ADR-N citations in code against the records |"
  echo "| \`$s list\`, \`view <n>\`, \`index -y\`, \`archive\`, \`domains\` | As before |"
  echo ""
  echo "### Record format"
  echo ""
  echo "Every record declares \`contract: adr/v1\`, a \`kind\` (decision or spec, or what adr.yaml declares), a \`capability\` from the vocabulary, and a \`status\` (proposed, accepted, rejected, abandoned, superseded, archived)."
  echo ""
  echo "A decision also carries a \`verb\` (add, cut, change, retire, constrain), a \`basis\`, and \`agent: {name, model}\`. Each basis entry names one source: operator, evidence, standard, upstream, or precedent. Following precedent must reach an external source."
  echo ""
  echo "An operator basis records \`level\` (authored, directed, guided), \`said\` and \`via\`. Write one only when the operator actually said it: quote written channels verbatim and mark a spoken one \`paraphrase: true\`. A decision the operator started waits for a \`considered\` entry before \`accept\`. When you ask the operator for either, use plain words, not the field names (adr/consider)."
  echo ""
  echo "A decision may carry \`observable\`: a list of what should be seen, run or tried when it holds, as plain lines or mappings with whatever keys suit (ADR-307). It is optional, and may be added after acceptance where the kind's \`mutable_after_accept\` lists it (the default list does). When drafting an add or change, ask the operator what should be observable; they may name it, decline, or leave the observing to you, in which case run the work and iterate until you can show it, then record what you used."
  echo ""
  echo "Records link through \`supersedes\`, \`amends: ADR-N#section\`, \`extends\` and \`decided_by\`. A change against a broader decision amends it."
  echo ""
  echo "A cut or retire is enacted once the removal lands: set \`enacted: \"<commit>\"\` (a quoted hash) on the accepted decision. Until then, \`cite\` warns on what still depends on it; after, it fails."
  echo ""
  echo "### Summary"
  echo ""
  echo "Every decision opens with \`## Summary\`, written so someone who did not take part can judge it: what is decided, what it trades away, whether it is one-way (said first when it is), probes labelled confident and not confident, and an inversion naming the two ends the decision sits between."
  echo ""
  echo "### Frozen once past proposed"
  echo ""
  echo "After a decision leaves proposed, only its kind's \`mutable_after_accept\` fields change, and the body grows only by appending. A change in what the project does is a new decision that amends or supersedes the old one."
}

# Outside a work tree there is nothing to vendor into; say nothing.
git -C "$PROJECT_DIR" rev-parse --is-inside-work-tree &>/dev/null || exit 0

# State 1: Declined
if [[ -f "$PROJECT_DIR/.claude/no-adr-tooling" ]]; then
  echo "ADR tooling declined for this project. Remove \`.claude/no-adr-tooling\` to enable."
  echo ""
  print_v0_format
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
  local_ver=$(tool_version "$PROJECT_DIR/$ADR_SCRIPT")
  univ_ver=""
  [[ -f "$UNIVERSAL" ]] && univ_ver=$(tool_version "$UNIVERSAL")
  # contract: adr/v1, optionally quoted, optionally followed by a comment
  contract=$(sed -nE "s/^contract:[[:space:]]*[\"']?([A-Za-z0-9/._-]+)[\"']?[[:space:]]*(#.*)?\$/\\1/p" \
    "$PROJECT_DIR/docs/architecture/adr.yaml" 2>/dev/null | head -1)
  v1_tool=0
  major=${local_ver%%[!0-9]*}
  [[ -n "$major" ]] && (( 10#$major >= 2 )) && v1_tool=1
  version_label=${local_ver:+v$local_ver}
  version_label=${version_label:-unversioned}

  # The four tool/contract combinations (ADR-304 §10)
  if [[ "$contract" == "adr/v1" && $v1_tool -eq 1 ]]; then
    print_v1_guide "$ADR_SCRIPT"
  elif [[ "$contract" == "adr/v1" ]]; then
    echo "**\`docs/architecture/adr.yaml\` declares \`contract: adr/v1\`, but \`$ADR_SCRIPT\` is $version_label and cannot enforce it.** Re-vendor the tool (the \`adr\` skill) before writing records."
    echo ""
    print_v0_commands "$ADR_SCRIPT"
  elif [[ -n "$contract" ]]; then
    echo "**\`docs/architecture/adr.yaml\` declares \`contract: $contract\`, which this guidance does not know.** Check the value; the known contract is adr/v1. Until then the v0 guidance below applies."
    echo ""
    print_v0_guide "$ADR_SCRIPT"
  else
    print_v0_guide "$ADR_SCRIPT"
    if [[ $v1_tool -eq 1 ]]; then
      echo ""
      echo "_This tool supports the adr/v1 contract (ADR-304). Adopting it is a decision with \`capability: adr\`; until \`adr.yaml\` declares \`contract: adr/v1\`, the v0 rules apply._"
    fi
  fi

  # Direction-aware drift check against the universal template (ADR-177):
  # compare TOOL_VERSION stamps to tell stale from customized from ahead.
  if [[ -f "$UNIVERSAL" ]]; then
    if [[ -z "$local_ver" && -n "$univ_ver" ]]; then
      echo ""
      echo "_The project's copy predates tool versioning (ways ships v${univ_ver}) — it is out of date. Re-vendor via the \`adr\` skill; if it was customized, diff first and carry the changes forward. Re-vendoring does not change the contract: v0 records keep working._"
    elif [[ -n "$local_ver" && -z "$univ_ver" ]]; then
      echo ""
      echo "_The project's copy is v${local_ver} but the installed template is unversioned — the agent-ways install is stale. Update it (\`/ways-update\`)._"
    elif [[ -n "$local_ver" && -n "$univ_ver" && "$local_ver" != "$univ_ver" ]]; then
      newest=$(printf '%s\n%s\n' "$local_ver" "$univ_ver" | sort -V | tail -1)
      if [[ "$newest" == "$univ_ver" ]]; then
        echo ""
        echo "_The project's copy is v${local_ver}; ways ships v${univ_ver} — out of date. Re-vendor via the \`adr\` skill; if it was customized, diff first and carry the changes forward. Re-vendoring does not change the contract: v0 records keep working._"
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
echo ""
print_v0_format
