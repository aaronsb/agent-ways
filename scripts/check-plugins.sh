#!/usr/bin/env bash
# Gates for the Claude Code mods under plugins/ (ADR-406):
#
#   1. the attend plugin carries attend's version (the plugin is released
#      with attend, since it depends on attend's verbs), and the mod's
#      MOD_VERSION constant in hooks/lib.ts equals the plugin's version;
#      the ways-judge plugin carries ways' version, for the same reason
#      (it calls `ways judge host|take|answer`, ADR-708);
#   2. `claude plugin validate` on the marketplace and on each plugin;
#   3. `claude plugin test` on each plugin;
#   4. a TypeScript check against Claude Code's declarations, where they can
#      be found: $CLAUDE_CODE_TYPES (the API's claude-code.d.ts), or the copy
#      the engine lays in <plugin>/.claude-plugin/types/ when it loads the
#      plugin from a folder it hot-reloads. Claude Code ships no command that
#      writes them, so a CI runner reports this step skipped.
#
# Usage: scripts/check-plugins.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

fail() { echo "check-plugins: $*" >&2; exit 1; }

plugin_version() {
  grep -oE '"version"[[:space:]]*:[[:space:]]*"[^"]*"' "$1" | head -n1 | sed -E 's/.*"([^"]*)"$/\1/'
}
cargo_version() {
  grep -m1 -E '^version *= *"' "$1" | sed -E 's/.*"([^"]+)".*/\1/'
}

# 1. Version lockstep with attend.
PV="$(plugin_version plugins/attend/.claude-plugin/plugin.json)"
AV="$(cargo_version tools/attend/Cargo.toml)"
[[ -n "$PV" ]] || fail "plugins/attend/.claude-plugin/plugin.json has no version"
[[ "$PV" == "$AV" ]] || fail "the attend plugin is at $PV but attend is at $AV; they release together (scripts/release.sh bump attend moves both)"
echo "version: attend plugin $PV matches attend $AV"

# The mod sends MOD_VERSION with its beat; it cannot read plugin.json, so the
# two are held equal here.
MV="$(grep -oE "^export const MOD_VERSION = '[^']*'" plugins/attend/hooks/lib.ts | sed -E "s/.*'([^']*)'/\\1/")"
[[ -n "$MV" ]] || fail "plugins/attend/hooks/lib.ts has no MOD_VERSION"
[[ "$MV" == "$PV" ]] || fail "MOD_VERSION in plugins/attend/hooks/lib.ts is $MV but the plugin is at $PV; scripts/release.sh bump attend moves both"
echo "version: MOD_VERSION $MV matches the plugin"

# The ways-judge mod calls ways verbs and is released with ways.
JV="$(plugin_version plugins/ways-judge/.claude-plugin/plugin.json)"
WV="$(cargo_version tools/ways-cli/Cargo.toml)"
[[ -n "$JV" ]] || fail "plugins/ways-judge/.claude-plugin/plugin.json has no version"
[[ "$JV" == "$WV" ]] || fail "the ways-judge plugin is at $JV but ways is at $WV; they release together (scripts/release.sh bump ways moves both)"
echo "version: ways-judge plugin $JV matches ways $WV"

# Where an organization manages Claude Code (Team and Enterprise, or a
# machine with managed settings), the built-in cc-plugin-sec-default sits
# outermost and sends classic.*, session.compact and other events past the
# plugins a person installs, so a mod's hook on one never runs there. Each
# mod's tests record what its register() hooks and check it against that
# list (passedOverHits, plugins/attend/tests/passed-over.ts), and
# `claude plugin test` below runs it. This step holds every mod with hooks
# to having that test: one that runs its register() through the recorder
# and checks the result, not one that only names the helper.
mods=0
for dir in plugins/*/hooks; do
  [ -d "$dir" ] || continue
  mods=$((mods + 1))
  plugin="${dir%/hooks}"
  found=0
  for t in "$plugin"/tests/*.test.ts; do
    [ -f "$t" ] || continue
    if grep -q -F -e 'registrationsOf(register' "$t" && grep -q -F -e 'passedOverHits(' "$t"; then
      found=1
    fi
  done
  [ "$found" -eq 1 ] || fail "$plugin has no test that runs its register() through registrationsOf and checks it with passedOverHits"
done
[ "$mods" -gt 0 ] || fail "no plugins/*/hooks directory to check"
# A plugin's tests import only from its own folder, so each mod carries a
# copy of the passed-over list, held to attend's text.
for copy in plugins/*/tests/passed-over.ts; do
  cmp -s "$copy" plugins/attend/tests/passed-over.ts || fail "$copy differs from plugins/attend/tests/passed-over.ts; the passed-over list is kept as one text"
done
echo "hooks: $mods mod(s) test their hooks against the events the policy plugin passes over"

command -v claude >/dev/null || fail "claude is not on PATH"

# 2. and 3.
claude plugin validate plugins >/dev/null || { claude plugin validate plugins; fail "the marketplace does not validate"; }
echo "validate: plugins (marketplace agent-ways)"
for dir in plugins/*/; do
  dir="${dir%/}"
  [[ -f "$dir/.claude-plugin/plugin.json" ]] || continue
  claude plugin validate "$dir" >/dev/null || { claude plugin validate "$dir"; fail "$dir does not validate"; }
  echo "validate: $dir"
  if compgen -G "$dir/tests/*.test.ts" >/dev/null || compgen -G "$dir/tests/*.test.tsx" >/dev/null; then
    claude plugin test "$dir"
  else
    fail "$dir has no tests"
  fi
done

# 4. Type check. One scratch directory holds each plugin's tsconfig in turn.
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
for dir in plugins/*/; do
  dir="${dir%/}"
  [[ -f "$dir/.claude-plugin/plugin.json" ]] || continue
  decl="${CLAUDE_CODE_TYPES:-}"
  [[ -n "$decl" ]] || decl="$dir/.claude-plugin/types/claude-code/index.d.ts"
  if [[ ! -f "$decl" ]]; then
    echo "typecheck: $dir skipped, no Claude Code declarations (set CLAUDE_CODE_TYPES to claude-code.d.ts)"
    continue
  fi
  abs="$ROOT/$dir"
  cat > "$tmp/tsconfig.json" <<EOF
{
  "compilerOptions": {
    "target": "es2023", "lib": ["es2023"], "types": [],
    "module": "esnext", "moduleResolution": "bundler",
    "allowImportingTsExtensions": true,
    "strict": true, "noUncheckedIndexedAccess": true,
    "noEmit": true, "skipLibCheck": true,
    "jsx": "react", "jsxFactory": "h", "jsxFragmentFactory": "Fragment"
  },
  "include": ["$(cd "$(dirname "$decl")" && pwd)/$(basename "$decl")", "$abs/hooks", "$abs/types", "$abs/tests"]
}
EOF
  if command -v tsc >/dev/null; then
    tsc -p "$tmp/tsconfig.json"
  else
    npx --yes --package typescript@5.6.3 -- tsc -p "$tmp/tsconfig.json"
  fi
  echo "typecheck: $dir"
done
