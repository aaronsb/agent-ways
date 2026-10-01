#!/usr/bin/env bash
# Hook scripts under hooks/ways, run against a temp HOME, XDG dirs and
# sessions root: clear-markers.sh clears only its own session's state, the
# memory macro finds MEMORY.md under Claude Code's project slug, and
# inject-subagent.sh injects stashed ways from every ways root. The hooks
# call the ways binary at $HOME/.claude/bin/ways; the test links the build
# named by $WAYS_TEST_BIN (default tools/target/debug/ways) there.

set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
HOOKS="$ROOT/hooks/ways"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
fail=0

check() {  # name expected actual
    if [[ "$2" == "$3" ]]; then
        echo "  PASS: $1"
    else
        echo "  FAIL: $1 — expected [$2], got [$3]"
        fail=1
    fi
}

WAYS_TEST_BIN="${WAYS_TEST_BIN:-$ROOT/tools/target/debug/ways}"
if [[ ! -x "$WAYS_TEST_BIN" ]]; then
    echo "  FAIL: no ways binary at $WAYS_TEST_BIN (cargo build -p ways)"
    exit 1
fi

# Every hook here resolves its state from HOME, XDG_* and XDG_RUNTIME_DIR.
export HOME="$WORK/home" XDG_RUNTIME_DIR="$WORK/run"
export XDG_CONFIG_HOME="$WORK/config" XDG_STATE_HOME="$WORK/state"
export XDG_CACHE_HOME="$WORK/cache" XDG_DATA_HOME="$WORK/data"
unset CLAUDE_PROJECT_DIR CLAUDE_AGENT_ID
mkdir -p "$HOME/.claude/bin"
ln -s "$WAYS_TEST_BIN" "$HOME/.claude/bin/ways"
SESSIONS="$XDG_RUNTIME_DIR/claude-sessions"

seed_sessions() {
    rm -rf "$SESSIONS"
    mkdir -p "$SESSIONS/sess-a/way-epochs" "$SESSIONS/sess-b/way-epochs"
}

# clear-markers.sh with a session id clears that session alone.
seed_sessions
echo '{"session_id":"sess-a","cwd":"/srv/p"}' | bash "$HOOKS/clear-markers.sh"
check "clear-markers removes its own session" "absent" "$([[ -e $SESSIONS/sess-a ]] && echo present || echo absent)"
check "clear-markers keeps another session" "present" "$([[ -e $SESSIONS/sess-b ]] && echo present || echo absent)"

# Without a session id it has nothing to clear, so every session survives.
seed_sessions
echo '{"cwd":"/srv/p"}' | bash "$HOOKS/clear-markers.sh"
check "clear-markers without a session id keeps every session" "2" "$(find "$SESSIONS" -mindepth 1 -maxdepth 1 | wc -l | tr -d ' ')"

# A session id that is not a plain name never reaches rm.
seed_sessions
echo '{"session_id":".."}' | bash "$HOOKS/clear-markers.sh"
check "clear-markers ignores a session id of .." "present" "$([[ -d $SESSIONS ]] && echo present || echo absent)"

# The memory macro reads MEMORY.md from the dir Claude Code names the project
# by: every non-alphanumeric character becomes '-'.
mkdir -p "$HOME/.claude/projects/-srv-mcp--prod-x-y/memory"
printf 'one\ntwo\n' > "$HOME/.claude/projects/-srv-mcp--prod-x-y/memory/MEMORY.md"
out=$(PATH="/usr/bin:/bin" CLAUDE_PROJECT_DIR="/srv/mcp/_prod/x.y" bash "$HOOKS/meta/memory/macro.sh")
check "memory macro finds an underscore project's MEMORY.md" "**MEMORY.md has 2 lines.**" "${out%% Review*}"

# inject-subagent.sh emits each stashed way, from the user root
# ($XDG_CONFIG_HOME/agent-ways/ways, ADR-143) as from the core root, with
# its frontmatter stripped and its macro output in place.
way() {  # root id scope body
    mkdir -p "$1/$2"
    printf -- '---\ndescription: test way\nscope: %s\nrefire: 0.15\n---\n%s\n' "$3" "$4" \
        > "$1/$2/$(basename "$2").md"
}
way "$XDG_CONFIG_HOME/agent-ways/ways" userdom/mine "subagent" "# User way body"
way "$HOME/.claude/hooks/ways" coredom/shipped "agent, subagent" "# Core way body"
printf -- '---\ndescription: test way\nscope: subagent\nrefire: 0.15\nmacro: append\n---\n# Macro way body\n' \
    > "$(mkdir -p "$HOME/.claude/hooks/ways/coredom/withmacro" && echo "$HOME/.claude/hooks/ways/coredom/withmacro")/withmacro.md"
printf '#!/bin/bash\necho macro-output\n' > "$HOME/.claude/hooks/ways/coredom/withmacro/macro.sh"
chmod +x "$HOME/.claude/hooks/ways/coredom/withmacro/macro.sh"

inject() {  # session -> additionalContext
    mkdir -p "$SESSIONS/$1/subagent-stash"
    echo '{"ways":["userdom/mine","coredom/shipped","coredom/withmacro"],"channels":["prompt","prompt","prompt"]}' \
        > "$SESSIONS/$1/subagent-stash/001.json"
    echo "{\"session_id\":\"$1\",\"agent_id\":\"agent-1\",\"cwd\":\"$WORK/project\"}" \
        | bash "$HOOKS/inject-subagent.sh" | jq -r '.hookSpecificOutput.additionalContext // empty'
}
rm -rf "$SESSIONS"
ctx=$(inject sess-sub)
expected=$'# User way body\n\n# Core way body\n\n# Macro way body\nmacro-output'
check "inject-subagent injects user, core and macro ways" "$expected" "$ctx"
# A second subagent in the same session gets the same ways again.
check "inject-subagent ignores the parent's refire state" "$expected" "$(inject sess-sub)"
check "inject-subagent logs each injection" "6" \
    "$(jq -r 'select(.event=="way_fired" and .scope=="subagent") | .way' "$XDG_STATE_HOME/agent-ways/events.jsonl" 2>/dev/null | wc -l | tr -d ' ')"

exit $fail
