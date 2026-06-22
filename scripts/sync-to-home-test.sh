#!/usr/bin/env bash
# Smoke test for scripts/sync-to-home.sh (ADR-140).
#
# Exercises the deterministic projection against a throwaway source repo and an
# empty target home — the test the broken jq merge in #182 never had because the
# logic lived in command prose. Asserts: valid settings.json, hooks quoted, ways
# permissions present, user settings preserved, projection landed, marker stamped,
# and idempotency across a second run.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
SYNC="$SCRIPT_DIR/sync-to-home.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
pass() { printf 'ok: %s\n' "$*"; }

# --- Build a minimal fake agent-ways source repo ---
SRC="$WORK/repo"
mkdir -p "$SRC/hooks/ways/meta" "$SRC/skills/demo" "$SRC/agents" "$SRC/commands" "$SRC/bin"
printf '#!/usr/bin/env bash\n' > "$SRC/hooks/check-config-updates.sh"
printf '#!/usr/bin/env bash\n' > "$SRC/hooks/refresh-claude-md.sh"
printf '# way\n' > "$SRC/hooks/ways/meta/note.md"
printf 'skill\n' > "$SRC/skills/demo/SKILL.md"
printf 'cmd\n' > "$SRC/commands/demo.md"
cat > "$SRC/settings.json" <<'JSON'
{
  "hooks": {
    "SessionStart": [
      { "hooks": [ { "type": "command", "command": "${HOME}/.claude/hooks/check-config-updates.sh" } ] }
    ],
    "UserPromptSubmit": [
      { "hooks": [ { "type": "command", "command": "${HOME}/.claude/bin/ways corpus --if-stale --quiet" } ] }
    ]
  },
  "permissions": { "allow": ["Bash(ways:*)"] }
}
JSON
git -C "$SRC" init -q && git -C "$SRC" add -A && git -C "$SRC" -c user.email=t@t -c user.name=t commit -qm init

# --- Target home with a pre-existing user settings.json ---
DEST="$WORK/home"
mkdir -p "$DEST"
cat > "$DEST/settings.json" <<'JSON'
{ "model": "claude-opus-4-8", "theme": "dark", "permissions": { "allow": ["Bash(git:*)"] } }
JSON

run() { CLAUDE_HOME="$DEST" SYNC_SRC="$SRC" bash "$SYNC" >/dev/null; }

# --- Run 1 ---
run
jq -e . "$DEST/settings.json" >/dev/null 2>&1 || fail "settings.json is not valid JSON after run 1"
pass "settings.json valid"

# user keys preserved
[[ "$(jq -r .model "$DEST/settings.json")" == "claude-opus-4-8" ]] || fail "model key not preserved"
[[ "$(jq -r .theme "$DEST/settings.json")" == "dark" ]] || fail "theme key not preserved"
pass "user settings (model/theme) preserved"

# ways permissions merged + user's own allow kept, deduped
allow="$(jq -c '.permissions.allow' "$DEST/settings.json")"
for p in 'Bash(ways:*)' 'Bash(attend:*)' 'Bash(way-embed:*)' 'Edit(~/.claude/**)' 'Bash(git:*)'; do
  echo "$allow" | grep -qF "$p" || fail "permission missing: $p"
done
[[ "$(jq '[.permissions.allow[] | select(. == "Bash(ways:*)")] | length' "$DEST/settings.json")" == "1" ]] || fail "Bash(ways:*) not deduped"
pass "permissions merged + deduped, user grants preserved"

# hook commands quoted
cmd1="$(jq -r '.hooks.SessionStart[0].hooks[0].command' "$DEST/settings.json")"
cmd2="$(jq -r '.hooks.UserPromptSubmit[0].hooks[0].command' "$DEST/settings.json")"
[[ "$cmd1" == '"${HOME}/.claude/hooks/check-config-updates.sh"' ]] || fail "no-arg command not quoted: $cmd1"
[[ "$cmd2" == '"${HOME}/.claude/bin/ways" corpus --if-stale --quiet' ]] || fail "arg-bearing command quoted wrong: $cmd2"
pass "hook commands quoted correctly (bare path + arg-bearing)"

# projection landed
[[ -f "$DEST/skills/demo/SKILL.md" ]] || fail "skills not projected"
[[ -f "$DEST/commands/demo.md" ]] || fail "commands not projected"
[[ -f "$DEST/hooks/ways/meta/note.md" ]] || fail "hooks/ways not projected"
[[ -f "$DEST/hooks/check-config-updates.sh" ]] || fail "top-level hook not projected"
pass "skills/agents/commands/hooks projected"

# marker stamped
[[ -f "$DEST/.claude-source" ]] || fail ".claude-source marker not written"
grep -q "^source=$SRC$" "$DEST/.claude-source" || fail "marker source path wrong"
grep -q "^head=$(git -C "$SRC" rev-parse HEAD)$" "$DEST/.claude-source" || fail "marker head sha wrong"
grep -q "^mode=copy$" "$DEST/.claude-source" || fail "marker mode wrong"
pass "marker + synced-HEAD stamp written"

# --- Run 2: idempotency ---
h1="$(sha256sum "$DEST/settings.json" | cut -d' ' -f1)"
run
h2="$(sha256sum "$DEST/settings.json" | cut -d' ' -f1)"
[[ "$h1" == "$h2" ]] || fail "settings.json not idempotent across re-run"
pass "idempotent settings.json across re-run"

# --- Topology guard: SRC == DEST is a no-op ---
out="$(CLAUDE_HOME="$SRC" SYNC_SRC="$SRC" bash "$SYNC")"
echo "$out" | grep -q "canonical in-place install" || fail "topology guard did not fire for SRC==DEST"
pass "topology guard: SRC==DEST is a no-op"

echo "ALL PASS"
