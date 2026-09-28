#!/usr/bin/env bash
# The ADR way's macro discloses guidance for the project's tool and contract
# (ADR-304 §10, #563). Each case builds a throwaway project and runs the macro
# against it, with the installed template pointed at this checkout's tool.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
MACRO="$REPO_ROOT/hooks/ways/documentation/adr/macro.sh"
TOOL="$REPO_ROOT/hooks/ways/documentation/adr/adr-tool"

WORK="$(cd "$(mktemp -d)" && pwd -P)"
trap 'rm -rf "$WORK"' EXIT
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1

PASS=0
FAIL=0
check() {  # check NAME EXPECT(contains|lacks) NEEDLE OUTPUT
  local name="$1" mode="$2" needle="$3" out="$4"
  if [[ "$mode" == contains && "$out" == *"$needle"* ]] || [[ "$mode" == lacks && "$out" != *"$needle"* ]]; then
    PASS=$((PASS + 1)); echo "  PASS: $name"
  else
    FAIL=$((FAIL + 1)); echo "  FAIL: $name ($mode '$needle')"; echo "$out" | sed 's/^/      /' | head -12
  fi
}

# project NAME TOOL_VERSION CONTRACT — a git project with a vendored tool
project() {
  local dir="$WORK/$1" version="$2" contract="$3"
  mkdir -p "$dir/docs/scripts" "$dir/docs/architecture"
  sed "s/^TOOL_VERSION = .*/TOOL_VERSION = \"$version\"/" "$TOOL" > "$dir/docs/scripts/adr"  # BSD and GNU alike
  chmod +x "$dir/docs/scripts/adr"
  { [[ -n "$contract" ]] && printf 'contract: %s\n' "$contract"; echo "domains: {}"; } > "$dir/docs/architecture/adr.yaml"
  git -C "$dir" init -q
  echo "$dir"
}

run_macro() { CLAUDE_PROJECT_DIR="$1" ADR_UNIVERSAL_TOOL="$TOOL" bash "$MACRO" 2>&1; }
CURRENT=$(sed -nE 's/^TOOL_VERSION = "([^"]+)"$/\1/p' "$TOOL" | head -1)

echo "Not a git repo, declined, not installed"
out=$(mkdir -p "$WORK/plain" && run_macro "$WORK/plain")
check "outside git: silent" lacks "ADR" "$out"
mkdir -p "$WORK/declined/.claude" && git -C "$WORK/declined" init -q && touch "$WORK/declined/.claude/no-adr-tooling"
out=$(run_macro "$WORK/declined")
check "declined: says so" contains "ADR tooling declined" "$out"
check "declined: still gets the v0 format" contains "Record format (adr/v0)" "$out"
mkdir -p "$WORK/bare" && git -C "$WORK/bare" init -q
out=$(run_macro "$WORK/bare")
check "not installed: offers the adr skill" contains "ADR Tooling Available" "$out"
check "not installed: still gets the v0 format" contains "Record format (adr/v0)" "$out"

echo "Legacy tool, no contract"
out=$(run_macro "$(project legacy-v0 1.2.0 '')")
check "v0 guide" contains "Record format (adr/v0)" "$out"
check "no v1 guide" lacks "ADR Tooling (adr/v1)" "$out"
check "stale tool noted, re-vendor safe" contains "Re-vendoring does not change the contract" "$out"

echo "v1-capable tool, no contract"
out=$(run_macro "$(project v2-v0 "$CURRENT" '')")
check "v0 guide" contains "Record format (adr/v0)" "$out"
check "v1 offered as a decision" contains "Adopting it is a decision with \`capability: adr\`" "$out"
check "no drift note at equal versions" lacks "out of date" "$out"

echo "v1-capable tool, adr/v1 contract"
out=$(run_macro "$(project v2-v1 "$CURRENT" adr/v1)")
check "v1 guide" contains "ADR Tooling (adr/v1)" "$out"
check "v1 guide names observable" contains "\`observable\`" "$out"
check "lifecycle commands" contains "accept <n>" "$out"
check "Summary guidance" contains "probes that check the operator's intent" "$out"
check "no v0 format" lacks "Record format (adr/v0)" "$out"

echo "Legacy tool, adr/v1 contract"
out=$(run_macro "$(project legacy-v1 1.2.0 adr/v1)")
check "warns the tool cannot enforce it" contains "cannot enforce it" "$out"
check "gives the v0 commands" contains "| Command | Purpose |" "$out"
check "no v0 record format under a v1 contract" lacks "Record format (adr/v0)" "$out"
check "no v1 guide" lacks "ADR Tooling (adr/v1)" "$out"

