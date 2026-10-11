#!/usr/bin/env bash
# scripts/release.sh refuses a release whose range changed plugins/attend/
# without moving the plugin's version (ADR-406 item 8). Runs `release.sh check`
# in a scratch repository with a tag, a change, and a bump.

set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
fail=0

check() {  # name expected actual
    if [[ "$2" == "$3" ]]; then echo "  PASS: $1"; else echo "  FAIL: $1 — expected [$2], got [$3]"; fail=1; fi
}

repo=$WORK/repo
mkdir -p "$repo/scripts" "$repo/plugins/attend/.claude-plugin" "$repo/plugins/attend/hooks" "$repo/tools/ways-cli"
cp "$ROOT/scripts/release.sh" "$repo/scripts/release.sh"
printf '[package]\nname = "ways"\nversion = "1.0.0"\n' > "$repo/tools/ways-cli/Cargo.toml"
manifest=$repo/plugins/attend/.claude-plugin/plugin.json
printf '{\n  "name": "attend",\n  "version": "0.17.0"\n}\n' > "$manifest"
echo "// one" > "$repo/plugins/attend/hooks/register.tsx"

g() { git -C "$repo" -c user.name=t -c user.email=t@t -c commit.gpgsign=false -c tag.gpgsign=false "$@"; }
g init -q
g add -A
g commit -qm base
g tag ways-v1.0.0

run() { (cd "$repo" && bash scripts/release.sh check ways >/dev/null 2>&1) && echo ok || echo refused; }

check "no change since the tag" ok "$(run)"
echo "README" > "$repo/README.md"; g add -A; g commit -qm "other"
check "a change outside plugins/attend" ok "$(run)"
echo "// two" > "$repo/plugins/attend/hooks/register.tsx"; g add -A; g commit -qm "mod change"
check "a mod change without a version bump" refused "$(run)"
sed -i.bak 's/0.17.0/0.18.0/' "$manifest" && rm -f "$manifest.bak"; g add -A; g commit -qm "bump"
check "the same change with the version moved" ok "$(run)"

exit $fail
