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

# The contract the vendored tool writes. From 2.2.0 the tool names it on a
# CURRENT_CONTRACT line and has `adr contract`; the line is read the same way
# as TOOL_VERSION, so the macro never runs the project's copy. A 2.x tool
# without the line writes adr/v1; an older one writes adr/v0.
tool_contract() {
  sed -nE 's/^CURRENT_CONTRACT = "(adr\/v[0-9]+)"$/\1/p' "$1" 2>/dev/null | head -1
}

# records_declaring DIR CONTRACT — prints "yes" when an ADR-*.md record under
# DIR declares CONTRACT in its frontmatter. Only the leading block between the
# first-line `---` and the next `---` is read, so a `contract:` line in a
# record's body (a fenced example) does not count.
records_declaring() {
  [[ -d "$1" ]] || return 0
  find "$1" -type f -name 'ADR-*.md' -exec awk -v want="$2" '
    { sub(/\r$/, "") }
    FNR == 1 { infm = ($0 == "---"); next }
    !infm { nextfile }
    /^---[ \t]*$/ { infm = 0; nextfile }
    /^contract:/ {
      v = $0
      sub(/^contract:[ \t]*/, "", v); sub(/[ \t]*#.*$/, "", v)
      gsub(/["\047]/, "", v); sub(/[ \t]+$/, "", v)
      if (v == want) { print "yes"; exit }
    }' {} + 2>/dev/null | head -1
}

# shape_notice — the vocabulary-shape notices (a large corpus under few
# domains or capabilities), from the installed agent-ways tool ($UNIVERSAL),
# never the project's copy. `adr domains --shape` prints them, one
# "Notice: ..." line each, or nothing. No python3, no PyYAML, an older tool
# without the flag, an error or a timeout all mean no notice.
shape_notice() {
  [[ -f "$UNIVERSAL" ]] && command -v python3 &>/dev/null || return 0
  local limit=() out line
  command -v timeout &>/dev/null && limit=(timeout 2)
  out=$(cd "$PROJECT_DIR" && "${limit[@]}" python3 "$UNIVERSAL" domains --shape 2>/dev/null) || return 0
  while IFS= read -r line; do
    [[ "$line" == "Notice: "* ]] || continue
    echo ""
    echo "_${line#Notice: }_"
  done <<< "$out"
}

# contract_newer A B — true when contract A (adr/vN) is newer than B
contract_newer() {
  local a=${1#adr/v} b=${2#adr/v}
  [[ "$a" =~ ^[0-9]+$ && "$b" =~ ^[0-9]+$ ]] && (( 10#$a > 10#$b ))
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
  echo "This project's records follow the adr/v1 contract (ADR-304): ADR means Agent Decision Record. Kinds and capabilities are declared in \`docs/architecture/adr.yaml\`."
  echo ""
  echo "| Command | Purpose |"
  echo "|---------|---------|"
  echo "| \`$s new <domain> <title> --kind K --verb V --capability C\` | Create a record with its v1 frontmatter (\`--agent\`, \`--model\` for a decision) |"
  echo "| \`$s lint [--check]\` | Validate records against the contract |"
  echo "| \`$s consider <n> --said \"...\" --via \"...\"\` | Append an answer to a decision's probes: the operator's, or an advisor's |"
  echo "| \`$s accept <n>\` | Accept a proposed record; refuses if the record fails its own field checks; \`$s lint\` checks its references |"
  echo "| \`$s reject <n> --reason \"...\"\` | Considered and declined |"
  echo "| \`$s abandon <n> --reason \"...\"\` | Dropped before a decision |"
  echo "| \`$s set <n> key=value key+=item\` | Edit frontmatter; refuses status, which the lifecycle commands set |"
  echo "| \`$s supersede <old> --by <new>\` | Record a supersession on both records |"
  echo "| \`$s enact <n> <commit>\` | Mark an accepted cut or retire done |"
  echo "| \`$s list --kind K --capability C --field KEY=VALUE --group-by KEY\` | Query the corpus (\`--json\` for scripts) |"
  echo "| \`$s cite [--check]\` | Check ADR-N citations in code against the records |"
  echo "| \`$s import scan <paths>\`, \`import apply [--dry-run]\` | Convert v0 or foreign records through editable sheets (ADR-306) |"
  echo "| \`$s domain add\`, \`rename\`, \`move\` | Change the layout; paths are rewritten, numbers never change |"
  echo "| \`$s view <n>\`, \`index -y\`, \`archive\`, \`domains\` | As before |"
  echo ""
  echo "Use these commands for record edits; they check what hand edits skip."
  echo ""
  echo "### Record format"
  echo ""
  echo "Every record declares \`contract: adr/v1\`, a \`kind\` (decision, spec or evidence, or what adr.yaml declares), a \`capability\` from the vocabulary, and a \`status\` (proposed, accepted, rejected, abandoned, superseded, archived)."
  echo ""
  echo "A decision decides. A spec describes behaviour that is kept current and stays editable. Evidence records a finding, survey, audit, measurement or exploration at a point in time, and a decision cites it in its \`basis\` (ADR-309)."
  echo ""
  echo "A record's number is its permanent identity. Its folder under \`docs/architecture/\` is its area, and a domain's band only allocates new numbers (ADR-310)."
  echo ""
  echo "A decision also carries a \`verb\` (add, cut, change, retire, constrain), a \`basis\`, and \`agent: {name, model}\`. Each basis entry names one source: operator, evidence, standard, upstream, or precedent. A precedent names the record the decision rests on."
  echo ""
  echo "An operator basis records \`level\` (authored, directed, guided), \`said\` and \`via\`. Write one only when the operator actually said it: quote written channels verbatim and mark a spoken one \`paraphrase: true\`. The agent makes the technical calls and grounds them in the basis. The Summary's probes are one or two checks of the operator's intent, asked in their terms; record any answer with \`consider\`, and \`accept\` does not wait on one. With no one present, as under \`/goal\`, an advisor may answer, recorded with \`consider --operator advisor\`. An optional canary, a deliberately wrong and harmless probe asked in the conversation only, is recorded with \`--canary caught|missed\` on whichever entry answered it. When you ask the operator, use plain words, not the field names (adr/consider)."
  echo ""
  echo "A decision may carry \`observable\`: a list of what should be seen, run or tried when it holds, as plain lines or mappings with whatever keys suit (ADR-307). It is optional, and may be added after acceptance. When drafting an add or change, ask the operator what should be observable; they may name it, decline, or leave the observing to you, in which case run the work and iterate until you can show it, then record what you used."
  echo ""
  echo "Records link through \`supersedes\`, \`amends: ADR-N#section\`, \`extends\` and \`decided_by\`. A change names what it replaces: it supersedes the decision it replaces, or amends the section it changes in a broader one."
  echo ""
  echo "A cut or retire is enacted once the removal lands: \`$s enact <n> <commit>\` sets \`enacted: \"<commit>\"\` on the accepted decision."
  echo ""
  echo "### Summary"
  echo ""
  echo "Every decision opens with \`## Summary\`, written so someone who did not take part can judge it: what is decided, what it trades away, whether it is one-way (said first when it is), one or two probes that check the operator's intent, labelled confident or not confident, and an inversion naming the two ends the decision sits between."
  echo ""
  echo "### Conventions"
  echo ""
  echo "The tool checks each record's fields and sections, and that its references resolve. It does not check the conventions below. This way teaches them, and review catches a record that breaks them (ADR-311)."
  echo ""
  echo "- Correct an accepted record by appending. Leave what was decided as written."
  echo "- Record a change in what the project does as a new decision that names what it replaces, through \`supersedes\` or \`amends\`."
  echo "- Quote the operator in their own words."
  echo ""
  echo "Git keeps every earlier version of a record and who changed it. Read them with \`git log -p\`."
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
  current=$(tool_contract "$PROJECT_DIR/$ADR_SCRIPT")
  has_contract_cmd=0
  [[ -n "$current" ]] && has_contract_cmd=1
  if [[ -z "$current" ]]; then
    if [[ $v1_tool -eq 1 ]]; then current="adr/v1"; else current="adr/v0"; fi
  fi
  [[ "$contract" == "adr/v0" ]] && contract=""
  # Records that already declare the tool's contract while adr.yaml does not
  records_ahead=0
  if [[ -z "$contract" && "$current" != "adr/v0" ]] \
     && [[ -n "$(records_declaring "$PROJECT_DIR/docs/architecture" "$current")" ]]; then
    records_ahead=1
  fi

  # The four tool/contract combinations (ADR-304 §10)
  if [[ "$contract" == "adr/v1" && $v1_tool -eq 1 ]]; then
    print_v1_guide "$ADR_SCRIPT"
  elif [[ "$contract" == "adr/v1" ]]; then
    echo "**\`docs/architecture/adr.yaml\` declares \`contract: adr/v1\`, but \`$ADR_SCRIPT\` is $version_label and cannot enforce it.** Re-vendor the tool (the \`adr\` skill) before writing records."
    echo ""
    print_v0_commands "$ADR_SCRIPT"
  elif [[ -n "$contract" ]]; then
    echo "**\`docs/architecture/adr.yaml\` declares \`contract: $contract\`, which this guidance does not know.** Check the value; the tool writes $current. Until then the v0 guidance below applies."
    echo ""
    print_v0_guide "$ADR_SCRIPT"
  elif [[ $records_ahead -eq 1 ]]; then
    echo "**Records under \`docs/architecture/\` declare \`contract: $current\`, but \`docs/architecture/adr.yaml\` declares no contract, so the tool checks them under the v0 rules.**"
    if [[ $has_contract_cmd -eq 1 ]]; then
      echo "Run \`$ADR_SCRIPT contract --upgrade\` to bring \`adr.yaml\` to $current. It adds the contract line and the blocks the contract needs, and leaves the other lines as they are."
    else
      echo "Re-vendor the tool (the \`adr\` skill), then run \`$ADR_SCRIPT contract --upgrade\` to bring \`adr.yaml\` to $current."
    fi
    echo ""
    print_v0_guide "$ADR_SCRIPT"
  else
    print_v0_guide "$ADR_SCRIPT"
    if [[ $v1_tool -eq 1 ]]; then
      echo ""
      if [[ $has_contract_cmd -eq 1 ]]; then
        echo "_This tool supports the $current contract (ADR-304). Adopting it is a decision with \`capability: adr\`; until \`adr.yaml\` declares \`contract: $current\`, the v0 rules apply. Once it is decided, \`$ADR_SCRIPT contract --upgrade\` writes that line and the blocks the contract needs._"
      else
        echo "_This tool supports the $current contract (ADR-304). Adopting it is a decision with \`capability: adr\`; until \`adr.yaml\` declares \`contract: $current\`, the v0 rules apply._"
      fi
    fi
  fi
  # A config on a known contract older than the tool's
  if [[ -n "$contract" && $has_contract_cmd -eq 1 ]] && contract_newer "$current" "$contract"; then
    echo ""
    echo "_\`adr.yaml\` declares $contract; this tool writes $current. \`$ADR_SCRIPT contract --upgrade\` brings \`adr.yaml\` to $current._"
  fi

  # A large corpus under few names, as the installed tool reports it.
  [[ $has_contract_cmd -eq 1 ]] && shape_notice

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