echo "Quoted and commented contract values"
check "double-quoted" contains "ADR Tooling (adr/v1)" "$(run_macro "$(project q2 "$CURRENT" '"adr/v1"')")"
check "single-quoted" contains "ADR Tooling (adr/v1)" "$(run_macro "$(project q1 "$CURRENT" "'adr/v1'")")"
check "trailing comment" contains "ADR Tooling (adr/v1)" "$(run_macro "$(project qc "$CURRENT" 'adr/v1  # adopted 2025-06')")"

echo "Unknown contract"
out=$(run_macro "$(project unknown "$CURRENT" adr/v2)")
check "names the unknown value" contains "contract: adr/v2" "$out"
check "falls back to v0" contains "Record format (adr/v0)" "$out"

echo "Odd version stamps"
check "prerelease 2.0.0-rc1 is v1-capable" contains "ADR Tooling (adr/v1)" "$(run_macro "$(project rc 2.0.0-rc1 adr/v1)")"
check "leading zero 08.1.0 is v1-capable" contains "ADR Tooling (adr/v1)" "$(run_macro "$(project oct 08.1.0 adr/v1)")"
dir=$(project unversioned 1.0.0 '')
sed -i.bak '/^TOOL_VERSION = /d' "$dir/docs/scripts/adr" && rm "$dir/docs/scripts/adr.bak"
out=$(run_macro "$dir")
check "unversioned copy: predates versioning" contains "predates tool versioning" "$out"
check "unversioned copy: re-vendor is safe" contains "Re-vendoring does not change the contract" "$out"
dir=$(project unversioned-v1 1.0.0 adr/v1)
sed -i.bak '/^TOOL_VERSION = /d' "$dir/docs/scripts/adr" && rm "$dir/docs/scripts/adr.bak"
check "unversioned under v1: labelled unversioned" contains "is unversioned and cannot enforce it" "$(run_macro "$dir")"

echo "Current contract read from the tool (#614)"
check "v1-capable tool, no contract: offers the upgrade" contains "docs/scripts/adr contract --upgrade\` writes that line" "$(run_macro "$(project up-v0 "$CURRENT" '')")"
check "explicit adr/v0: the v0 guide, not the unknown notice" lacks "does not know" "$(run_macro "$(project up-explicit "$CURRENT" adr/v0)")"
check "v1 contract: no upgrade offer" lacks "contract --upgrade" "$(run_macro "$(project up-v1 "$CURRENT" adr/v1)")"
dir=$(project up-old 2.1.0 '')
sed -i.bak '/^CURRENT_CONTRACT = /d' "$dir/docs/scripts/adr" && rm "$dir/docs/scripts/adr.bak"
out=$(run_macro "$dir")
check "2.1 tool without the line: falls back to adr/v1" contains "supports the adr/v1 contract" "$out"
check "2.1 tool without the command: no upgrade offer" lacks "contract --upgrade" "$out"
dir=$(project up-ahead "$CURRENT" '')
mkdir -p "$dir/docs/architecture/core"
printf -- '---\ncontract: adr/v1\nkind: decision\n---\n\n# ADR-100: x\n' > "$dir/docs/architecture/core/ADR-100-x.md"
out=$(run_macro "$dir")
check "records ahead of adr.yaml: said" contains "declare \`contract: adr/v1\`, but \`docs/architecture/adr.yaml\` declares no contract" "$out"
check "records ahead of adr.yaml: offers the upgrade" contains "Run \`docs/scripts/adr contract --upgrade\`" "$out"
check "records ahead of adr.yaml: v0 guide still applies" contains "Record format (adr/v0)" "$out"
dir=$(project up-body "$CURRENT" '')
mkdir -p "$dir/docs/architecture/core"
printf -- '---\nstatus: Accepted\ndate: 2025-01-01\n---\n\n# ADR-100: x\n\n```yaml\ncontract: adr/v1\n```\n' > "$dir/docs/architecture/core/ADR-100-x.md"
printf -- '# ADR-101: no frontmatter\n\ncontract: adr/v1\n' > "$dir/docs/architecture/core/ADR-101-y.md"
out=$(run_macro "$dir")
check "contract line in a record body: not records ahead" lacks "Records under" "$out"
check "contract line in a record body: no push to run the upgrade" lacks "Run \`docs/scripts/adr contract --upgrade\`" "$out"
dir=$(project up-ahead-crlf "$CURRENT" '')
mkdir -p "$dir/docs/architecture/core"
printf -- '---\r\ncontract: "adr/v1"  # imported\r\nkind: decision\r\n---\r\n\r\n# ADR-100: x\r\n' > "$dir/docs/architecture/core/ADR-100-x.md"
check "records ahead: quoted, commented, CRLF frontmatter" contains "Records under" "$(run_macro "$dir")"
dir=$(project up-ahead-old 2.1.0 '')
sed -i.bak '/^CURRENT_CONTRACT = /d' "$dir/docs/scripts/adr" && rm "$dir/docs/scripts/adr.bak"
mkdir -p "$dir/docs/architecture/core"
printf -- '---\ncontract: adr/v1\nkind: decision\n---\n\n# ADR-100: x\n' > "$dir/docs/architecture/core/ADR-100-x.md"
check "records ahead, tool without the command: re-vendor first" contains "Re-vendor the tool (the \`adr\` skill), then run" "$(run_macro "$dir")"

