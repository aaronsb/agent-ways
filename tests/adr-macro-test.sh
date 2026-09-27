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
check "Summary guidance" contains "probes labelled confident and not confident" "$out"
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

echo "v1 guide content"
out=$(run_macro "$(project content "$CURRENT" adr/v1)")
check "enactment" contains "enacted:" "$out"
check "no bold-label bullets" lacks "- **" "$out"

echo ""
echo "=== ADR Macro Tests: $PASS passed, $FAIL failed ==="
[[ $FAIL -eq 0 ]]
