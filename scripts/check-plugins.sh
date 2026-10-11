#!/usr/bin/env bash
# Gates for the Claude Code mods under plugins/ (ADR-406):
#
#   1. the attend plugin carries attend's version (the plugin is released
#      with attend, since it depends on attend's verbs);
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
  sed -nE 's/^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/p' "$1" | head -n1
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

# 4. Type check.
for dir in plugins/*/; do
  dir="${dir%/}"
  [[ -f "$dir/.claude-plugin/plugin.json" ]] || continue
  decl="${CLAUDE_CODE_TYPES:-}"
  [[ -n "$decl" ]] || decl="$dir/.claude-plugin/types/claude-code/index.d.ts"
  if [[ ! -f "$decl" ]]; then
    echo "typecheck: $dir skipped, no Claude Code declarations (set CLAUDE_CODE_TYPES to claude-code.d.ts)"
    continue
  fi
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
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