echo "Vocabulary shape (from the installed tool)"
# records DIR FOLDER FIRST COUNT FRONTMATTER — COUNT records with that frontmatter
records() {
  local n
  mkdir -p "$1/docs/architecture/$2"
  for ((n = $3; n < $3 + $4; n++)); do
    printf -- '---\n%b---\n\n# ADR-%d: r\n' "$5" "$n" > "$1/docs/architecture/$2/ADR-$n-r.md"
  done
}
# domains DIR — declare core, api and ui
domains() {
  local cfg="$1/docs/architecture/adr.yaml"
  grep -v '^domains:' "$cfg" > "$cfg.tmp"
  printf 'domains:\n  core: {range: [100, 199], name: Core, description: Core, folder: core}\n  api: {range: [300, 399], name: API, description: API, folder: api}\n  ui: {range: [500, 599], name: UI, description: UI, folder: ui}\n' >> "$cfg.tmp"
  mv "$cfg.tmp" "$cfg"
}
dir=$(project shape-fat "$CURRENT" '')
domains "$dir"
records "$dir" core 100 45 'status: Accepted\ndate: 2025-01-01\n'
records "$dir" api 300 5 'status: Accepted\ndate: 2025-01-01\n'
out=$(run_macro "$dir")
check "fat domain: notice" contains "_50 records in 2 domains; core holds 45 (90%)." "$out"
check "fat domain: names the seeds" contains "Of the 3 capabilities seeded from the domains" "$out"
check "fat domain: one notice" lacks "adr/v1 records use" "$out"
dir=$(project shape-even "$CURRENT" '')
domains "$dir"
records "$dir" core 100 15 'status: Accepted\ndate: 2025-01-01\n'
records "$dir" api 300 15 'status: Accepted\ndate: 2025-01-01\n'
records "$dir" ui 500 15 'status: Accepted\ndate: 2025-01-01\n'
check "balanced domains: no notice" lacks "records in" "$(run_macro "$dir")"
dir=$(project shape-v1 "$CURRENT" adr/v1)
domains "$dir"
printf 'capabilities:\n  ingest: x\n  search: y\n' >> "$dir/docs/architecture/adr.yaml"
records "$dir" core 100 45 'contract: adr/v1\nkind: evidence\ncapability: ingest\nstatus: accepted\ndate: 2025-01-01\n'
out=$(run_macro "$dir")
check "fat capability: notice" contains "_45 adr/v1 records use 1 capability (ingest)." "$out"
check "fat capability: no domain notice under v1" lacks "records in" "$out"
# Fail quiet: the notice needs the installed tool to answer.
fat="$WORK/shape-fat"
out=$(CLAUDE_PROJECT_DIR="$fat" ADR_UNIVERSAL_TOOL="$WORK/no-such-tool" bash "$MACRO" 2>&1)
check "missing installed tool: no notice" lacks "records in" "$out"
check "missing installed tool: guidance still printed" contains "Record format (adr/v0)" "$out"
printf '#!/usr/bin/env python3\nprint("Notice: 1 records in 1 domain.")\nraise SystemExit(1)\n' > "$WORK/failing-tool"
out=$(CLAUDE_PROJECT_DIR="$fat" ADR_UNIVERSAL_TOOL="$WORK/failing-tool" bash "$MACRO" 2>&1)
check "installed tool exits nonzero: no notice" lacks "records in" "$out"
printf '#!/usr/bin/env python3\nimport sys\nsys.exit(2 if "--shape" in sys.argv else 0)\n' > "$WORK/old-tool"
out=$(CLAUDE_PROJECT_DIR="$fat" ADR_UNIVERSAL_TOOL="$WORK/old-tool" bash "$MACRO" 2>&1)
check "installed tool without --shape: no notice" lacks "records in" "$out"
printf '#!/usr/bin/env python3\nprint("Traceback (most recent call last):")\nprint("Notice-ish")\n' > "$WORK/noisy-tool"
out=$(CLAUDE_PROJECT_DIR="$fat" ADR_UNIVERSAL_TOOL="$WORK/noisy-tool" bash "$MACRO" 2>&1)
check "installed tool prints other lines: none passed on" lacks "Traceback" "$out"

echo "v1 guide content"
out=$(run_macro "$(project content "$CURRENT" adr/v1)")
check "enactment" contains "enacted:" "$out"
check "no bold-label bullets" lacks "- **" "$out"

echo ""
echo "=== ADR Macro Tests: $PASS passed, $FAIL failed ==="
[[ $FAIL -eq 0 ]]
