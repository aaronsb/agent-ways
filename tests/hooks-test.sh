#!/usr/bin/env bash
# Hook scripts under hooks/ways, run against a temp HOME and sessions root:
# clear-markers.sh clears only its own session's state, and the memory macro
# finds MEMORY.md under Claude Code's project slug.

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

# Every hook here resolves its state from HOME and XDG_RUNTIME_DIR.
export HOME="$WORK/home" XDG_RUNTIME_DIR="$WORK/run"
unset CLAUDE_PROJECT_DIR
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

exit $fail
