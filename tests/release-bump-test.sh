#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

# scripts/release.sh bump and tag, for one component and for a batch on one
# branch (ADR-150). Runs against a scratch repository whose origin is a local
# bare repository, with gh stubbed, so nothing reaches GitHub. Asserts the
# manifest, lock and plugin versions, one commit per bump, the branch, the PR
# title, and the tags.

set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
fail=0

check() {  # name expected actual
    if [[ "$2" == "$3" ]]; then echo "  PASS: $1"; else echo "  FAIL: $1 — expected [$2], got [$3]"; fail=1; fi
}

command -v cargo >/dev/null || { echo "  SKIP: cargo is not on PATH"; exit 0; }

# Git reads only this config, so the operator's signing and hooks stay out.
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=$WORK/gitconfig
printf '[user]\n\tname = t\n\temail = t@t\n[commit]\n\tgpgsign = false\n[tag]\n\tgpgsign = false\n[init]\n\tdefaultBranch = main\n' > "$GIT_CONFIG_GLOBAL"

# gh records each PR title it is asked to open.
mkdir -p "$WORK/stub"
export GH_LOG=$WORK/gh.log
cat > "$WORK/stub/gh" <<'EOF'
#!/usr/bin/env bash
while [[ $# -gt 0 ]]; do [[ "$1" == --title ]] && { printf '%s\n' "$2" >> "$GH_LOG"; shift; }; shift; done
EOF
chmod +x "$WORK/stub/gh"
export PATH="$WORK/stub:$PATH"

repo=$WORK/repo
origin=$WORK/origin.git
mkdir -p "$repo/scripts" "$repo/plugins/attend/.claude-plugin" "$repo/plugins/attend/hooks" \
    "$repo/plugins/ways-judge/.claude-plugin" "$repo/tools"
cp "$ROOT/scripts/release.sh" "$repo/scripts/release.sh"
crate() {  # dir name version
    mkdir -p "$repo/tools/$1/src"
    printf '[package]\nname = "%s"\nversion = "%s"\nedition = "2021"\n' "$2" "$3" > "$repo/tools/$1/Cargo.toml"
    : > "$repo/tools/$1/src/lib.rs"
}
printf '[workspace]\nresolver = "2"\nmembers = ["ways-cli", "attend", "attend-chat"]\n' > "$repo/tools/Cargo.toml"
crate ways-cli ways 1.0.0
crate attend attend 0.17.0
crate attend-chat attend-chat 0.3.0
printf 'target/\n' > "$repo/.gitignore"
cargo generate-lockfile --offline --manifest-path "$repo/tools/Cargo.toml" >/dev/null 2>&1 \
    || { echo "  FAIL: cargo generate-lockfile for the fixture"; exit 1; }
printf '{\n  "name": "attend",\n  "version": "0.17.0"\n}\n' > "$repo/plugins/attend/.claude-plugin/plugin.json"
printf "export const MOD_VERSION = '0.17.0'\n" > "$repo/plugins/attend/hooks/lib.ts"
echo "// one" > "$repo/plugins/attend/hooks/register.tsx"
printf '{\n  "name": "ways-judge",\n  "version": "1.0.0"\n}\n' > "$repo/plugins/ways-judge/.claude-plugin/plugin.json"

g() { git -C "$repo" "$@"; }
g init -q
g add -A
g commit -qm base
g tag ways-v1.0.0; g tag attend-v0.17.0; g tag attend-chat-v0.3.0
git init -q --bare "$origin"
g remote add origin "$origin"
g push -q origin main

# Read a version out of the fixture.
manifest_v() { grep -m1 '^version' "$repo/tools/$1/Cargo.toml" | sed -E 's/.*"(.*)".*/\1/'; }
lock_v() { awk -v p="\"$1\"" '/^\[\[package\]\]/{n=""} $1=="name"{n=$3} $1=="version"&&n==p{gsub(/"/,"",$3);print $3;exit}' "$repo/tools/Cargo.lock"; }
plugin_v() { grep -m1 '"version"' "$repo/plugins/$1/.claude-plugin/plugin.json" | sed -E 's/.*: *"(.*)".*/\1/'; }
mod_v() { sed -nE "s/^export const MOD_VERSION = '(.*)'/\1/p" "$repo/plugins/attend/hooks/lib.ts"; }
commits() { g rev-list --count main..HEAD; }
changed() { g diff --name-only main HEAD | LC_ALL=C sort | paste -sd' '; }
last_title() { tail -n1 "$GH_LOG" 2>/dev/null; }
pushed() { git -C "$origin" rev-parse -q --verify "refs/heads/$1" >/dev/null && echo yes || echo no; }

bump() {  # components [level]: run the bump from main, print ok or refused
    g checkout -q main
    (cd "$repo" && bash scripts/release.sh bump "$@" >"$WORK/out" 2>&1) && echo ok || echo refused
}

refuses() {  # name reason components [level]: the bump refuses, naming the reason
    local name="$1" reason="$2"; shift 2
    check "$name" refused "$(bump "$@")"
    check "  for its reason" "$reason" "$(grep -oF "$reason" "$WORK/out" | head -n1)"
}

echo "one component"
check "bump ways minor" ok "$(bump ways minor)"
check "branch" "release/ways-v1.1.0" "$(g branch --show-current)"
check "ways manifest" 1.1.0 "$(manifest_v ways-cli)"
check "ways lock" 1.1.0 "$(lock_v ways)"
check "attend lock untouched" 0.17.0 "$(lock_v attend)"
check "ways-judge plugin" 1.1.0 "$(plugin_v ways-judge)"
check "one commit" 1 "$(commits)"
check "the commit's files" "plugins/ways-judge/.claude-plugin/plugin.json tools/Cargo.lock tools/ways-cli/Cargo.toml" "$(changed)"
check "commit subject" "release(ways): bump to 1.1.0" "$(g log -1 --format=%s)"
check "PR title" "release(ways): v1.1.0" "$(last_title)"
check "branch pushed" yes "$(pushed release/ways-v1.1.0)"

# A mod change on main without a plugin version move.
g checkout -q main
echo "// two" > "$repo/plugins/attend/hooks/register.tsx"
g commit -qam "mod change"
g push -q origin main

echo "the plugin check"
refuses "bump ways alone refuses over the mod change" "plugins/attend/ changed since ways-v1.0.0" ways patch
check "and stays on main" main "$(g branch --show-current)"

echo "several components"
date=$(date +%Y-%m-%d)
check "bump ways attend attend-chat minor" ok "$(bump "ways attend attend-chat" minor)"
check "branch" "release/${date}-3components" "$(g branch --show-current)"
check "ways manifest" 1.1.0 "$(manifest_v ways-cli)"
check "attend manifest" 0.18.0 "$(manifest_v attend)"
check "attend-chat manifest" 0.4.0 "$(manifest_v attend-chat)"
check "ways lock" 1.1.0 "$(lock_v ways)"
check "attend lock" 0.18.0 "$(lock_v attend)"
check "attend-chat lock" 0.4.0 "$(lock_v attend-chat)"
check "attend plugin" 0.18.0 "$(plugin_v attend)"
check "MOD_VERSION" 0.18.0 "$(mod_v)"
check "ways-judge plugin" 1.1.0 "$(plugin_v ways-judge)"
check "one commit" 1 "$(commits)"
check "the commit's files" "plugins/attend/.claude-plugin/plugin.json plugins/attend/hooks/lib.ts plugins/ways-judge/.claude-plugin/plugin.json tools/Cargo.lock tools/attend-chat/Cargo.toml tools/attend/Cargo.toml tools/ways-cli/Cargo.toml" "$(changed)"
check "PR title" "release: attend v0.18.0, ways v1.1.0, attend-chat v0.4.0" "$(last_title)"
check "branch pushed" yes "$(pushed "release/${date}-3components")"

echo "per-component levels"
check "bump attend:minor attend-chat:major with no default" ok "$(bump "attend:minor attend-chat:major")"
check "branch" "release/${date}-2components" "$(g branch --show-current)"
check "attend manifest" 0.18.0 "$(manifest_v attend)"
check "attend-chat manifest" 1.0.0 "$(manifest_v attend-chat)"
check "attend-chat lock" 1.0.0 "$(lock_v attend-chat)"
check "ways untouched" 1.0.0 "$(manifest_v ways-cli)"
check "one commit" 1 "$(commits)"
refuses "a batch whose branch exists" "branch release/${date}-2components already exists" "ways attend" patch
g branch -q -D "release/${date}-2components"
git -C "$origin" branch -q -D "release/${date}-2components"
check "bump ways:major,attend with patch as the default" ok "$(bump "ways:major,attend" patch)"
check "branch" "release/${date}-2components" "$(g branch --show-current)"
check "ways manifest" 2.0.0 "$(manifest_v ways-cli)"
check "ways lock" 2.0.0 "$(lock_v ways)"
check "attend manifest" 0.17.1 "$(manifest_v attend)"
check "attend lock" 0.17.1 "$(lock_v attend)"
check "attend-chat untouched" 0.3.0 "$(manifest_v attend-chat)"
check "attend plugin" 0.17.1 "$(plugin_v attend)"
check "MOD_VERSION" 0.17.1 "$(mod_v)"
check "ways-judge plugin" 2.0.0 "$(plugin_v ways-judge)"
check "one commit" 1 "$(commits)"
check "PR title" "release: attend v0.17.1, ways v2.0.0" "$(last_title)"

echo "refusals"
refuses "an entry with no level and no default" "no level for ways" "ways attend"
refuses "an unknown level" "level for ways must be patch|minor|major" "ways:huge attend" minor
refuses "an unknown component" "unknown component 'nope'" "ways nope" minor
refuses "a component listed twice" "ways is listed twice" "ways ways:patch" minor
g tag attend-v0.17.1 main
refuses "a tag that already exists" "tag attend-v0.17.1 already exists" "ways attend" patch
g tag -d attend-v0.17.1 >/dev/null
check "and stays on main" main "$(g branch --show-current)"

echo "publish"
# Land the per-component bump on main, as the merged PR would.
g checkout -q main
g merge -q --ff-only "release/${date}-2components"
g push -q origin main
(cd "$repo" && bash scripts/release.sh tag "ways attend" >"$WORK/out" 2>&1) && r=ok || r=refused
check "tag ways attend" ok "$r"
check "tags in order" "attend-v0.17.1 ways-v2.0.0" "$(sed -nE 's/^release: tagged ([^ ]+) on .*/\1/p' "$WORK/out" | paste -sd' ')"
check "attend tag on main" "$(g rev-parse main)" "$(g rev-parse 'attend-v0.17.1^{commit}')"
check "ways tag on main" "$(g rev-parse main)" "$(g rev-parse 'ways-v2.0.0^{commit}')"
check "not pushed without --push" "" "$(git -C "$origin" tag -l 'attend-v0.17.1' 'ways-v2.0.0')"
g tag -d attend-v0.17.1 ways-v2.0.0 >/dev/null
(cd "$repo" && bash scripts/release.sh tag "attend,ways" --push >"$WORK/out" 2>&1) && r=ok || r=refused
check "tag attend,ways --push" ok "$r"
check "both tags pushed" "attend-v0.17.1 ways-v2.0.0" "$(git -C "$origin" tag -l 'attend-v0.17.1' 'ways-v2.0.0' | paste -sd' ')"
check "a tag that already exists refuses the batch" refused "$( (cd "$repo" && bash scripts/release.sh tag "ways attend" >/dev/null 2>&1) && echo ok || echo refused)"

echo "make"
check "cut-release passes the list as one argument" \
    "bash scripts/release.sh bump \"ways attend\" \"minor\"" \
    "$(make -s -n -C "$ROOT" cut-release COMPONENT="ways attend" LEVEL=minor 2>/dev/null)"
check "publish-release passes the list as one argument" \
    "bash scripts/release.sh tag \"ways attend\" --push" \
    "$(make -s -n -C "$ROOT" publish-release COMPONENT="ways attend" PUSH=1 2>/dev/null)"

exit $fail
