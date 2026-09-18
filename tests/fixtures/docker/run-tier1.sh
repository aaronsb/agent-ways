#!/usr/bin/env bash
# Tier 1 runner (ADR-186): install and configure agent-ways on a clean Debian
# home with no API key, then assert the state the reviews of #501, #502 and
# #504 asked for. Runs inside the fixture container; see compose.yaml.
#
# Environment:
#   FLAVOR          branch (default) or release
#   CLAUDE_VERSION  expected `claude --version`; `latest` skips the exact match
#   GH_TOKEN        for the release-asset downloads
#
# Mounts: /src (the checkout, branch flavor), /binaries (ways, ways-audit,
# attend, attend-chat built for the same commit), /fixture (this directory).

set -uo pipefail

FLAVOR="${FLAVOR:-branch}"
CLAUDE_VERSION="${CLAUDE_VERSION:-latest}"
FIX=/fixture
APP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/agent-ways"
CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}/agent-ways/config.yaml"
DEST="$HOME/.claude"
WORK="$HOME/.claude-work"
PROJECT="$HOME/project"
LOG="$HOME/tier1.log"

PASS=0
FAIL=0
FAILED=()

# --- assertion helpers ---------------------------------------------------

ok()   { PASS=$((PASS + 1)); printf '  ok    %s\n' "$1"; }
fail() { FAIL=$((FAIL + 1)); FAILED+=("$1"); printf '  FAIL  %s\n' "$1"; [[ $# -gt 1 ]] && printf '        %s\n' "${@:2}"; }

# assert NAME COMMAND...  — passes when the command exits 0
assert() {
  local name="$1"; shift
  if "$@" >/dev/null 2>&1; then ok "$name"; else fail "$name" "command: $*"; fi
}

# assert_eq NAME EXPECTED ACTUAL
assert_eq() {
  if [[ "$2" == "$3" ]]; then ok "$1"; else fail "$1" "expected: $2" "actual:   $3"; fi
}

# assert_contains NAME NEEDLE HAYSTACK
assert_contains() {
  if [[ "$3" == *"$2"* ]]; then ok "$1"; else fail "$1" "missing: $2"; fi
}

section() { printf '\n== %s\n' "$1"; }

# Hash of a whole tree: every regular file's path and content, in path order.
tree_hash() {
  (cd "$1" && find . -type f | LC_ALL=C sort | while read -r f; do sha256sum "$f"; done) | sha256sum | cut -d' ' -f1
}

file_hash() { sha256sum "$1" | cut -d' ' -f1; }

# --- hook driver: run an event the way Claude Code does --------------------
#
# Reads the hook commands for EVENT out of the merged settings.json, keeps the
# entries whose matcher is absent or matches NAME (the SessionStart source or
# the tool name), expands ${HOME}, and pipes PAYLOAD to each command's stdin.
# Prints the concatenated stdout; records every exit code in HOOK_EXITS.
HOOK_EXITS=""
run_event() {
  local event="$1" name="$2" payload="$3" cmd expanded rc
  HOOK_EXITS=""
  while IFS= read -r cmd; do
    [[ -z "$cmd" ]] && continue
    expanded="${cmd//\$\{HOME\}/$HOME}"
    (cd "$PROJECT" && CLAUDE_PROJECT_DIR="$PROJECT" bash -c "$expanded" < "$payload" 2>>"$LOG")
    rc=$?
    HOOK_EXITS+="$rc:$expanded"$'\n'
  done < <(jq -r --arg ev "$event" --arg name "$name" '
      .hooks[$ev][]?
      | (.matcher // "") as $m
      | select($m == "" or ($name | test("^(" + $m + ")$")))
      | .hooks[].command' "$DEST/settings.json")
}

nonzero_hooks() { printf '%s' "$HOOK_EXITS" | grep -v '^0:' || true; }

# --- 0. preflight ----------------------------------------------------------

section "preflight ($FLAVOR flavor, Claude Code $CLAUDE_VERSION)"
: > "$LOG"
assert "claude is on PATH" command -v claude
if [[ -z "${GH_TOKEN:-}" ]]; then
  echo "  warn  GH_TOKEN is empty: release-asset downloads through gh will fail"
fi
if [[ "$FLAVOR" == "branch" ]]; then
  assert "checkout mounted at /src" test -f /src/hooks/check-config-updates.sh
  for b in ways ways-audit attend attend-chat; do
    assert "binary mounted: $b" test -x "/binaries/$b"
  done
fi

# --- 1. seed the home ------------------------------------------------------

section "seed the home"
mkdir -p "$DEST" "$WORK" "$PROJECT"
cp -r "$FIX/seed/claude/." "$DEST/"
cp -r "$FIX/seed/claude-work/." "$WORK/"
(cd "$PROJECT" && git init -q && printf '# project\n' > README.md && git add -A && git commit -qm init)
SEED_SETTINGS_HASH=$(file_hash "$DEST/settings.json")
SEED_SKILL_HASH=$(file_hash "$DEST/skills/my-skill/SKILL.md")
SEED_WORK_HASH=$(tree_hash "$WORK")
assert "real skills dir seeded" test -d "$DEST/skills/my-skill"
assert "second config dir seeded" test -f "$WORK/settings.json"

# --- 2. install, unattended ------------------------------------------------

section "installer, unattended"
case "$FLAVOR" in
  branch)
    # A clone carries the committed HEAD of the checkout; uncommitted edits
    # to hooks or scripts are not under test until they are committed.
    git clone -q /src "$APP_DIR"
    mkdir -p "$APP_DIR/bin"
    cp /binaries/ways /binaries/ways-audit /binaries/attend /binaries/attend-chat "$APP_DIR/bin/"
    chmod +x "$APP_DIR"/bin/*
    # The installer takes the working directory as the source when run bare.
    (cd "$APP_DIR" && bash scripts/install.sh < /dev/null) > "$HOME/install.out" 2>&1
    INSTALL_RC=$?
    ;;
  release)
    curl -fsSL https://raw.githubusercontent.com/aaronsb/agent-ways/main/scripts/install.sh \
      | bash -s -- --bootstrap > "$HOME/install.out" 2>&1
    INSTALL_RC=$?
    ;;
  *)
    echo "unknown FLAVOR: $FLAVOR" >&2; exit 2 ;;
esac
INSTALL_OUT=$(cat "$HOME/install.out")

assert_eq "installer exits 1 on the real skills dir" "1" "$INSTALL_RC"
assert_contains "installer names the refusal" "Projection stopped" "$INSTALL_OUT"
assert "app staged" test -x "$APP_DIR/bin/ways"
assert "skills dir is still a real directory" test -d "$DEST/skills" -a ! -L "$DEST/skills"
assert_eq "user skill untouched" "$SEED_SKILL_HASH" "$(file_hash "$DEST/skills/my-skill/SKILL.md")"
assert_eq "settings.json untouched by a refused run" "$SEED_SETTINGS_HASH" "$(file_hash "$DEST/settings.json")"
assert "no settings backup written" test ! -e "$DEST/settings.json.bak"
assert "no hooks linked" test ! -e "$DEST/hooks/ways"
assert "ways on PATH" command -v ways

# --- 3. the documented recovery: reconcile --force --------------------------

section "recovery: ways reconcile --force"
FORCE_OUT=$(ways reconcile --force 2>&1)
FORCE_RC=$?
assert_eq "reconcile --force exits 0" "0" "$FORCE_RC"
assert "skills is now our symlink" test -L "$DEST/skills"
assert_eq "skills resolves into the app" "$APP_DIR/skills" "$(readlink -f "$DEST/skills")"
BACKUP=$(ls -d "$DEST"/skills.ways-backup-* 2>/dev/null | head -1)
assert "user skills moved aside, not deleted" test -n "$BACKUP" -a -f "$BACKUP/my-skill/SKILL.md"
[[ -n "$BACKUP" ]] && assert_eq "user skill intact in the backup" "$SEED_SKILL_HASH" "$(file_hash "$BACKUP/my-skill/SKILL.md")"
assert "hooks/ways linked" test -L "$DEST/hooks/ways"
assert "bin/ways linked" test -x "$DEST/bin/ways"

# --- 4. settings.json shape ------------------------------------------------

section "settings.json merge"
assert "settings.json is valid JSON" jq -e . "$DEST/settings.json"
assert "backup of the pre-merge file exists" test -f "$DEST/settings.json.bak"
assert_eq "backup equals the seed" "$SEED_SETTINGS_HASH" "$(file_hash "$DEST/settings.json.bak")"
assert_eq "model key kept" "opus" "$(jq -r .model "$DEST/settings.json")"
for ev in UserPromptSubmit SessionStart Stop; do
  seeded=$(jq -c --arg ev "$ev" '.hooks[$ev][0]' "$FIX/seed/claude/settings.json")
  assert "user hook kept by identity: $ev" \
    jq -e --arg ev "$ev" --argjson e "$seeded" '.hooks[$ev] | index($e) != null' "$DEST/settings.json"
done
assert "our SessionStart hooks present" \
  jq -e '[.hooks.SessionStart[].hooks[].command] | any(contains("/hooks/ways/check-setup.sh"))' "$DEST/settings.json"
assert "our UserPromptSubmit hook present" \
  jq -e '[.hooks.UserPromptSubmit[].hooks[].command] | any(contains("/hooks/ways/check-prompt.sh"))' "$DEST/settings.json"
assert "our PreToolUse Bash hook present" \
  jq -e '[.hooks.PreToolUse[] | select(.matcher == "Bash") | .hooks[].command] | any(contains("/hooks/ways/check-bash-pre.sh"))' "$DEST/settings.json"
assert "permissions merged" jq -e '.permissions.allow | index("Bash(ways:*)") != null' "$DEST/settings.json"

# --- 5. target recorded ----------------------------------------------------

section "target recorded (ADR-184)"
assert "user config exists" test -f "$CONFIG"
assert "targets key written" grep -q '^targets:' "$CONFIG"
TARGETS=$(ways config targets --json 2>/dev/null)
assert_eq "targets are explicit" "true" "$(jq -r .explicit <<<"$TARGETS")"
assert_eq "one target" "1" "$(jq -r '.targets | length' <<<"$TARGETS")"
assert_eq "target is ~/.claude" "$DEST" "$(jq -r '.targets[0].dir' <<<"$TARGETS")"
assert_eq "target enabled" "true" "$(jq -r '.targets[0].enabled' <<<"$TARGETS")"
assert_eq "target active" "active" "$(jq -r '.targets[0].state' <<<"$TARGETS")"
assert "ways config targets renders" ways config targets

# --- 6. ways status --------------------------------------------------------

section "ways status"
STATUS_TEXT=$(ways status 2>&1); STATUS_RC=$?
assert_eq "ways status exits 0" "0" "$STATUS_RC"
assert_contains "status names the active state" "Install:   active" "$STATUS_TEXT"
STATUS_JSON=$(ways status --json 2>/dev/null)
assert_eq "status --json: install active" "active" "$(jq -r .install.state <<<"$STATUS_JSON")"
assert_eq "status --json: engine embedding" "embedding" "$(jq -r .engine.active <<<"$STATUS_JSON")"

# --- 7. reconcile is idempotent ----------------------------------------------

section "reconcile --dry-run, twice"
DRY1=$(ways reconcile --dry-run 2>&1); DRY1_RC=$?
DRY2=$(ways reconcile --dry-run 2>&1); DRY2_RC=$?
assert_eq "first dry-run exits 0" "0" "$DRY1_RC"
assert_eq "second dry-run exits 0" "0" "$DRY2_RC"
assert_eq "dry-run output identical" "$DRY1" "$DRY2"
assert_contains "dry-run reports up to date" "up to date" "$DRY1"
if [[ "$DRY1" == *refused* ]]; then fail "dry-run reports no refusal" "$DRY1"; else ok "dry-run reports no refusal"; fi
SETTINGS_AFTER=$(file_hash "$DEST/settings.json")
assert "bare reconcile exits 0" ways reconcile --quiet
assert_eq "bare reconcile leaves settings.json alone" "$SETTINGS_AFTER" "$(file_hash "$DEST/settings.json")"

# --- 8. the second config dir ----------------------------------------------

section "second config directory"
assert_eq "~/.claude-work untouched" "$SEED_WORK_HASH" "$(tree_hash "$WORK")"

# --- 9. attend and claude --------------------------------------------------

section "attend and claude"
assert "attend on PATH" command -v attend
assert "attend status exits 0" attend status
CLAUDE_OUT=$(claude --version 2>&1); CLAUDE_RC=$?
assert_eq "claude --version exits 0" "0" "$CLAUDE_RC"
if [[ "$CLAUDE_VERSION" != "latest" ]]; then
  assert_contains "claude is the pinned version" "$CLAUDE_VERSION" "$CLAUDE_OUT"
else
  echo "  info  claude --version: $CLAUDE_OUT"
fi

# --- 10. hooks driven with synthetic payloads --------------------------------

section "hooks: SessionStart (startup)"
OUT=$(run_event SessionStart startup "$FIX/payloads/session-start.json")
assert_contains "ways table injected" "## Available Ways" "$OUT"
assert_contains "core guidance injected" "epistemic:" "$OUT"
assert_contains "user hook still runs" "USER-HOOK-SESSION-START" "$OUT"
NZ=$(nonzero_hooks); if [[ -z "$NZ" ]]; then ok "every SessionStart hook exits 0"; else fail "every SessionStart hook exits 0" "$NZ"; fi

section "hooks: UserPromptSubmit"
OUT=$(run_event UserPromptSubmit "" "$FIX/payloads/user-prompt.json")
assert_contains "hookSpecificOutput emitted" '"hookEventName":"UserPromptSubmit"' "$OUT"
assert_contains "ADR way disclosed on an ADR prompt" "# ADR Way" "$OUT"
assert_contains "user hook still runs" "USER-HOOK-PROMPT" "$OUT"
NZ=$(nonzero_hooks); if [[ -z "$NZ" ]]; then ok "every UserPromptSubmit hook exits 0"; else fail "every UserPromptSubmit hook exits 0" "$NZ"; fi

section "hooks: PreToolUse Bash"
OUT=$(run_event PreToolUse Bash "$FIX/payloads/pre-tool-use-bash.json")
assert_contains "decision approve" '"decision":"approve"' "$OUT"
assert_contains "gitconfig way disclosed on git config --global" "# Global Git Config" "$OUT"
NZ=$(nonzero_hooks); if [[ -z "$NZ" ]]; then ok "every PreToolUse Bash hook exits 0"; else fail "every PreToolUse Bash hook exits 0" "$NZ"; fi

section "hooks: PreToolUse Edit"
OUT=$(run_event PreToolUse Edit "$FIX/payloads/pre-tool-use-edit.json")
assert_contains "validate way disclosed on README.md" "# Validating Documentation" "$OUT"
NZ=$(nonzero_hooks); if [[ -z "$NZ" ]]; then ok "every PreToolUse Edit hook exits 0"; else fail "every PreToolUse Edit hook exits 0" "$NZ"; fi

# --- summary ---------------------------------------------------------------

printf '\n== tier 1: %d passed, %d failed\n' "$PASS" "$FAIL"
if [[ $FAIL -gt 0 ]]; then
  printf '  - %s\n' "${FAILED[@]}"
  printf '\n-- installer output --\n'; cat "$HOME/install.out"
  printf '\n-- reconcile --force output --\n'; printf '%s\n' "${FORCE_OUT:-}"
  printf '\n-- merged settings.json hooks --\n'
  jq -c '.hooks | to_entries[] | {event: .key, commands: [.value[].hooks[].command]}' "$DEST/settings.json" 2>/dev/null
  printf '\n-- ways status --\n'; ways status 2>&1
  printf '\n-- hook stderr --\n'; tail -n 60 "$LOG"
  exit 1
fi
